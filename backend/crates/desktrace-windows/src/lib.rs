//! Windows 系统适配层：时区、文件身份、系统对话框与外壳操作、开机启动、单实例、
//! 应用清单和前台/资源管理器采集。只读取所需信息，不写数据库、不决定候选状态。
//!
//! FFI 所需的 `unsafe` 只出现在本 crate，每处都注明句柄与生命周期的安全前提。

#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod catalog;
#[cfg(windows)]
mod clock;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod explorer;
#[cfg(windows)]
mod files;
#[cfg(windows)]
mod foreground;
#[cfg(windows)]
mod instance;
#[cfg(windows)]
mod known;
#[cfg(windows)]
mod recent;
#[cfg(windows)]
mod shell;

#[cfg(not(windows))]
mod fallback;

use thiserror::Error;

#[cfg(windows)]
pub use autostart::{autostart_enabled, set_autostart};
#[cfg(windows)]
pub use catalog::{CatalogScan, PackageApp, Win32App, read_catalog};
#[cfg(windows)]
pub use clock::local_offset_minutes;
#[cfg(windows)]
pub use explorer::{ExplorerProbe, ExplorerView};
#[cfg(windows)]
pub use files::folder_file_identity;
#[cfg(windows)]
pub use foreground::{
    ForegroundApp, ForegroundIdentity, ForegroundWatcher, SystemSignal, idle_milliseconds,
    resolve_foreground,
};
#[cfg(windows)]
pub use instance::{InstanceRole, SingleInstance};
#[cfg(windows)]
pub use known::{DirectoryItem, desktop_folder, list_directory};
#[cfg(windows)]
pub use recent::{RecentOpen, RecentWatcher, recent_tracking_enabled};
#[cfg(windows)]
pub use shell::{
    open_application_settings, open_folder, open_start_settings, pick_folder, reveal_file,
    save_csv_dialog,
};

#[cfg(not(windows))]
pub use fallback::*;

/// 系统适配错误，文案可直接展示，不含原始路径。
#[derive(Debug, Error)]
pub enum WindowsError {
    /// 系统接口返回失败，附 HRESULT 供诊断。
    #[error("系统接口调用失败（{code:#010x}）")]
    Api {
        /// HRESULT 或 Win32 错误码。
        code: i32,
    },
    /// 当前平台不支持该操作。
    #[error("当前系统不支持该操作")]
    Unsupported,
    /// 路径不可用。
    #[error("路径不存在或无法访问")]
    InvalidPath,
    /// 后台线程意外结束。
    #[error("系统操作线程异常结束")]
    ThreadFailed,
}

#[cfg(windows)]
impl From<windows::core::Error> for WindowsError {
    fn from(error: windows::core::Error) -> Self {
        Self::Api {
            code: error.code().0,
        }
    }
}

/// 检查当前编译平台是否属于首期支持的操作系统。
pub fn platform_supported() -> bool {
    cfg!(all(target_os = "windows", target_arch = "x86_64"))
}

/// 采集适配是否已经实现。资源管理器多标签页仍需按测试计划实机验证，
/// 识别不了的视图会报告 `unsupported_view`，不会猜测路径。
pub fn collector_implemented() -> bool {
    cfg!(windows)
}
