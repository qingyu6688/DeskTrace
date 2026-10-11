use desktrace_core::{DEDUP_WINDOW_MS, IntervalReason};
use serde::Serialize;
use tauri::State;

use super::{CommandResult, blocking, respond};
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DatabaseSummary {
    schema_version: u32,
    object_count: i64,
    event_count: i64,
    data_directory: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BootstrapStatus {
    app_version: &'static str,
    platform_supported: bool,
    collector_implemented: bool,
    database: DatabaseSummary,
    dedup_window_seconds: i64,
    interval_reason_count: usize,
}

/// 初始化状态：版本、平台、数据库计数与数据目录。
#[tauri::command]
pub(crate) async fn get_bootstrap_status(
    state: State<'_, AppState>,
) -> CommandResult<BootstrapStatus> {
    let storage = state.storage.clone();
    blocking(move || {
        let directory = storage.directory().to_string_lossy().into_owned();
        let result = storage
            .call(|storage| storage.summary())
            .map(|summary| BootstrapStatus {
                app_version: env!("CARGO_PKG_VERSION"),
                platform_supported: desktrace_windows::platform_supported(),
                collector_implemented: desktrace_windows::collector_implemented(),
                database: DatabaseSummary {
                    schema_version: summary.schema_version,
                    object_count: summary.object_count,
                    event_count: summary.event_count,
                    data_directory: directory,
                },
                dedup_window_seconds: DEDUP_WINDOW_MS / 1000,
                interval_reason_count: IntervalReason::ALL.len(),
            });
        respond(result, "初始化状态读取成功")
    })
    .await
}
