//! 跨领域小工具：时间戳、路径校验和内容指纹。
//!
//! 这个目录只放字面相同的纯函数，不放业务 Store，不读写 SQLite，不持有凭据。
//! 对外用 `crate::support::unix_now_seconds` 取 UNIX 秒，不要再复制一份。
//!
//! 未合并 `diagnostics` / `usage` 的 `Local::now().timestamp()`。
//! 未合并 `balance` 的 `chrono::Utc::now().timestamp()`。
//! 它们走 chrono 时区语义，本轮不证明与 SystemTime UNIX epoch 完全等价。
//! 未合并 `channel_probe::unix_now` 和 `database` 里的内联 SystemTime。
//! 未合并 `official_accounts` 的 `now_seconds`，那两个大根文件由另一路拆分。
//! `conversation_archive` 的私有目录权限错误码不同，未并入本目录。

mod hash;
mod paths;
mod time;

pub use hash::sha256_label;
pub use paths::{resolve_workspace, set_private_directory};
pub use time::unix_now_seconds;
