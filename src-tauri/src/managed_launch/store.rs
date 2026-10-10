//! ManagedLaunchStore 构造。
//!
//! 只负责从 Database 装配 Store 并种下初始工作区。不写渠道、不准备启动。
//! `from_database` 给 App 装配用；测试走 `for_test`。

#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;

use tauri::{App, Manager};

use crate::database::Database;
use crate::route_store::RouteStore;

use super::models::ManagedLaunchStore;
use super::validation::set_private_directory;

impl ManagedLaunchStore {
    pub fn from_database(app: &App, database: Database) -> Result<Self, String> {
        let instances_root = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法定位应用数据目录：{error}"))?
            .join("codex-instances");
        std::fs::create_dir_all(&instances_root)
            .map_err(|error| format!("无法创建 Codex 实例目录：{error}"))?;
        set_private_directory(&instances_root)?;
        let store = Self {
            connection: database.connection(),
            route_store: RouteStore::from_database(database),
            instances_root,
        };
        store.ensure_initial_workspace()?;
        Ok(store)
    }

    #[cfg(test)]
    pub(super) fn for_test<T: Send + Sync + 'static>(
        database: Database,
        instances_root: PathBuf,
        _vault: Arc<T>,
    ) -> Self {
        std::fs::create_dir_all(&instances_root).unwrap();
        Self {
            connection: database.connection(),
            route_store: RouteStore::from_database(database),
            instances_root,
        }
    }
}
