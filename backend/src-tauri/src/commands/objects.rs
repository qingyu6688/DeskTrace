use desktrace_core::{CandidateStatus, CoverageStatus, ObjectType, validate_threshold};
use desktrace_storage::{
    EventView, ObjectDetail, ObjectQuery, ObjectSummary, Overview, PageResult, SortField,
};
use serde::Deserialize;
use tauri::State;

use super::{CommandResult, blocking, fail, parse, respond};
use crate::services::clock::{now_ms, offset_minutes};
use crate::state::AppState;

/// 每页条数上限。
pub(crate) const MAX_PAGE_SIZE: u32 = 200;
/// 搜索词最长字符数。
const MAX_KEYWORD_CHARS: usize = 200;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectQueryInput {
    object_type: String,
    keyword: Option<String>,
    candidate_statuses: Option<Vec<String>>,
    coverage_status: Option<String>,
    threshold_days: u32,
    sort_by: String,
    sort_direction: String,
    page: u32,
    page_size: u32,
}

/// 校验列表查询；每个字段都有白名单，排序字段不会拼进 SQL。
pub(crate) fn parse_query(value: serde_json::Value) -> Result<ObjectQuery, String> {
    let input: ObjectQueryInput = parse(value)?;
    // `location` 用于桌面页：文件和文件夹一起列出。
    let object_types = match input.object_type.as_str() {
        "location" => vec![ObjectType::Folder, ObjectType::File],
        other => vec![ObjectType::parse(other).ok_or("对象类型不正确")?],
    };
    let threshold_days =
        validate_threshold(input.threshold_days).map_err(|error| error.to_string())?;
    let keyword = input
        .keyword
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| !keyword.is_empty());
    if keyword
        .as_ref()
        .is_some_and(|keyword| keyword.chars().count() > MAX_KEYWORD_CHARS)
    {
        return Err("搜索词最多 200 个字符".into());
    }
    let statuses = input
        .candidate_statuses
        .unwrap_or_default()
        .iter()
        .map(|status| CandidateStatus::parse(status).ok_or("候选状态不正确"))
        .collect::<Result<Vec<_>, _>>()?;
    let coverage = input
        .coverage_status
        .map(|value| CoverageStatus::parse(&value).ok_or("记录覆盖筛选不正确"))
        .transpose()?;
    let sort_by = match input.sort_by.as_str() {
        "displayName" => SortField::DisplayName,
        "lastUsedAt" => SortField::LastUsedAt,
        "firstSeenAt" => SortField::FirstSeenAt,
        "installationDate" => SortField::InstallationDate,
        _ => return Err("排序字段不正确".into()),
    };
    let descending = match input.sort_direction.as_str() {
        "asc" => false,
        "desc" => true,
        _ => return Err("排序方向不正确".into()),
    };
    if input.page == 0 {
        return Err("页码从 1 开始".into());
    }
    if !(1..=MAX_PAGE_SIZE).contains(&input.page_size) {
        return Err("每页条数必须为 1～200".into());
    }
    Ok(ObjectQuery {
        object_types,
        keyword,
        statuses,
        coverage,
        threshold_days,
        sort_by,
        descending,
        page: input.page,
        page_size: input.page_size,
    })
}

/// 概览：两类对象在同一阈值下的状态计数、最近缺口与观察范围。
#[tauri::command]
pub(crate) async fn get_overview(
    state: State<'_, AppState>,
    threshold_days: u32,
) -> CommandResult<Overview> {
    let threshold = match validate_threshold(threshold_days) {
        Ok(value) => value,
        Err(error) => return Ok(fail(400, error.to_string())),
    };
    let storage = state.storage.clone();
    blocking(move || {
        respond(
            storage.call(move |storage| storage.overview(threshold, now_ms())),
            "查询成功",
        )
    })
    .await
}

/// 列表：搜索、筛选、排序和分页。
#[tauri::command]
pub(crate) async fn list_objects(
    state: State<'_, AppState>,
    query: serde_json::Value,
) -> CommandResult<PageResult<ObjectSummary>> {
    let query = match parse_query(query) {
        Ok(query) => query,
        Err(message) => return Ok(fail(400, message)),
    };
    let storage = state.storage.clone();
    blocking(move || {
        respond(
            storage.call(move |storage| storage.list_objects(&query, now_ms())),
            "查询成功",
        )
    })
    .await
}

/// 详情：对象、日期证据、近期事件、轨迹。阈值缺省时用设置中的值。
#[tauri::command]
pub(crate) async fn get_object_detail(
    state: State<'_, AppState>,
    object_id: String,
    threshold_days: Option<u32>,
) -> CommandResult<ObjectDetail> {
    if let Some(Err(error)) = threshold_days.map(validate_threshold) {
        return Ok(fail(400, error.to_string()));
    }
    let storage = state.storage.clone();
    blocking(move || {
        let result = storage.call(move |storage| {
            let settings = storage.settings()?;
            let threshold = threshold_days.unwrap_or(settings.threshold_days);
            storage.object_detail(
                &object_id,
                threshold,
                settings.event_retention_days,
                now_ms(),
                &offset_minutes,
            )
        });
        respond(result, "查询成功")
    })
    .await
}

/// 分页读取事件，不一次加载全部历史。
#[tauri::command]
pub(crate) async fn list_object_events(
    state: State<'_, AppState>,
    object_id: String,
    page: u32,
    page_size: u32,
) -> CommandResult<PageResult<EventView>> {
    if page == 0 || !(1..=MAX_PAGE_SIZE).contains(&page_size) {
        return Ok(fail(400, "页码从 1 开始，每页条数必须为 1～200"));
    }
    let storage = state.storage.clone();
    blocking(move || {
        respond(
            storage.call(move |storage| storage.object_events(&object_id, page, page_size)),
            "查询成功",
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_query;

    fn base() -> serde_json::Value {
        json!({ "objectType": "application", "thresholdDays": 90, "sortBy": "lastUsedAt", "sortDirection": "asc", "page": 1, "pageSize": 50 })
    }

    #[test]
    fn query_accepts_valid_input() {
        let mut value = base();
        value["candidateStatuses"] = json!(["stale_candidate", "never_observed"]);
        value["keyword"] = json!("  Blender ");
        let query = parse_query(value).map_err(|error| error.to_string());
        assert!(query.is_ok_and(
            |query| query.statuses.len() == 2 && query.keyword.as_deref() == Some("Blender")
        ));
    }

    #[test]
    fn location_query_lists_files_and_folders() {
        let mut value = base();
        value["objectType"] = json!("location");
        let query = parse_query(value).map_err(|error| error.to_string());
        assert!(query.is_ok_and(|query| query.object_types.len() == 2));
    }

    #[test]
    fn query_rejects_unknown_fields_and_values() {
        let mut unknown = base();
        unknown["orderBy"] = json!("name; DROP TABLE");
        assert!(parse_query(unknown).is_err());
        for (field, value) in [
            ("sortBy", json!("observed_at")),
            ("sortDirection", json!("up")),
            ("thresholdDays", json!(0)),
            ("pageSize", json!(201)),
            ("page", json!(0)),
            ("objectType", json!("document")),
            ("candidateStatuses", json!(["deleted"])),
            ("keyword", json!("长".repeat(201))),
        ] {
            let mut value_set = base();
            value_set[field] = value;
            assert!(parse_query(value_set).is_err(), "{field}");
        }
    }
}
