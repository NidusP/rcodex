//! 官方账户刷新、断开和删除。
//!
//! 不启动新的授权流程。由 OfficialAccountService 对外方法调用。

use std::fs;
use std::path::PathBuf;

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use crate::database::resource_guard::{
    release_operation_leases_in_transaction, ResourceKind, ResourceRef,
};
use crate::official_auth::{AccountRateLimits, AccountSnapshot};

use super::models::{
    account_projection, auth_error_code, current_resources, insert_quota_snapshot,
    managed_home_matches, now_seconds, required_id, required_path, service_error,
    AccountActionInput, OfficialAccountError, OfficialAccountStatus, OP_DELETE, OP_DISCONNECT,
    OP_REFRESH,
};
use super::OfficialAccountService;

struct CredentialRevision {
    id: String,
    managed_home: PathBuf,
    #[allow(dead_code)]
    auth_state: String,
}

impl OfficialAccountService {
    pub fn refresh_account_status(
        &self,
        input: AccountActionInput,
    ) -> Result<OfficialAccountStatus, OfficialAccountError> {
        let account_id = required_id("account_id", &input.account_id)?;
        let executable = required_path("codex_executable", &input.codex_executable)?;
        let current = self
            .current_revision(&account_id)?
            .ok_or_else(|| service_error("official_account_needs_reauth", false))?;
        let resources = current_resources(&account_id, &current.id);
        self.guard.assert_resources_mutable(&resources)?;
        if !managed_home_matches(
            &self.managed_homes_root,
            &account_id,
            &current.id,
            &current.managed_home,
        ) {
            return Err(service_error("managed_home_invalid", false));
        }
        let operation_id = Uuid::new_v4().to_string();
        self.guard.acquire_operation_leases(
            &operation_id,
            OP_REFRESH,
            Some(std::process::id() as i64),
            Some("official status refresh"),
            &resources,
            now_seconds(),
        )?;

        let result = (|| {
            let probed = self.auth.probe(&executable)?;
            let mut session = probed.start_session(&current.managed_home)?;
            let snapshot = session.refresh_account()?;
            let quota = session.read_rate_limits().ok().flatten();
            Ok((snapshot, quota))
        })();
        match result {
            Ok((snapshot, quota)) => self.commit_refreshed_account(
                &operation_id,
                &account_id,
                &current.id,
                snapshot,
                quota,
            ),
            Err(error) => {
                let code = auth_error_code(&error);
                self.record_refresh_error(&operation_id, &current.id, code)?;
                Err(OfficialAccountError::from_auth(error))
            }
        }
    }

    pub fn disconnect_account(
        &self,
        input: AccountActionInput,
    ) -> Result<OfficialAccountStatus, OfficialAccountError> {
        let account_id = required_id("account_id", &input.account_id)?;
        let Some(current) = self.current_revision(&account_id)? else {
            return self.account_status(&account_id);
        };
        let resources = current_resources(&account_id, &current.id);
        self.guard.assert_resources_mutable(&resources)?;
        if !managed_home_matches(
            &self.managed_homes_root,
            &account_id,
            &current.id,
            &current.managed_home,
        ) {
            return Err(service_error("managed_home_invalid", false));
        }
        let executable = required_path("codex_executable", &input.codex_executable)?;
        let operation_id = Uuid::new_v4().to_string();
        self.guard.acquire_operation_leases(
            &operation_id,
            OP_DISCONNECT,
            Some(std::process::id() as i64),
            Some("official local logout"),
            &resources,
            now_seconds(),
        )?;

        let logout = (|| {
            let probed = self.auth.probe(&executable)?;
            let mut session = probed.start_session(&current.managed_home)?;
            session.logout()
        })();
        if let Err(error) = logout {
            let _ = self.guard.release_operation_leases(&operation_id);
            return Err(OfficialAccountError::from_auth(error));
        }
        self.commit_disconnected(&operation_id, &account_id, &current.id)?;
        self.account_status(&account_id)
    }

    pub fn delete_account(
        &self,
        input: AccountActionInput,
    ) -> Result<OfficialAccountStatus, OfficialAccountError> {
        let account_id = required_id("account_id", &input.account_id)?;
        let revisions = self.account_revisions(&account_id)?;
        let operation_id = Uuid::new_v4().to_string();
        let mut resources = vec![ResourceRef::logical(
            ResourceKind::OfficialAccount,
            account_id.clone(),
        )];
        resources.extend(revisions.iter().map(|revision| {
            ResourceRef::revision(
                ResourceKind::OfficialCredentialRevision,
                revision.id.clone(),
                revision.id.clone(),
            )
        }));
        self.guard.acquire_operation_leases(
            &operation_id,
            OP_DELETE,
            Some(std::process::id() as i64),
            Some("official account tombstone"),
            &resources,
            now_seconds(),
        )?;

        for revision in &revisions {
            if !managed_home_matches(
                &self.managed_homes_root,
                &account_id,
                &revision.id,
                &revision.managed_home,
            ) {
                let _ = self.guard.release_operation_leases(&operation_id);
                return Err(service_error("managed_home_invalid", false));
            }
        }

        self.commit_tombstone(&operation_id, &account_id)?;
        for revision in &revisions {
            if revision.managed_home.is_dir() {
                fs::remove_dir_all(&revision.managed_home)
                    .map_err(|_| service_error("managed_home_cleanup_failed", true))?;
            }
        }
        self.account_status(&account_id)
    }
    fn current_revision(
        &self,
        account_id: &str,
    ) -> Result<Option<CredentialRevision>, OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let account_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_accounts WHERE id=?1)",
                [account_id],
                |row| row.get(0),
            )
            .map_err(|_| service_error("database_error", true))?;
        if !account_exists {
            return Err(service_error("official_account_not_found", false));
        }
        connection
            .query_row(
                "SELECT r.id, r.managed_codex_home, r.auth_state
                 FROM codex_accounts a
                 JOIN official_credential_revisions r
                   ON r.id=a.current_credential_revision_id
                 WHERE a.id=?1",
                [account_id],
                |row| {
                    Ok(CredentialRevision {
                        id: row.get(0)?,
                        managed_home: PathBuf::from(row.get::<_, String>(1)?),
                        auth_state: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|_| service_error("database_error", true))
    }

    fn account_revisions(
        &self,
        account_id: &str,
    ) -> Result<Vec<CredentialRevision>, OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM codex_accounts WHERE id=?1)",
                [account_id],
                |row| row.get(0),
            )
            .map_err(|_| service_error("database_error", true))?;
        if !exists {
            return Err(service_error("official_account_not_found", false));
        }
        let mut statement = connection
            .prepare(
                "SELECT id, managed_codex_home, auth_state
                 FROM official_credential_revisions WHERE account_id=?1
                 ORDER BY created_at ASC",
            )
            .map_err(|_| service_error("database_error", true))?;
        let rows = statement
            .query_map([account_id], |row| {
                Ok(CredentialRevision {
                    id: row.get(0)?,
                    managed_home: PathBuf::from(row.get::<_, String>(1)?),
                    auth_state: row.get(2)?,
                })
            })
            .map_err(|_| service_error("database_error", true))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| service_error("database_error", true))?;
        Ok(rows)
    }

    fn commit_refreshed_account(
        &self,
        operation_id: &str,
        account_id: &str,
        revision_id: &str,
        snapshot: AccountSnapshot,
        quota: Option<AccountRateLimits>,
    ) -> Result<OfficialAccountStatus, OfficialAccountError> {
        let projection = match account_projection(snapshot) {
            Ok(projection) => projection,
            Err(error) => {
                self.mark_needs_reauth(operation_id, revision_id, &error.code)?;
                return Err(error);
            }
        };
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let now = now_seconds();
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='authorized', auth_checked_at=?2, auth_error_code=NULL
                 WHERE id=?1",
                params![revision_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        if let Some(quota) = quota {
            insert_quota_snapshot(&transaction, account_id, revision_id, &quota, now)?;
        }
        transaction
            .execute(
                "UPDATE codex_accounts SET masked_identity=?2, plan_label=?3, updated_at=?4
                 WHERE id=?1",
                params![
                    account_id,
                    projection.masked_identity,
                    projection.plan_label,
                    now,
                ],
            )
            .map_err(|_| service_error("database_error", true))?;
        release_operation_leases_in_transaction(&transaction, operation_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))?;
        drop(connection);
        self.account_status(account_id)
    }

    fn record_refresh_error(
        &self,
        operation_id: &str,
        revision_id: &str,
        error_code: &str,
    ) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_checked_at=?2, auth_error_code=?3 WHERE id=?1",
                params![revision_id, now_seconds(), error_code],
            )
            .map_err(|_| service_error("database_error", true))?;
        release_operation_leases_in_transaction(&transaction, operation_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))
    }

    fn mark_needs_reauth(
        &self,
        operation_id: &str,
        revision_id: &str,
        error_code: &str,
    ) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='needs_reauth', auth_checked_at=?2,
                     auth_error_code=?3, retired_at=NULL WHERE id=?1",
                params![revision_id, now_seconds(), error_code],
            )
            .map_err(|_| service_error("database_error", true))?;
        release_operation_leases_in_transaction(&transaction, operation_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))
    }

    fn commit_disconnected(
        &self,
        operation_id: &str,
        account_id: &str,
        revision_id: &str,
    ) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let now = now_seconds();
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='needs_reauth', auth_checked_at=?2,
                     auth_error_code='official_account_disconnected', retired_at=NULL
                 WHERE id=?1",
                params![revision_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE codex_accounts
                 SET current_credential_revision_id=NULL, updated_at=?2 WHERE id=?1",
                params![account_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        release_operation_leases_in_transaction(&transaction, operation_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))
    }

    fn commit_tombstone(
        &self,
        operation_id: &str,
        account_id: &str,
    ) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let now = now_seconds();
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='retired', auth_checked_at=?2,
                     auth_error_code='official_account_deleted', retired_at=?2
                 WHERE account_id=?1 AND auth_state != 'retired'",
                params![account_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE codex_accounts
                 SET masked_identity=NULL, workspace_label=NULL, plan_label=NULL,
                     current_credential_revision_id=NULL, updated_at=?2 WHERE id=?1",
                params![account_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        release_operation_leases_in_transaction(&transaction, operation_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))
    }

    fn account_status(
        &self,
        account_id: &str,
    ) -> Result<OfficialAccountStatus, OfficialAccountError> {
        self.database
            .connection()
            .lock()
            .map_err(|_| service_error("database_error", true))?
            .query_row(
                "SELECT a.id, a.masked_identity, a.workspace_label, a.plan_label,
                        a.current_credential_revision_id,
                        CASE
                          WHEN a.current_credential_revision_id IS NULL
                           AND EXISTS (
                             SELECT 1 FROM official_credential_revisions deleted
                             WHERE deleted.account_id=a.id
                               AND deleted.auth_error_code='official_account_deleted'
                           )
                          THEN 'deleted'
                          ELSE COALESCE(r.auth_state, 'needs_reauth')
                        END,
                        COALESCE(
                          r.auth_error_code,
                          (SELECT 'official_account_deleted'
                           FROM official_credential_revisions deleted
                           WHERE deleted.account_id=a.id
                             AND deleted.auth_error_code='official_account_deleted'
                           LIMIT 1)
                        )
                 FROM codex_accounts a
                 LEFT JOIN official_credential_revisions r
                   ON r.id=a.current_credential_revision_id
                 WHERE a.id=?1",
                [account_id],
                |row| {
                    Ok(OfficialAccountStatus {
                        account_id: row.get(0)?,
                        masked_identity: row.get(1)?,
                        workspace_label: row.get(2)?,
                        plan_label: row.get(3)?,
                        current_credential_revision_id: row.get(4)?,
                        auth_state: row.get(5)?,
                        auth_error_code: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|_| service_error("database_error", true))?
            .ok_or_else(|| service_error("official_account_not_found", false))
    }
}
