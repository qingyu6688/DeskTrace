//! 应用全局状态，由 Tauri 托管，命令通过 `State<AppState>` 访问。

use std::sync::Arc;

use desktrace_core::CollectionState;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};

use crate::collector::Collector;
use crate::services::clock::now_ms;
use crate::services::jobs::JobRegistry;
use crate::services::notifier::Notifier;
use crate::services::storage::{ServiceError, StorageService};
use crate::services::tokens::TokenStore;

pub(crate) struct AppState {
    pub(crate) storage: StorageService,
    pub(crate) jobs: Arc<JobRegistry>,
    pub(crate) tokens: TokenStore,
    pub(crate) collector: Collector,
    pub(crate) notifier: Arc<Notifier>,
}

/// 托盘里“暂停/恢复记录”菜单项，状态变化时同步文字。
pub(crate) struct TrayMenu {
    pub(crate) toggle: MenuItem<Wry>,
}

/// 保存并应用新的采集意图。首次启用和恢复时为尚无起点的对象建立观察起点。
pub(crate) fn apply_collection_state(
    app: &AppHandle,
    next: CollectionState,
) -> Result<CollectionState, ServiceError> {
    let state = app.state::<AppState>();
    let now = now_ms();
    let applied = state.storage.call(move |storage| {
        let current = storage.collection_state()?;
        // 已启用过的记录不能回到“尚未启用”；重复请求保持幂等。
        let target = if next == CollectionState::NotStarted {
            current
        } else {
            next
        };
        if target != current {
            storage.save_collection_state(target, now)?;
        }
        if target == CollectionState::Running {
            storage.start_pending_observations(now)?;
        }
        Ok(target)
    })?;
    state.collector.set_state(applied);
    sync_tray(app, applied);
    state.notifier.objects_changed("application");
    state.notifier.objects_changed("folder");
    Ok(applied)
}

/// 同步托盘菜单文字。
pub(crate) fn sync_tray(app: &AppHandle, collection: CollectionState) {
    if let Some(tray) = app.try_state::<TrayMenu>() {
        let text = match collection {
            CollectionState::Running => "暂停记录",
            CollectionState::Paused => "恢复记录",
            CollectionState::NotStarted => "开始记录…",
        };
        let _ = tray.toggle.set_text(text);
    }
}
