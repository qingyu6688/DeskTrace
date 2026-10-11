//! UTC 毫秒与公历日期互转。只做整数运算，不依赖系统时区；本地偏移由调用方传入。

/// 一天的毫秒数，阈值按 86400 秒计算，不随夏令时伸缩。
pub const DAY_MS: i64 = 86_400_000;

const MINUTE_MS: i64 = 60_000;

/// 把公历日期换算为距 1970-01-01 的天数（proleptic Gregorian）。
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// 把距 1970-01-01 的天数换算为公历年月日。
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // 年份来自 i64 毫秒的合理范围，转换失败时回落到 1970 只会出现在非法输入上。
    (
        i32::try_from(year).unwrap_or(1970),
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// 判断年月日是否是真实存在的公历日期。
pub fn is_valid_date(year: i32, month: u32, day: u32) -> bool {
    if !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max_day = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= max_day
}

/// 把天数格式化为 `YYYY-MM-DD`。
pub fn format_date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// 把 UTC 毫秒格式化为秒级 RFC 3339 字符串，例如 `2026-06-18T13:40:00Z`。
pub fn format_utc(ms: i64) -> String {
    let days = ms.div_euclid(DAY_MS);
    let second_of_day = ms.rem_euclid(DAY_MS) / 1000;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        format_date(days),
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60
    )
}

/// 把 UTC 毫秒按给定偏移格式化为带偏移的 ISO 8601，用于 CSV 导出。
pub fn format_with_offset(ms: i64, offset_minutes: i32) -> String {
    let local = ms + i64::from(offset_minutes) * MINUTE_MS;
    let days = local.div_euclid(DAY_MS);
    let second_of_day = local.rem_euclid(DAY_MS) / 1000;
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let offset = offset_minutes.unsigned_abs();
    format!(
        "{}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        format_date(days),
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60,
        offset / 60,
        offset % 60
    )
}

/// 返回某个 UTC 时刻在给定偏移下的本地日期天数。
pub fn local_day(ms: i64, offset_minutes: i32) -> i64 {
    (ms + i64::from(offset_minutes) * MINUTE_MS).div_euclid(DAY_MS)
}

/// 解析 `YYYY-MM-DD`，非法日期返回 `None`。
pub fn parse_date(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digits = bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !digits {
        return None;
    }
    let year = text.get(0..4)?.parse().ok()?;
    let month = text.get(5..7)?.parse().ok()?;
    let day = text.get(8..10)?.parse().ok()?;
    is_valid_date(year, month, day).then(|| days_from_civil(year, month, day))
}
