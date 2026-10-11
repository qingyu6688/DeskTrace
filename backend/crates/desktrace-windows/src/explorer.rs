//! 资源管理器活动目录探测。
//!
//! Windows 11 的多个标签页共享同一个顶层窗口。活动标签页取顶层窗口下第一个
//! `ShellTabWindowClass` 子窗口，再在 `IShellWindows` 中找到 `IShellBrowser::GetWindow`
//! 等于它的那一项。这一规则需要按测试计划 TC-09 实机验证；任何一步对不上都返回
//! `Unsupported`，不使用窗口标题或后台标签页猜测路径。

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CoCreateInstance, CoTaskMemFree, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    GPFIDL_DEFAULT, IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, IWebBrowser2,
    SHGetPathFromIDListEx, SID_STopLevelBrowser, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetClassNameW};
use windows::core::{Interface, w};

use crate::WindowsError;
use crate::com::ComApartment;

const EXPLORER_CLASS: &str = "CabinetWClass";
const MAX_CLASS_CHARS: usize = 256;
const MAX_PATH_CHARS: usize = 32_768;

/// 一次探测结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplorerView {
    /// 前台窗口不是资源管理器文件窗口。
    NotExplorer,
    /// 活动标签页是文件系统目录。
    Folder(String),
    /// 活动标签页是此电脑、搜索结果等虚拟位置，不进入长期判断。
    Virtual,
    /// 无法可靠确定活动标签页。
    Unsupported,
}

/// 绑定在创建线程上的探测器，持有 STA 和外壳窗口集合；不能跨线程使用。
pub struct ExplorerProbe {
    windows: IShellWindows,
    _apartment: ComApartment,
}

impl ExplorerProbe {
    /// 在当前线程初始化 STA 并连接外壳窗口集合。
    pub fn new() -> Result<Self, WindowsError> {
        let apartment = ComApartment::single_threaded()?;
        // SAFETY：COM 已在当前线程初始化为 STA，对象只在本线程使用。
        let windows: IShellWindows =
            unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)? };
        Ok(Self {
            windows,
            _apartment: apartment,
        })
    }

    /// 探测给定前台窗口的活动目录。
    pub fn probe(&self, window: isize) -> ExplorerView {
        let top = HWND(window as *mut c_void);
        if class_name(top).as_deref() != Some(EXPLORER_CLASS) {
            return ExplorerView::NotExplorer;
        }
        // SAFETY：top 是调用方刚取得的前台窗口；查找失败返回错误而不是无效句柄。
        let active_tab =
            unsafe { FindWindowExW(Some(top), None, w!("ShellTabWindowClass"), None) }.ok();
        match self.active_browser(window, active_tab) {
            Some(browser) => folder_path(&browser),
            None => ExplorerView::Unsupported,
        }
    }

    fn active_browser(&self, window: isize, active_tab: Option<HWND>) -> Option<IShellBrowser> {
        // SAFETY：所有 COM 调用都在创建 windows 的 STA 线程上进行，接口引用由 windows-rs 管理。
        unsafe {
            let count = self.windows.Count().ok()?;
            let mut matches = Vec::new();
            for index in 0..count {
                let Ok(dispatch) = self.windows.Item(&VARIANT::from(index)) else {
                    continue;
                };
                let Ok(web) = dispatch.cast::<IWebBrowser2>() else {
                    continue;
                };
                if web.HWND().ok().map(|handle| handle.0) != Some(window) {
                    continue;
                }
                let Ok(provider) = web.cast::<IServiceProvider>() else {
                    continue;
                };
                let browser = provider
                    .QueryService::<IShellBrowser>(&SID_STopLevelBrowser)
                    .or_else(|_| provider.QueryService::<IShellBrowser>(&IShellBrowser::IID));
                let Ok(browser) = browser else { continue };
                if let Some(tab) = active_tab {
                    if browser.GetWindow().ok() == Some(tab) {
                        return Some(browser);
                    }
                } else {
                    matches.push(browser);
                }
            }
            // 没有标签页子窗口的旧版资源管理器：同一顶层窗口只有一个浏览器时才采用。
            if active_tab.is_none() && matches.len() == 1 {
                matches.pop()
            } else {
                None
            }
        }
    }
}

fn class_name(window: HWND) -> Option<String> {
    let mut buffer = [0u16; MAX_CLASS_CHARS];
    // SAFETY：缓冲区为本地数组，系统按长度截断。
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    (length > 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]))
}

fn folder_path(browser: &IShellBrowser) -> ExplorerView {
    // SAFETY：接口来自当前 STA；GetCurFolder 返回系统分配的 ID 列表，读取后释放一次。
    unsafe {
        let Ok(view) = browser.QueryActiveShellView() else {
            return ExplorerView::Unsupported;
        };
        let Ok(folder_view) = view.cast::<IFolderView>() else {
            return ExplorerView::Unsupported;
        };
        let Ok(folder) = folder_view.GetFolder::<IPersistFolder2>() else {
            return ExplorerView::Virtual;
        };
        let Ok(pidl) = folder.GetCurFolder() else {
            return ExplorerView::Virtual;
        };
        let mut buffer = vec![0u16; MAX_PATH_CHARS];
        let found = SHGetPathFromIDListEx(pidl, &mut buffer, GPFIDL_DEFAULT).as_bool();
        CoTaskMemFree(Some(pidl as *const c_void));
        if !found {
            return ExplorerView::Virtual;
        }
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        match String::from_utf16_lossy(&buffer[..end]) {
            path if path.is_empty() => ExplorerView::Virtual,
            path => ExplorerView::Folder(path),
        }
    }
}
