//! 官方账户授权：start / poll / cancel 和提示 URL。
//!
//! 不刷新已授权账户、不删除账户。由 OfficialAccountService 对外方法调用。

use std::{fs, path::Path, time::Duration};

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use crate::database::resource_guard::release_operation_leases_in_transaction;
use crate::official_auth::{
    AccountKind, AccountRateLimits, AccountSnapshot, CancelLoginStatus, CodexAppServerProbe,
    LoginCompleted, LoginStart, OfficialAuthError,
};

use super::driver::PendingAuthorization;
use super::models::{
    account_projection, auth_error_code, authorization_resources, insert_quota_snapshot,
    login_completion_error_code, now_seconds, prepare_managed_home, required_id, required_path,
    service_error, validate_workspace_label, AuthorizationMode, AuthorizationPrompt,
    AuthorizationStarted, AuthorizationState, AuthorizationStatus, OfficialAccountError,
    StartAuthorizationInput, AUTH_POLL_TIMEOUT, OP_AUTHORIZE,
};
use super::OfficialAccountService;

enum PollOutcome {
    Completed(PendingAuthorization, LoginCompleted),
    Failed(PendingAuthorization, OfficialAuthError),
}

struct AccountAuthorizationRecord {
    current_revision_id: Option<String>,
}

impl OfficialAccountService {
    pub fn start_authorization(
        &self,
        input: StartAuthorizationInput,
    ) -> Result<AuthorizationStarted, OfficialAccountError> {
        let workspace_label = validate_workspace_label(input.workspace_label)?;
        let account_id = match input.account_id.as_deref() {
            Some(value) => required_id("account_id", value)?,
            None => Uuid::new_v4().to_string(),
        };
        let executable = required_path("codex_executable", &input.codex_executable)?;
        let previous = self.account_for_authorization(&account_id, input.account_id.is_some())?;
        let revision_id = Uuid::new_v4().to_string();
        let attempt_id = Uuid::new_v4().to_string();
        let operation_id = attempt_id.clone();
        let resources = authorization_resources(
            &account_id,
            previous.current_revision_id.as_deref(),
            &revision_id,
        );

        // Fast rejection avoids probing Codex when an active instance or another
        // account operation already owns this resource. Lease acquisition below
        // repeats the check transactionally after the potentially slow probe.
        self.guard.assert_resources_mutable(&resources)?;
        let probed = self
            .auth
            .probe(&executable)
            .map_err(OfficialAccountError::from_auth)?;
        let probe = probed.metadata().clone();
        self.guard.acquire_operation_leases(
            &operation_id,
            OP_AUTHORIZE,
            Some(std::process::id() as i64),
            Some("official authorization pending"),
            &resources,
            now_seconds(),
        )?;

        let managed_home = self.managed_homes_root.join(&account_id).join(&revision_id);
        if let Err(error) = prepare_managed_home(&managed_home) {
            let _ = self.guard.release_operation_leases(&operation_id);
            return Err(error);
        }

        if let Err(error) = self.insert_authorization_records(
            &account_id,
            input.account_id.is_some(),
            workspace_label.as_deref(),
            &revision_id,
            &attempt_id,
            &managed_home,
            &probe,
        ) {
            let _ = self.guard.release_operation_leases(&operation_id);
            let _ = fs::remove_dir_all(&managed_home);
            return Err(error);
        }

        let mut session = match probed.start_session(&managed_home) {
            Ok(session) => session,
            Err(error) => return self.fail_start(attempt_id, error),
        };
        let login = match input.mode {
            AuthorizationMode::Browser => session.start_browser_login(),
            AuthorizationMode::DeviceCode => session.start_device_code_login(),
        };
        let login = match login {
            Ok(login) => login,
            Err(error) => return self.fail_start(attempt_id, error),
        };
        let login_id = login.login_id().to_owned();

        if let Err(error) = self.set_attempt_awaiting(&attempt_id) {
            let _ = session.cancel_login(&login_id);
            let _ = self.finalize_failure(&attempt_id, "database_error");
            return Err(error);
        }

        let transient = match login {
            LoginStart::Browser { auth_url, .. } => AuthorizationPrompt::Browser {
                auth_url: auth_url.to_string(),
            },
            LoginStart::DeviceCode {
                verification_url,
                user_code,
                ..
            } => AuthorizationPrompt::DeviceCode {
                verification_url: verification_url.to_string(),
                user_code,
            },
        };
        let prompt_url = match &transient {
            AuthorizationPrompt::Browser { auth_url } => auth_url.clone(),
            AuthorizationPrompt::DeviceCode {
                verification_url, ..
            } => verification_url.clone(),
        };
        let pending = PendingAuthorization {
            attempt_id: attempt_id.clone(),
            account_id: account_id.clone(),
            revision_id: revision_id.clone(),
            operation_id,
            login_id,
            prompt_url,
            session,
        };
        match self.pending.lock() {
            Ok(mut sessions) => {
                sessions.insert(attempt_id.clone(), pending);
            }
            Err(_) => {
                let mut pending = pending;
                let _ = pending.session.cancel_login(&pending.login_id);
                let _ = self.finalize_failure(&attempt_id, "authorization_session_unavailable");
                return Err(service_error("authorization_session_unavailable", true));
            }
        }

        Ok(AuthorizationStarted {
            attempt_id,
            account_id,
            credential_revision_id: revision_id,
            state: AuthorizationState::AwaitingUser,
            prompt: transient,
        })
    }

    pub fn poll_authorization(
        &self,
        attempt_id: &str,
    ) -> Result<AuthorizationStatus, OfficialAccountError> {
        let attempt_id = required_id("attempt_id", attempt_id)?;
        let outcome = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| service_error("authorization_session_unavailable", true))?;
            let Some(session) = pending.get_mut(&attempt_id) else {
                drop(pending);
                return self.authorization_status(&attempt_id);
            };
            match session
                .session
                .poll_login_completion(&session.login_id, AUTH_POLL_TIMEOUT)
            {
                Ok(None) => return self.authorization_status(&attempt_id),
                Ok(Some(completed)) => {
                    let session = pending
                        .remove(&attempt_id)
                        .expect("pending authorization still exists");
                    PollOutcome::Completed(session, completed)
                }
                Err(error) => {
                    let session = pending
                        .remove(&attempt_id)
                        .expect("pending authorization still exists");
                    PollOutcome::Failed(session, error)
                }
            }
        };

        match outcome {
            PollOutcome::Completed(mut pending, completed) if completed.success => {
                match pending.session.read_account() {
                    Ok(snapshot) => {
                        let quota = pending.session.read_rate_limits().ok().flatten();
                        self.commit_authorized(pending, snapshot, quota)
                    }
                    Err(error) => {
                        let code = auth_error_code(&error);
                        self.finalize_failure(&pending.attempt_id, code)?;
                        self.authorization_status(&pending.attempt_id)
                    }
                }
            }
            PollOutcome::Completed(pending, completed) => {
                self.finalize_failure(
                    &pending.attempt_id,
                    login_completion_error_code(&completed),
                )?;
                self.authorization_status(&pending.attempt_id)
            }
            PollOutcome::Failed(pending, error) => {
                let code = auth_error_code(&error);
                self.finalize_failure(&pending.attempt_id, code)?;
                self.authorization_status(&pending.attempt_id)
            }
        }
    }

    pub fn authorization_prompt_url(
        &self,
        attempt_id: &str,
    ) -> Result<String, OfficialAccountError> {
        let attempt_id = required_id("attempt_id", attempt_id)?;
        let sessions = self
            .pending
            .lock()
            .map_err(|_| service_error("authorization_session_unavailable", true))?;
        sessions
            .get(&attempt_id)
            .map(|pending| pending.prompt_url.clone())
            .ok_or_else(|| service_error("authorization_attempt_not_active", false))
    }

    pub fn cancel_authorization(
        &self,
        attempt_id: &str,
    ) -> Result<AuthorizationStatus, OfficialAccountError> {
        let attempt_id = required_id("attempt_id", attempt_id)?;
        let mut pending = {
            let mut sessions = self
                .pending
                .lock()
                .map_err(|_| service_error("authorization_session_unavailable", true))?;
            let Some(pending) = sessions.remove(&attempt_id) else {
                drop(sessions);
                return self.authorization_status(&attempt_id);
            };
            pending
        };

        if let Ok(Some(completed)) = pending
            .session
            .poll_login_completion(&pending.login_id, Duration::ZERO)
        {
            if completed.success {
                return self.commit_completed_session(pending);
            }
            self.finalize_failure(&attempt_id, login_completion_error_code(&completed))?;
            return self.authorization_status(&attempt_id);
        }

        match pending.session.cancel_login(&pending.login_id) {
            Ok(CancelLoginStatus::Canceled) => {
                self.finalize_cancelled(&pending)?;
            }
            Ok(CancelLoginStatus::NotFound) => {
                if let Ok(snapshot) = pending.session.read_account() {
                    if matches!(snapshot.account, Some(AccountKind::ChatGpt { .. })) {
                        let quota = pending.session.read_rate_limits().ok().flatten();
                        return self.commit_authorized(pending, snapshot, quota);
                    }
                }
                self.finalize_cancelled(&pending)?;
            }
            Err(_) => {
                // Dropping the isolated app-server session terminates the pending login.
                self.finalize_cancelled(&pending)?;
            }
        }
        self.authorization_status(&attempt_id)
    }
    fn fail_start<T>(
        &self,
        attempt_id: String,
        error: OfficialAuthError,
    ) -> Result<T, OfficialAccountError> {
        let code = auth_error_code(&error);
        self.finalize_failure(&attempt_id, code)?;
        Err(OfficialAccountError::from_auth(error))
    }

    fn commit_completed_session(
        &self,
        mut pending: PendingAuthorization,
    ) -> Result<AuthorizationStatus, OfficialAccountError> {
        match pending.session.read_account() {
            Ok(snapshot) => {
                let quota = pending.session.read_rate_limits().ok().flatten();
                self.commit_authorized(pending, snapshot, quota)
            }
            Err(error) => {
                let code = auth_error_code(&error);
                self.finalize_failure(&pending.attempt_id, code)?;
                self.authorization_status(&pending.attempt_id)
            }
        }
    }

    fn insert_authorization_records(
        &self,
        account_id: &str,
        existing_account: bool,
        workspace_label: Option<&str>,
        revision_id: &str,
        attempt_id: &str,
        managed_home: &Path,
        probe: &CodexAppServerProbe,
    ) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let now = now_seconds();
        if existing_account {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM codex_accounts WHERE id=?1)",
                    [account_id],
                    |row| row.get(0),
                )
                .map_err(|_| service_error("database_error", true))?;
            if !exists {
                return Err(service_error("official_account_not_found", false));
            }
            if let Some(label) = workspace_label {
                transaction
                    .execute(
                        "UPDATE codex_accounts SET workspace_label=?2, updated_at=?3 WHERE id=?1",
                        params![account_id, label, now],
                    )
                    .map_err(|_| service_error("database_error", true))?;
            }
        } else {
            transaction
                .execute(
                    "INSERT INTO codex_accounts (
                        id, masked_identity, workspace_label, plan_label,
                        current_credential_revision_id, created_at, updated_at
                     ) VALUES (?1, NULL, ?2, NULL, NULL, ?3, ?3)",
                    params![account_id, workspace_label, now],
                )
                .map_err(|_| service_error("database_error", true))?;
        }
        transaction
            .execute(
                "INSERT INTO official_credential_revisions (
                    id, account_id, managed_codex_home, auth_state, auth_checked_at,
                    auth_error_code, codex_version, schema_fingerprint, created_at, retired_at
                 ) VALUES (?1, ?2, ?3, 'authorizing', NULL, NULL, ?4, ?5, ?6, NULL)",
                params![
                    revision_id,
                    account_id,
                    managed_home.display().to_string(),
                    probe.version,
                    probe.schema_fingerprint,
                    now,
                ],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "INSERT INTO codex_authorization_attempts (
                    id, account_id, credential_revision_id, state, started_at,
                    completed_at, error_code, codex_version, schema_fingerprint
                 ) VALUES (?1, ?2, ?3, 'authorizing', ?4, NULL, NULL, ?5, ?6)",
                params![
                    attempt_id,
                    account_id,
                    revision_id,
                    now,
                    probe.version,
                    probe.schema_fingerprint,
                ],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))
    }

    fn set_attempt_awaiting(&self, attempt_id: &str) -> Result<(), OfficialAccountError> {
        self.database
            .connection()
            .lock()
            .map_err(|_| service_error("database_error", true))?
            .execute(
                "UPDATE codex_authorization_attempts SET state='awaiting_user'
                 WHERE id=?1 AND state='authorizing'",
                [attempt_id],
            )
            .map_err(|_| service_error("database_error", true))?;
        Ok(())
    }

    fn commit_authorized(
        &self,
        pending: PendingAuthorization,
        snapshot: AccountSnapshot,
        quota: Option<AccountRateLimits>,
    ) -> Result<AuthorizationStatus, OfficialAccountError> {
        let projection = match account_projection(snapshot) {
            Ok(projection) => projection,
            Err(error) => {
                self.finalize_failure(&pending.attempt_id, &error.code)?;
                return self.authorization_status(&pending.attempt_id);
            }
        };
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let now = now_seconds();
        let old_revision: Option<String> = transaction
            .query_row(
                "SELECT current_credential_revision_id FROM codex_accounts WHERE id=?1",
                [&pending.account_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| service_error("database_error", true))?
            .flatten();
        if let Some(old_revision) = old_revision.filter(|id| id != &pending.revision_id) {
            transaction
                .execute(
                    "UPDATE official_credential_revisions
                     SET auth_state='retired', retired_at=?2
                     WHERE id=?1 AND auth_state != 'retired'",
                    params![old_revision, now],
                )
                .map_err(|_| service_error("database_error", true))?;
        }
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='authorized', auth_checked_at=?2,
                     auth_error_code=NULL, retired_at=NULL
                 WHERE id=?1",
                params![pending.revision_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        if let Some(quota) = quota {
            insert_quota_snapshot(
                &transaction,
                &pending.account_id,
                &pending.revision_id,
                &quota,
                now,
            )?;
        }
        transaction
            .execute(
                "UPDATE codex_accounts
                 SET masked_identity=?2, plan_label=?3,
                     current_credential_revision_id=?4, updated_at=?5
                 WHERE id=?1",
                params![
                    pending.account_id,
                    projection.masked_identity,
                    projection.plan_label,
                    pending.revision_id,
                    now,
                ],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE codex_authorization_attempts
                 SET state='authorized', completed_at=?2, error_code=NULL
                 WHERE id=?1 AND state IN ('authorizing','awaiting_user','verifying')",
                params![pending.attempt_id, now],
            )
            .map_err(|_| service_error("database_error", true))?;
        release_operation_leases_in_transaction(&transaction, &pending.operation_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))?;
        drop(connection);
        self.authorization_status(&pending.attempt_id)
    }

    fn finalize_failure(
        &self,
        attempt_id: &str,
        error_code: &str,
    ) -> Result<(), OfficialAccountError> {
        self.finalize_attempt(attempt_id, "failed", "failed", error_code)
    }

    fn finalize_cancelled(
        &self,
        pending: &PendingAuthorization,
    ) -> Result<(), OfficialAccountError> {
        self.finalize_attempt(
            &pending.attempt_id,
            "cancelled",
            "failed",
            "authorization_cancelled",
        )
    }

    fn finalize_attempt(
        &self,
        attempt_id: &str,
        attempt_state: &str,
        revision_state: &str,
        error_code: &str,
    ) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let record = transaction
            .query_row(
                "SELECT credential_revision_id FROM codex_authorization_attempts WHERE id=?1",
                [attempt_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|_| service_error("database_error", true))?
            .ok_or_else(|| service_error("authorization_attempt_not_found", false))?;
        let now = now_seconds();
        transaction
            .execute(
                "UPDATE codex_authorization_attempts
                 SET state=?2, completed_at=?3, error_code=?4
                 WHERE id=?1 AND state IN ('authorizing','awaiting_user','verifying')",
                params![attempt_id, attempt_state, now, error_code],
            )
            .map_err(|_| service_error("database_error", true))?;
        if let Some(revision_id) = record {
            transaction
                .execute(
                    "UPDATE official_credential_revisions
                     SET auth_state=?2, auth_checked_at=?3, auth_error_code=?4
                     WHERE id=?1 AND auth_state='authorizing'",
                    params![revision_id, revision_state, now, error_code],
                )
                .map_err(|_| service_error("database_error", true))?;
        }
        release_operation_leases_in_transaction(&transaction, attempt_id)?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))
    }

    pub(crate) fn authorization_status(
        &self,
        attempt_id: &str,
    ) -> Result<AuthorizationStatus, OfficialAccountError> {
        self.database
            .connection()
            .lock()
            .map_err(|_| service_error("database_error", true))?
            .query_row(
                "SELECT id, account_id, credential_revision_id, state, error_code
                 FROM codex_authorization_attempts WHERE id=?1",
                [attempt_id],
                |row| {
                    let state: String = row.get(3)?;
                    Ok(AuthorizationStatus {
                        attempt_id: row.get(0)?,
                        account_id: row.get(1)?,
                        credential_revision_id: row.get(2)?,
                        state: AuthorizationState::from_db(&state),
                        error_code: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|_| service_error("database_error", true))?
            .ok_or_else(|| service_error("authorization_attempt_not_found", false))
    }

    fn account_for_authorization(
        &self,
        account_id: &str,
        must_exist: bool,
    ) -> Result<AccountAuthorizationRecord, OfficialAccountError> {
        let record = self
            .database
            .connection()
            .lock()
            .map_err(|_| service_error("database_error", true))?
            .query_row(
                "SELECT current_credential_revision_id FROM codex_accounts WHERE id=?1",
                [account_id],
                |row| {
                    Ok(AccountAuthorizationRecord {
                        current_revision_id: row.get(0)?,
                    })
                },
            )
            .optional()
            .map_err(|_| service_error("database_error", true))?;
        match (record, must_exist) {
            (Some(record), _) => Ok(record),
            (None, true) => Err(service_error("official_account_not_found", false)),
            (None, false) => Ok(AccountAuthorizationRecord {
                current_revision_id: None,
            }),
        }
    }
}
