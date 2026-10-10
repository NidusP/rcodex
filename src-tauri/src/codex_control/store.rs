//! CodexControlStore 构造。
//!
//! 只负责从 Database 装配 Store，并在启动时做一次中断实例恢复。不读快照、不创建预设。
//! from_database 给 App 装配用；测试走 for_test。

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use rusqlite::Connection;
use tauri::{App, Manager};

use crate::database::Database;

use super::helpers::set_private_directory;

#[derive(Clone)]
pub struct CodexControlStore {
    pub(super) connection: Arc<Mutex<Connection>>,
    pub(super) instances_root: PathBuf,
    pub(super) official_homes_root: PathBuf,
}

impl CodexControlStore {
    pub fn from_database(app: &App, database: Database) -> Result<Self, String> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
        let instances_root = app_data.join("codex-instances");
        std::fs::create_dir_all(&instances_root)
            .map_err(|error| format!("无法创建 Codex 实例目录：{error}"))?;
        set_private_directory(&instances_root)?;
        let store = Self {
            connection: database.connection(),
            instances_root,
            official_homes_root: app_data.join("managed-official-codex-homes"),
        };
        store.mark_interrupted_gateway_instances_degraded()?;
        store.reconcile_all()?;
        Ok(store)
    }

    #[cfg(test)]
    pub(super) fn for_test(database: Database, instances_root: PathBuf) -> Self {
        std::fs::create_dir_all(&instances_root).unwrap();
        Self {
            connection: database.connection(),
            official_homes_root: instances_root.join("managed-official-codex-homes"),
            instances_root,
        }
    }
}
