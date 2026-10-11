//! 按时刻计算当前时区的本地偏移，夏令时由系统时区规则决定。

use desktrace_core::days_from_civil;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// 1601-01-01 到 1970-01-01 的毫秒数，FILETIME 以前者为纪元。
const FILETIME_EPOCH_OFFSET_MS: i64 = 11_644_473_600_000;
const TICKS_PER_MS: i64 = 10_000;

/// 返回某个 UTC 时刻在当前系统时区下的偏移分钟数；系统调用失败时按 0 处理。
pub fn local_offset_minutes(utc_ms: i64) -> i32 {
    offset(utc_ms).unwrap_or(0)
}

fn offset(utc_ms: i64) -> Option<i32> {
    let ticks = u64::try_from(
        utc_ms
            .checked_add(FILETIME_EPOCH_OFFSET_MS)?
            .checked_mul(TICKS_PER_MS)?,
    )
    .ok()?;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY：两个 SYSTEMTIME 都是本函数栈上的有效可写结构，FILETIME 只读；
    // 传入 None 表示使用当前系统时区，不涉及外部句柄。
    unsafe {
        FileTimeToSystemTime(&file_time, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    let minutes = |time: &SYSTEMTIME| {
        days_from_civil(
            i32::from(time.wYear),
            u32::from(time.wMonth),
            u32::from(time.wDay),
        ) * 1440
            + i64::from(time.wHour) * 60
            + i64::from(time.wMinute)
    };
    i32::try_from(minutes(&local) - minutes(&utc)).ok()
}
