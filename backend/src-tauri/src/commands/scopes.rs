use desktrace_core::{CollectionState, NormalizedPath, normalize_local_path};
use desktrace_storage::{FolderScope, StorageError};
use serde::Serialize;
use tauri::{State, WebviewWindow};

use super::{CommandResult, blocking, fail, ok, respond};
use crate::services::clock::now_ms;
use crate::services::storage::StorageService;
use crate::services::tokens::FolderSelection;
use crate::state::AppState;

/// 目录选择结果：展示路径与一次性凭据。用户取消时整个结果为空。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PickedFolder {
    path: String,
    selection_token: String,
}

#[derive(Serialize)]
pub(crate) struct Removed {
    removed: bool,
}

pub(crate) fn owner_window(window: &WebviewWindow) -> isize {
    window.hwnd().map(|handle| handle.0 as isize).unwrap_or(0)
}

/// 打开系统文件夹选择对话框。路径只在后端保存，前端拿到的是 60 秒内有效的一次性凭据。
#[tauri::command]
pub(crate) async fn pick_folder_scope(
    state: State<'_, AppState>,
    window: WebviewWindow,
) -> CommandResult<Option<PickedFolder>> {
    let owner = owner_window(&window);
    let picked =
        tauri::async_runtime::spawn_blocking(move || desktrace_windows::pick_folder(owner)).await;
    let path = match picked {
        Ok(Ok(Some(path))) => path,
        Ok(Ok(None)) => return Ok(ok("已取消", None)),
        _ => return Ok(fail(500, "无法打开文件夹选择窗口")),
    };
    let normalized = match normalize_local_path(&path) {
        Ok(normalized) => normalized,
        Err(error) => return Ok(fail(422, error.to_string())),
    };
    let file_identity = desktrace_windows::folder_file_identity(&normalized.display);
    let display = normalized.display.clone();
    let token = state.tokens.issue_selection(FolderSelection {
        path: normalized,
        file_identity,
    });
    Ok(ok(
        "已选择文件夹",
        Some(PickedFolder {
            path: display,
            selection_token: token,
        }),
    ))
}

/// 首次启动时自动加入系统桌面这一标记；用户之后删除桌面范围不会再被加回。
const DESKTOP_SEEDED_FLAG: &str = "desktop_scope_seeded";

/// 当前用户的系统桌面（已重定向时为实际位置）。
pub(crate) fn desktop_path() -> Option<NormalizedPath> {
    desktrace_windows::desktop_folder().and_then(|path| normalize_local_path(&path).ok())
}

/// 首次启动时把系统桌面加入观察范围，只做一次。桌面已经在范围中时只记下标记。
pub(crate) fn seed_desktop_scope(storage: &StorageService, now: i64) {
    let Some(desktop) = desktop_path() else {
        return;
    };
    let file_identity = desktrace_windows::folder_file_identity(&desktop.display);
    let _ = storage.call(move |storage| {
        if storage.flag(DESKTOP_SEEDED_FLAG)? {
            return Ok(());
        }
        let running = storage.collection_state()? == CollectionState::Running;
        match storage.add_folder_scope(&desktop, file_identity, false, running, now) {
            Ok(_) | Err(StorageError::ScopeExists) => {}
            Err(error) => return Err(error),
        }
        storage.set_flag(DESKTOP_SEEDED_FLAG, now)
    });
}

fn mark_desktop(mut scopes: Vec<FolderScope>) -> Vec<FolderScope> {
    let desktop = desktop_path().map(|path| path.key);
    for scope in &mut scopes {
        scope.is_system_desktop = desktop.is_some()
            && normalize_local_path(&scope.path).ok().map(|path| path.key) == desktop;
    }
    scopes
}

/// 范围变化成功后通知采集线程立即重新扫描直接子项。
fn after_change<T>(state: &AppState, response: super::ApiResponse<T>) -> CommandResult<T> {
    if response.is_success() {
        state.collector.rescan_scopes();
        state.notifier.objects_changed("folder");
        state.notifier.objects_changed("file");
    }
    Ok(response)
}

/// 用选择凭据新增观察范围。凭据过期或已使用返回 409。
#[tauri::command]
pub(crate) async fn add_folder_scope(
    state: State<'_, AppState>,
    selection_token: String,
    include_descendants: bool,
) -> CommandResult<FolderScope> {
    let Some(selection) = state.tokens.take_selection(&selection_token) else {
        return Ok(fail(409, "选择已过期，请重新选择文件夹"));
    };
    let storage = state.storage.clone();
    let response = blocking(move || {
        let result = storage.call(move |storage| {
            let running = storage.collection_state()? == CollectionState::Running;
            storage.add_folder_scope(
                &selection.path,
                selection.file_identity,
                include_descendants,
                running,
                now_ms(),
            )
        });
        respond(
            result.map(|scope| mark_desktop(vec![scope]).remove(0)),
            "已添加观察范围",
        )
    })
    .await?;
    after_change(&state, response)
}

/// 把系统桌面加入观察范围，不需要选择对话框。已经在范围中时返回 409。
#[tauri::command]
pub(crate) async fn add_desktop_scope(state: State<'_, AppState>) -> CommandResult<FolderScope> {
    let Some(desktop) = desktop_path() else {
        return Ok(fail(503, "无法读取系统桌面的位置"));
    };
    let storage = state.storage.clone();
    let response = blocking(move || {
        let file_identity = desktrace_windows::folder_file_identity(&desktop.display);
        let result = storage.call(move |storage| {
            let running = storage.collection_state()? == CollectionState::Running;
            storage.add_folder_scope(&desktop, file_identity, false, running, now_ms())
        });
        respond(
            result.map(|scope| mark_desktop(vec![scope]).remove(0)),
            "已添加系统桌面",
        )
    })
    .await?;
    after_change(&state, response)
}

#[tauri::command]
pub(crate) async fn list_folder_scopes(
    state: State<'_, AppState>,
) -> CommandResult<Vec<FolderScope>> {
    let storage = state.storage.clone();
    blocking(move || {
        respond(
            storage
                .call(|storage| storage.folder_scopes())
                .map(mark_desktop),
            "查询成功",
        )
    })
    .await
}

/// 修改范围的启用状态与是否包含子目录，并重新计算文件和目录对象能否被观察。
#[tauri::command]
pub(crate) async fn update_folder_scope(
    state: State<'_, AppState>,
    scope_id: String,
    enabled: bool,
    include_descendants: bool,
) -> CommandResult<FolderScope> {
    let storage = state.storage.clone();
    let response = blocking(move || {
        let result = storage.call(move |storage| {
            let running = storage.collection_state()? == CollectionState::Running;
            storage.update_folder_scope(&scope_id, enabled, include_descendants, running, now_ms())
        });
        respond(
            result.map(|scope| mark_desktop(vec![scope]).remove(0)),
            "已更新观察范围",
        )
    })
    .await?;
    after_change(&state, response)
}

/// 删除范围：停止相关观察，保留既有历史。
#[tauri::command]
pub(crate) async fn remove_folder_scope(
    state: State<'_, AppState>,
    scope_id: String,
) -> CommandResult<Removed> {
    let storage = state.storage.clone();
    let response = blocking(move || {
        let result = storage.call(move |storage| {
            let running = storage.collection_state()? == CollectionState::Running;
            storage.remove_folder_scope(&scope_id, running, now_ms())
        });
        respond(result.map(|()| Removed { removed: true }), "已删除观察范围")
    })
    .await?;
    after_change(&state, response)
}
