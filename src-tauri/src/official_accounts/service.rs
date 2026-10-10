//! OfficialAccountService 装配：从 Database 构造并恢复中断授权。
//!
//! 不发起 login、不写账户投影。from_database 给 App 装配用，测试走 new。

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use rusqlite::{params, Transaction, TransactionBehavior};
use tauri::{App, Manager};

use crate::database::{resource_guard::ResourceGuard, Database};

use super::driver::{AuthDriver, PendingAuthorization, ProcessAuthDriver};
use super::models::{
    managed_home_matches, now_seconds, service_error, OfficialAccountError, OP_AUTHORIZE,
    OP_DELETE, OP_DISCONNECT, OP_REFRESH,
};

#[derive(Clone)]
pub struct OfficialAccountService {
    pub(super) database: Database,
    pub(super) guard: ResourceGuard,
    pub(super) managed_homes_root: PathBuf,
    pub(super) pending: Arc<Mutex<HashMap<String, PendingAuthorization>>>,
    pub(super) auth: Arc<dyn AuthDriver>,
}

impl OfficialAccountService {
    pub fn from_database(app: &App, database: Database) -> Result<Self, OfficialAccountError> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|_| service_error("app_data_unavailable", false))?;
        let service = Self::new(
            database,
            app_data.join("managed-official-codex-homes"),
            Arc::new(ProcessAuthDriver),
        );
        service.recover_interrupted_authorizations()?;
        Ok(service)
    }

    pub(crate) fn new(
        database: Database,
        managed_homes_root: PathBuf,
        auth: Arc<dyn AuthDriver>,
    ) -> Self {
        Self {
            guard: ResourceGuard::new(database.clone()),
            database,
            managed_homes_root,
            pending: Arc::new(Mutex::new(HashMap::new())),
            auth,
        }
    }

    pub(crate) fn recover_interrupted_authorizations(&self) -> Result<(), OfficialAccountError> {
        let connection = self.database.connection();
        let connection = connection
            .lock()
            .map_err(|_| service_error("database_error", true))?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| service_error("database_error", true))?;
        let now = now_seconds();
        transaction
            .execute(
                "UPDATE codex_authorization_attempts
                 SET state='interrupted', completed_at=?1,
                     error_code='authorization_interrupted'
                 WHERE state IN ('authorizing','awaiting_user','verifying')",
                [now],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='failed', auth_checked_at=?1,
                     auth_error_code='authorization_interrupted'
                 WHERE auth_state='authorizing'",
                [now],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "UPDATE official_credential_revisions
                 SET auth_state='needs_reauth', auth_checked_at=?1,
                     auth_error_code='disconnect_interrupted'
                 WHERE id IN (
                    SELECT resource_id FROM resource_operation_leases
                    WHERE operation_kind=?2
                      AND resource_kind='official_credential_revision'
                 ) AND auth_state='authorized'",
                params![now, OP_DISCONNECT],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .execute(
                "DELETE FROM resource_operation_leases
                 WHERE operation_kind IN (?1, ?2, ?3, ?4) AND state='active'",
                params![OP_AUTHORIZE, OP_REFRESH, OP_DISCONNECT, OP_DELETE],
            )
            .map_err(|_| service_error("database_error", true))?;
        transaction
            .commit()
            .map_err(|_| service_error("database_error", true))?;
        drop(connection);
        self.cleanup_tombstoned_homes();
        Ok(())
    }

    fn cleanup_tombstoned_homes(&self) {
        let homes = self
            .database
            .connection()
            .lock()
            .ok()
            .and_then(|connection| {
                let mut statement = connection
                    .prepare(
                        "SELECT id, account_id, managed_codex_home
                         FROM official_credential_revisions
                         WHERE auth_error_code='official_account_deleted'",
                    )
                    .ok()?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            PathBuf::from(row.get::<_, String>(2)?),
                        ))
                    })
                    .ok()?
                    .collect::<Result<Vec<_>, _>>()
                    .ok();
                rows
            })
            .unwrap_or_default();
        for (revision_id, account_id, home) in homes {
            if !managed_home_matches(&self.managed_homes_root, &account_id, &revision_id, &home) {
                continue;
            }
            if home.is_dir() {
                let _ = fs::remove_dir_all(home);
            }
        }
    }
}
