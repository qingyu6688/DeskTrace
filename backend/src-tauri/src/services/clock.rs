//! 墙上时间与本地偏移。事件时间统一用 UTC 毫秒，偏移只用于展示和按本地日期汇总。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 UTC 毫秒。系统时间早于 1970 年时返回 0，后续写入会被时间校验拒绝。
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// 某个时刻的本地偏移分钟数。
pub(crate) fn offset_minutes(utc_ms: i64) -> i32 {
    desktrace_windows::local_offset_minutes(utc_ms)
}
