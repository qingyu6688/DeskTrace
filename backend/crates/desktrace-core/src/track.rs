//! 观察轨迹的日期汇总。存储层按 15 分钟 UTC 桶取出记录，这里换算为本地日期。

use crate::time::{format_date, local_day};

/// 存储层聚合记录的时间桶。所有现行时区偏移都是 15 分钟的整数倍，
/// 因此同一个桶不会跨越本地日期边界。
pub const TRACK_BUCKET_MS: i64 = 15 * 60_000;

/// 把桶起点换算为本地日期 `YYYY-MM-DD`，升序去重。
/// `offset_minutes` 给出每个时刻的本地偏移，夏令时由调用方按时刻计算。
pub fn local_usage_days(
    bucket_starts_ms: &[i64],
    offset_minutes: impl Fn(i64) -> i32,
) -> Vec<String> {
    let mut days: Vec<i64> = bucket_starts_ms
        .iter()
        .map(|start| local_day(*start, offset_minutes(*start)))
        .collect();
    days.sort_unstable();
    days.dedup();
    days.into_iter().map(format_date).collect()
}
