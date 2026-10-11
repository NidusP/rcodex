//! 本机 SQLite：连接、schema 迁移和过期清理。
//!
//! 不读写业务行，不保存 API Key 原文，不打开用户默认 ~/.codex。
//! 由各 Store 装配和 Gateway sidecar 打开同一份库。

#[path = "../resource_guard.rs"]
pub(crate) mod resource_guard;

mod connection;
mod migrate_legacy;
mod migrate_v7_plus;
// 文件仍是 migrate.rs。模块不能叫 migrate，否则会挡住测试直接调用的 migrate()。
#[path = "migrate.rs"]
mod schema;

#[cfg(test)]
mod tests;

pub use connection::Database;

#[cfg(test)]
#[allow(unused_imports)]
use connection::{configure_connection, prune_expired, DATABASE_FILENAME};
#[cfg(test)]
#[allow(unused_imports)]
use migrate_v7_plus::{migrate_to_v20, migrate_to_v23};
#[cfg(test)]
#[allow(unused_imports)]
use rusqlite::{Connection, Transaction, TransactionBehavior};
#[cfg(test)]
#[allow(unused_imports)]
use schema::{migrate, CURRENT_SCHEMA_VERSION};
