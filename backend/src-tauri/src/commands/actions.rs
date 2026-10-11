use desktrace_core::ObjectType;
use desktrace_windows::WindowsError;
use serde::Serialize;
use tauri::State;

use super::{CommandResult, blocking, fail, ok, service_failure};
use crate::state::AppState;

#[derive(Serialize)]
pub(crate) struct Requested {
    requested: bool,
}

/// 打开目录或在资源管理器中定位已验证的应用入口。请求成功不代表使用，不刷新最近时间。
#[tauri::command]
pub(crate) async fn open_object_location(
    state: State<'_, AppState>,
    object_id: String,
) -> CommandResult<Requested> {
    let storage = state.storage.clone();
    blocking(move || {
        let record = match storage.call(move |storage| storage.object_record(&object_id)) {
            Ok(Some(record)) => record,
            Ok(None) => return fail(404, "该记录已不存在，请刷新列表"),
            Err(error) => return service_failure(error),
        };
        let Some(path) = record.current_path else {
            return fail(422, "此对象没有可定位的位置或应用入口");
        };
        let result = match record.object_type {
            ObjectType::Folder => desktrace_windows::open_folder(&path),
            // 文件和应用入口都在资源管理器中定位并选中，不直接打开。
            ObjectType::Application | ObjectType::File => desktrace_windows::reveal_file(&path),
        };
        match result {
            Ok(()) => ok("已请求打开", Requested { requested: true }),
            Err(WindowsError::InvalidPath) => fail(404, "位置不存在或无法访问"),
            Err(_) => fail(500, "系统拒绝了打开请求"),
        }
    })
    .await
}

/// 进入 Windows 应用设置，由用户自行决定是否卸载。
#[tauri::command]
pub(crate) async fn open_application_settings() -> CommandResult<Requested> {
    blocking(|| match desktrace_windows::open_application_settings() {
        Ok(()) => ok("已请求打开 Windows 应用设置", Requested { requested: true }),
        Err(_) => fail(500, "无法打开 Windows 应用设置"),
    })
    .await
}

/// 进入“设置 > 个性化 > 开始”，由用户自己决定是否开启“显示最近打开的项目”。程序不改动该设置。
#[tauri::command]
pub(crate) async fn open_recent_items_settings() -> CommandResult<Requested> {
    blocking(|| match desktrace_windows::open_start_settings() {
        Ok(()) => ok("已请求打开 Windows 开始设置", Requested { requested: true }),
        Err(_) => fail(500, "无法打开 Windows 设置"),
    })
    .await
}
