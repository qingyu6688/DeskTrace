use desktrace_core::{Settings, SettingsPatch};
use serde::Deserialize;
use tauri::State;

use super::{CommandResult, blocking, fail, parse, respond, service_failure};
use crate::services::clock::now_ms;
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsPatchInput {
    threshold_days: Option<u32>,
    event_retention_days: Option<u32>,
    auto_start_enabled: Option<bool>,
}

/// 开机启动命令：带引号的程序路径加 `--autostart`，启动后只在托盘运行。
fn autostart_command() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|path| format!("\"{}\" --autostart", path.display()))
}

#[tauri::command]
pub(crate) async fn get_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    let storage = state.storage.clone();
    blocking(move || respond(storage.call(|storage| storage.settings()), "查询成功")).await
}

/// 校验白名单设置补丁后整体保存；开机启动写入失败时不保存任何字段。
#[tauri::command]
pub(crate) async fn update_settings(
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> CommandResult<Settings> {
    let input: SettingsPatchInput = match parse(patch) {
        Ok(input) => input,
        Err(message) => return Ok(fail(400, message)),
    };
    let patch = SettingsPatch {
        threshold_days: input.threshold_days,
        event_retention_days: input.event_retention_days,
        auto_start_enabled: input.auto_start_enabled,
    };
    let storage = state.storage.clone();
    let notifier = state.notifier.clone();
    blocking(move || {
        let current = match storage.call(|storage| storage.settings()) {
            Ok(current) => current,
            Err(error) => return service_failure(error),
        };
        let next = match current.apply(patch) {
            Ok(next) => next,
            Err(error) => return fail(400, error.to_string()),
        };
        if next.auto_start_enabled != current.auto_start_enabled {
            let applied = autostart_command().ok_or(()).and_then(|command| {
                desktrace_windows::set_autostart(next.auto_start_enabled, &command).map_err(|_| ())
            });
            if applied.is_err() {
                return fail(500, "无法更改开机启动，设置未保存");
            }
        }
        let result = storage.call(move |storage| {
            storage.save_settings(&next, now_ms())?;
            Ok(next)
        });
        if result.is_ok() && next.threshold_days != current.threshold_days {
            notifier.objects_changed("application");
            notifier.objects_changed("folder");
        }
        respond(result, "设置已保存")
    })
    .await
}
