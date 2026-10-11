use desktrace_core::CollectionState;
use desktrace_storage::{ExclusionMode, ExclusionRule};
use serde::Serialize;
use tauri::State;

use super::{CommandResult, blocking, fail, respond};
use crate::services::clock::now_ms;
use crate::state::AppState;

#[derive(Serialize)]
pub(crate) struct Removed {
    removed: bool,
}

/// 新增排除。`mode` 为 `object` 或 `folder_tree`，子树模式只用于目录。
#[tauri::command]
pub(crate) async fn add_exclusion(
    state: State<'_, AppState>,
    object_id: String,
    mode: String,
) -> CommandResult<ExclusionRule> {
    let mode = match mode.as_str() {
        "object" => ExclusionMode::Object,
        "folder_tree" => ExclusionMode::FolderTree,
        _ => return Ok(fail(400, "排除方式不正确")),
    };
    let storage = state.storage.clone();
    let notifier = state.notifier.clone();
    blocking(move || {
        let result = storage.call(move |storage| storage.add_exclusion(&object_id, mode, now_ms()));
        if let Ok(rule) = &result {
            notifier.objects_changed(rule.object_type.as_str());
        }
        respond(result, "已排除")
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_exclusions(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ExclusionRule>> {
    let storage = state.storage.clone();
    blocking(move || respond(storage.call(|storage| storage.exclusions()), "查询成功")).await
}

/// 删除排除规则，恢复的对象从当前时刻重新开始观察。
#[tauri::command]
pub(crate) async fn remove_exclusion(
    state: State<'_, AppState>,
    rule_id: String,
) -> CommandResult<Removed> {
    let storage = state.storage.clone();
    let notifier = state.notifier.clone();
    blocking(move || {
        let result = storage.call(move |storage| {
            let running = storage.collection_state()? == CollectionState::Running;
            storage.remove_exclusion(&rule_id, running, now_ms())
        });
        if result.is_ok() {
            notifier.objects_changed("application");
            notifier.objects_changed("folder");
        }
        respond(result.map(|_| Removed { removed: true }), "已恢复观察")
    })
    .await
}
