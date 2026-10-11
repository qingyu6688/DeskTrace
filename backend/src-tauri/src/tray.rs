//! 托盘：打开窗口、暂停/恢复记录、明确退出。关闭主窗口只隐藏，记录在托盘继续。

use desktrace_core::CollectionState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager};

use crate::state::{AppState, TrayMenu, apply_collection_state, sync_tray};

const MENU_OPEN: &str = "open";
const MENU_TOGGLE: &str = "toggle";
const MENU_QUIT: &str = "quit";

pub(crate) fn create(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "打开 DeskTrace", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, MENU_TOGGLE, "开始记录…", true, None::<&str>)?;
    let exit_item = MenuItem::with_id(app, MENU_QUIT, "退出（停止记录）", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &toggle, &separator, &exit_item])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("DeskTrace · 本机使用记录")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => show_main_window(app),
            MENU_TOGGLE => toggle_collection(app),
            MENU_QUIT => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(TrayMenu { toggle });
    Ok(())
}

pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 托盘切换记录状态。尚未启用记录时打开窗口，由界面完成首次引导。
fn toggle_collection(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match state.collector.status().collection_state {
        CollectionState::NotStarted => show_main_window(app),
        current => {
            let next = if current == CollectionState::Running {
                CollectionState::Paused
            } else {
                CollectionState::Running
            };
            let app = app.clone();
            std::thread::spawn(move || {
                if let Ok(applied) = apply_collection_state(&app, next) {
                    sync_tray(&app, applied);
                }
            });
        }
    }
}

/// 明确退出：先结束开放区间，再退出进程。
pub(crate) fn quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.collector.shutdown();
    }
    app.exit(0);
}
