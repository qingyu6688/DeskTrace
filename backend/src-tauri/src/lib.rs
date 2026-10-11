//! 桌面入口：单实例、数据库线程、采集调度、托盘与受限 IPC。

mod collector;
mod commands;
mod services;
mod state;
mod tray;

use std::sync::Arc;

use desktrace_windows::{InstanceRole, SingleInstance};
use tauri::{Manager, RunEvent, WindowEvent};

use crate::collector::Collector;
use crate::services::clock::now_ms;
use crate::services::jobs::JobRegistry;
use crate::services::notifier::Notifier;
use crate::services::storage::StorageService;
use crate::services::tokens::TokenStore;
use crate::state::{AppState, sync_tray};

/// 开机启动时附带的参数：只在托盘运行，不弹出主窗口。
const AUTOSTART_FLAG: &str = "--autostart";

/// 开发与测试专用的数据目录变量，发布构建忽略它，避免任意路径覆盖用户数据位置。
#[cfg(debug_assertions)]
const DATA_DIR_VARIABLE: &str = "DESKTRACE_DATA_DIR";

fn data_directory(local_data: std::path::PathBuf) -> std::path::PathBuf {
    #[cfg(debug_assertions)]
    if let Some(custom) = std::env::var_os(DATA_DIR_VARIABLE).map(std::path::PathBuf::from)
        && custom.is_absolute()
    {
        return custom;
    }
    local_data.join("DeskTrace")
}

/// 主实例存续期间持有的单实例句柄。
struct InstanceGuard(#[allow(dead_code)] SingleInstance);

/// 启动桌面程序。已有实例运行时只通知它显示窗口，然后退出。
pub fn run() -> Result<(), tauri::Error> {
    let instance = match SingleInstance::acquire() {
        Ok(InstanceRole::Secondary) => return Ok(()),
        Ok(InstanceRole::Primary(instance)) => Some(instance),
        // 单实例检测失败时仍允许启动，数据库线程本身保证单写入。
        Err(_) => None,
    };
    let autostart = std::env::args().any(|argument| argument == AUTOSTART_FLAG);
    let app = tauri::Builder::default()
        .setup(move |app| {
            let directory = data_directory(app.path().local_data_dir()?);
            let storage = StorageService::start(directory)?;
            let now = now_ms();
            // 上次运行遗留的开放区间先结束，未正常退出的时段标为未知，不补造历史。
            let collection = storage
                .call(move |storage| {
                    storage.recover_open_intervals(now)?;
                    storage.collection_state()
                })
                .ok();
            // 首次启动把系统桌面加入观察范围；之后用户删除了也不会再自动加回。
            commands::scopes::seed_desktop_scope(&storage, now);
            let notifier = Arc::new(Notifier::new(app.handle().clone()));
            let catalog_app = app.handle().clone();
            let collector = Collector::start(
                storage.clone(),
                notifier.clone(),
                Box::new(move || {
                    commands::collection::start_catalog_refresh(&catalog_app);
                }),
            )?;
            app.manage(AppState {
                storage,
                jobs: JobRegistry::new(notifier.clone()),
                tokens: TokenStore::default(),
                collector,
                notifier,
            });
            tray::create(app)?;
            if let Some(collection) = collection {
                sync_tray(app.handle(), collection);
            }
            commands::collection::start_catalog_refresh(app.handle());
            if let Some(instance) = instance {
                let handle = app.handle().clone();
                instance.watch_activation(move || tray::show_main_window(&handle))?;
                app.manage(InstanceGuard(instance));
            }
            if !autostart {
                tray::show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭主窗口只隐藏到托盘，记录继续；明确退出在托盘菜单中。
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap::get_bootstrap_status,
            commands::objects::get_overview,
            commands::objects::list_objects,
            commands::objects::get_object_detail,
            commands::objects::list_object_events,
            commands::collection::refresh_application_catalog,
            commands::collection::get_collector_status,
            commands::collection::set_collection_state,
            commands::scopes::pick_folder_scope,
            commands::scopes::add_folder_scope,
            commands::scopes::add_desktop_scope,
            commands::scopes::list_folder_scopes,
            commands::scopes::update_folder_scope,
            commands::scopes::remove_folder_scope,
            commands::exclusions::add_exclusion,
            commands::exclusions::list_exclusions,
            commands::exclusions::remove_exclusion,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::actions::open_object_location,
            commands::actions::open_application_settings,
            commands::actions::open_recent_items_settings,
            commands::data::request_export,
            commands::data::get_job_status,
            commands::data::cancel_job,
            commands::data::prepare_clear_usage_data,
            commands::data::clear_usage_data,
        ])
        .build(tauri::generate_context!())?;
    app.run(|handle, event| {
        if let RunEvent::Exit = event
            && let Some(state) = handle.try_state::<AppState>()
        {
            state.collector.shutdown();
        }
    });
    Ok(())
}
