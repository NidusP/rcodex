//! UNIX 纪元秒。
//!
//! 只包装 SystemTime，不引入 chrono，不处理时区日历。
//! 由各 Store 写入 created_at / updated_at 时调用。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 UNIX 纪元秒。时钟回拨时按 0 处理，避免 panic。
pub fn unix_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
