//! CSV 导出格式：UTF-8 BOM、CRLF、标准双引号转义；以公式前缀开头的文本加单引号作为纯文本，
//! 防止在表格软件中被当作公式执行。时间按本地偏移输出 ISO 8601，日期级值保持日期。

use desktrace_core::{CandidateStatus, CoverageStatus, ObjectType, format_with_offset};
use desktrace_storage::ObjectSummary;

/// 可导出的字段，顺序即导出顺序。
pub(crate) const EXPORT_FIELDS: [(&str, &str); 9] = [
    ("objectType", "对象类型"),
    ("displayName", "名称"),
    ("path", "路径或入口"),
    ("lastUsedAt", "最近观察时间"),
    ("installationDate", "系统日期"),
    ("installationMeaning", "日期含义"),
    ("observationStartedAt", "观察起点"),
    ("candidateStatus", "候选状态"),
    ("coverageStatus", "记录覆盖"),
];

/// UTF-8 BOM，让表格软件按 UTF-8 打开中文。
pub(crate) const BOM: &str = "\u{feff}";

/// 校验并按固定顺序整理用户确认的字段；含未知字段或为空时返回 `None`。
pub(crate) fn confirmed_fields(requested: &[String]) -> Option<Vec<&'static str>> {
    if requested.is_empty()
        || requested
            .iter()
            .any(|field| !EXPORT_FIELDS.iter().any(|(key, _)| key == field))
    {
        return None;
    }
    Some(
        EXPORT_FIELDS
            .iter()
            .filter(|(key, _)| requested.iter().any(|field| field == key))
            .map(|(key, _)| *key)
            .collect(),
    )
}

/// 表头行。
pub(crate) fn header(fields: &[&str]) -> String {
    let cells: Vec<String> = fields
        .iter()
        .filter_map(|field| {
            EXPORT_FIELDS
                .iter()
                .find(|(key, _)| key == field)
                .map(|(_, label)| escape(label))
        })
        .collect();
    format!("{}\r\n", cells.join(","))
}

/// 一行数据。`offset` 给出各时刻的本地偏移分钟数。
pub(crate) fn row(item: &ObjectSummary, fields: &[&str], offset: &dyn Fn(i64) -> i32) -> String {
    let cells: Vec<String> = fields
        .iter()
        .map(|field| escape(&cell(item, field, offset)))
        .collect();
    format!("{}\r\n", cells.join(","))
}

fn cell(item: &ObjectSummary, field: &str, offset: &dyn Fn(i64) -> i32) -> String {
    let time = |value: &Option<String>| {
        value
            .as_deref()
            .and_then(parse_utc)
            .map(|ms| format_with_offset(ms, offset(ms)))
            .unwrap_or_default()
    };
    match field {
        "objectType" => match item.object_type {
            ObjectType::Application => "应用".into(),
            ObjectType::Folder => "文件夹".into(),
            ObjectType::File => "文件".into(),
        },
        "displayName" => item.display_name.clone(),
        "path" => item.current_path.clone().unwrap_or_default(),
        "lastUsedAt" => time(&item.last_used_at),
        "observationStartedAt" => time(&item.observation_started_at),
        // 日期级值原样输出；时刻级值换算为本地时间。
        "installationDate" => match item.installation_date.as_deref() {
            Some(date) if date.len() == 10 => date.to_string(),
            Some(_) => time(&item.installation_date),
            None => String::new(),
        },
        "installationMeaning" => meaning_label(&item.installation_meaning).into(),
        "candidateStatus" => status_label(item.candidate_status).into(),
        "coverageStatus" if item.candidate_status.shows_coverage() => {
            coverage_label(item.coverage_status).into()
        }
        _ => String::new(),
    }
}

/// 解析本程序输出的 `YYYY-MM-DDTHH:MM:SSZ`。
fn parse_utc(text: &str) -> Option<i64> {
    let days = desktrace_core::parse_date(text.get(0..10)?)?;
    let hour: i64 = text.get(11..13)?.parse().ok()?;
    let minute: i64 = text.get(14..16)?.parse().ok()?;
    let second: i64 = text.get(17..19)?.parse().ok()?;
    Some(days * desktrace_core::DAY_MS + ((hour * 60 + minute) * 60 + second) * 1000)
}

fn escape(value: &str) -> String {
    // 以公式符号或制表、回车开头的单元格在表格软件里可能被执行，加单引号作为文本。
    let guarded = match value.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{value}"),
        _ => value.to_string(),
    };
    if guarded.contains([',', '"', '\n', '\r']) || guarded != value {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

pub(crate) fn status_label(status: CandidateStatus) -> &'static str {
    match status {
        CandidateStatus::Recent => "近期有使用记录",
        CandidateStatus::StaleCandidate => "较久未记录到使用",
        CandidateStatus::NeverObserved => "观察期内未记录到使用",
        CandidateStatus::InsufficientObservation => "观察时间不足",
        CandidateStatus::Unknown => "无法判断",
        CandidateStatus::Excluded => "已排除",
        CandidateStatus::Unavailable => "已失联或暂不可访问",
    }
}

fn coverage_label(coverage: CoverageStatus) -> &'static str {
    match coverage {
        CoverageStatus::Continuous => "记录连续",
        CoverageStatus::Partial => "存在记录中断",
        CoverageStatus::Unknown => "覆盖未知",
    }
}

fn meaning_label(meaning: &str) -> &'static str {
    match meaning {
        "install_or_service" => "安装或维护日期",
        "install_or_update" => "包安装或更新日期",
        "registered" => "系统登记日期",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_quotes_newlines_and_formula_prefixes() {
        assert_eq!(escape("普通文本"), "普通文本");
        assert_eq!(escape("a,b"), "\"a,b\"");
        assert_eq!(escape("说\"明\""), "\"说\"\"明\"\"\"");
        assert_eq!(escape("第一行\n第二行"), "\"第一行\n第二行\"");
        assert_eq!(escape("=SUM(A1)"), "\"'=SUM(A1)\"");
        assert_eq!(escape("-1"), "\"'-1\"");
        assert_eq!(escape("@cmd"), "\"'@cmd\"");
    }

    #[test]
    fn fields_are_validated_and_ordered() {
        let fields = confirmed_fields(&["candidateStatus".into(), "displayName".into()]);
        assert_eq!(fields, Some(vec!["displayName", "candidateStatus"]));
        assert_eq!(confirmed_fields(&["password".into()]), None);
        assert_eq!(confirmed_fields(&[]), None);
        assert_eq!(
            header(&["displayName", "candidateStatus"]),
            "名称,候选状态\r\n"
        );
    }

    #[test]
    fn utc_text_round_trips() {
        assert_eq!(
            parse_utc(&desktrace_core::format_utc(1_791_684_000_000)),
            Some(1_791_684_000_000)
        );
        assert_eq!(parse_utc("bad"), None);
    }
}
