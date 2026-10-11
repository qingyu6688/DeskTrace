//! 非 Windows 平台的占位实现：接口形状与 Windows 版一致，所有系统操作都返回“不支持”。
//! 首期只发布 Windows 版本，这里只用于在其他平台上做编译检查。

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;

use crate::WindowsError;

/// 系统信号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemSignal {
    /// 前台窗口切换。
    Foreground,
    /// 会话锁定。
    SessionLocked,
    /// 会话解锁。
    SessionUnlocked,
    /// 系统休眠。
    Suspended,
    /// 系统恢复。
    Resumed,
}

/// 前台应用身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForegroundIdentity {
    /// 打包应用。
    Package {
        /// 应用用户模型 ID。
        aumid: String,
    },
    /// 可执行文件。
    Executable {
        /// 路径。
        path: String,
    },
}

/// 前台应用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForegroundApp {
    /// 身份。
    pub identity: ForegroundIdentity,
    /// 窗口句柄。
    pub window: isize,
    /// 进程 ID。
    pub process_id: u32,
    /// 进程创建时间。
    pub process_created: u64,
    /// 是否为资源管理器。
    pub is_explorer: bool,
}

/// 前台监听器占位。
pub struct ForegroundWatcher;

impl ForegroundWatcher {
    /// 不支持。
    pub fn start(_sender: SyncSender<SystemSignal>) -> Result<Self, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    /// 没有丢弃的信号。
    pub fn take_dropped() -> u64 {
        0
    }
}

/// 资源管理器探测结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplorerView {
    /// 不是资源管理器。
    NotExplorer,
    /// 文件系统目录。
    Folder(String),
    /// 虚拟位置。
    Virtual,
    /// 无法识别。
    Unsupported,
}

/// 资源管理器探测器占位。
pub struct ExplorerProbe;

impl ExplorerProbe {
    /// 不支持。
    pub fn new() -> Result<Self, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    /// 不支持。
    pub fn probe(&self, _window: isize) -> ExplorerView {
        ExplorerView::Unsupported
    }
}

/// Win32 卸载项。
#[derive(Clone, Debug)]
pub struct Win32App {
    /// 清单身份。
    pub uninstall_key: String,
    /// 名称。
    pub display_name: String,
    /// 发布者。
    pub publisher: Option<String>,
    /// 版本。
    pub version: Option<String>,
    /// 安装日期原始值。
    pub install_date_raw: Option<String>,
    /// 是否 MSI。
    pub is_msi: bool,
    /// 主程序。
    pub entry_executable: Option<String>,
}

/// 打包应用入口。
#[derive(Clone, Debug)]
pub struct PackageApp {
    /// 应用用户模型 ID。
    pub aumid: String,
    /// 名称。
    pub display_name: String,
    /// 发布者。
    pub publisher: Option<String>,
    /// 版本。
    pub version: Option<String>,
    /// 安装时间。
    pub installed_at_ms: Option<i64>,
}

/// 清单读取结果。
#[derive(Clone, Debug, Default)]
pub struct CatalogScan {
    /// Win32 卸载项。
    pub win32: Vec<Win32App>,
    /// 打包应用。
    pub packages: Vec<PackageApp>,
    /// 是否完整。
    pub complete: bool,
    /// 是否取消。
    pub cancelled: bool,
}

/// 实例角色。
pub enum InstanceRole {
    /// 主实例。
    Primary(SingleInstance),
    /// 次实例。
    Secondary,
}

/// 单实例占位：非 Windows 平台总是主实例。
pub struct SingleInstance;

impl SingleInstance {
    /// 总是成为主实例。
    pub fn acquire() -> Result<InstanceRole, WindowsError> {
        Ok(InstanceRole::Primary(Self))
    }

    /// 无需等待唤起。
    pub fn watch_activation(
        &self,
        _on_activate: impl Fn() + Send + 'static,
    ) -> Result<(), WindowsError> {
        Ok(())
    }
}

/// 读不到清单，标记为不完整，避免误判卸载。
pub fn read_catalog(_cancel: &AtomicBool) -> CatalogScan {
    CatalogScan::default()
}

/// 按 UTC 处理。
pub fn local_offset_minutes(_utc_ms: i64) -> i32 {
    0
}

/// 不支持文件 ID。
pub fn folder_file_identity(_path: &str) -> Option<String> {
    None
}

/// 不支持。
pub fn resolve_foreground() -> Option<ForegroundApp> {
    None
}

/// 不支持。
pub fn idle_milliseconds() -> Option<u32> {
    None
}

/// 不支持。
pub fn pick_folder(_owner: isize) -> Result<Option<String>, WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 不支持。
pub fn save_csv_dialog(_owner: isize, _default_name: &str) -> Result<Option<String>, WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 不支持。
pub fn open_folder(_path: &str) -> Result<(), WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 不支持。
pub fn reveal_file(_path: &str) -> Result<(), WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 不支持。
pub fn open_application_settings() -> Result<(), WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 不支持。
pub fn set_autostart(_enabled: bool, _command: &str) -> Result<(), WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 不支持。
pub fn autostart_enabled() -> Result<Option<String>, WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 目录下的一个直接子项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryItem {
    /// 完整路径。
    pub path: String,
    /// 是否为目录。
    pub is_directory: bool,
}

/// 没有系统桌面。
pub fn desktop_folder() -> Option<String> {
    None
}

/// 不支持。
pub fn list_directory(_path: &str) -> Result<Vec<DirectoryItem>, WindowsError> {
    Err(WindowsError::Unsupported)
}

/// 一次被登记的打开。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentOpen {
    /// 目标路径。
    pub target: String,
}

/// 最近项目监听占位。
pub struct RecentWatcher;

impl RecentWatcher {
    /// 不支持。
    pub fn start(_sender: SyncSender<RecentOpen>) -> Result<Self, WindowsError> {
        Err(WindowsError::Unsupported)
    }

    /// 没有丢弃的记录。
    pub fn take_dropped() -> u64 {
        0
    }
}

/// 没有该来源。
pub fn recent_tracking_enabled() -> bool {
    false
}

/// 不支持。
pub fn open_start_settings() -> Result<(), WindowsError> {
    Err(WindowsError::Unsupported)
}
