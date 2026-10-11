//! 系统文件对话框与外壳操作。路径只通过系统 API 传递，不拼接命令行。

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::{COMDLG_FILTERSPEC, ITEMIDLIST};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
    FileSaveDialog, IFileOpenDialog, IFileSaveDialog, IShellItem, SHOpenFolderAndSelectItems,
    SHParseDisplayName, SIGDN_FILESYSPATH, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HRESULT, HSTRING, w};

use crate::WindowsError;
use crate::com::run_in_sta;

/// `ShellExecuteW` 返回值大于 32 表示成功。
const SHELL_EXECUTE_SUCCESS_MIN: isize = 32;

fn owner_window(owner: isize) -> Option<HWND> {
    (owner != 0).then_some(HWND(owner as *mut c_void))
}

fn is_cancelled(error: &windows::core::Error) -> bool {
    error.code() == HRESULT::from_win32(ERROR_CANCELLED.0)
}

/// 读取外壳项的文件系统路径并释放系统分配的字符串。
fn item_path(item: &IShellItem) -> Result<String, WindowsError> {
    // SAFETY：GetDisplayName 返回由 CoTaskMemAlloc 分配的字符串，转成 String 后立即释放一次。
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let text = raw.to_string().map_err(|_| WindowsError::InvalidPath);
        CoTaskMemFree(Some(raw.0 as *const c_void));
        text
    }
}

/// 打开系统文件夹选择对话框。用户取消时返回 `None`。`owner` 为父窗口句柄，0 表示无父窗口。
pub fn pick_folder(owner: isize) -> Result<Option<String>, WindowsError> {
    run_in_sta(move || {
        // SAFETY：对话框对象在本 STA 线程创建并使用，Show 期间父窗口句柄由调用方保证有效。
        unsafe {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let options = dialog.GetOptions()?;
            dialog
                .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)?;
            dialog.SetTitle(w!("选择要观察的文件夹"))?;
            if let Err(error) = dialog.Show(owner_window(owner)) {
                return if is_cancelled(&error) {
                    Ok(None)
                } else {
                    Err(error.into())
                };
            }
            Ok(Some(item_path(&dialog.GetResult()?)?))
        }
    })
}

/// 打开 CSV 保存对话框，返回用户确认的目标路径。用户取消时返回 `None`。
pub fn save_csv_dialog(owner: isize, default_name: &str) -> Result<Option<String>, WindowsError> {
    let default_name = HSTRING::from(default_name);
    run_in_sta(move || {
        // SAFETY：同 pick_folder；过滤器字符串为静态常量，生命周期覆盖调用。
        unsafe {
            let dialog: IFileSaveDialog =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            let options = dialog.GetOptions()?;
            dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT)?;
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: w!("CSV 文件"),
                pszSpec: w!("*.csv"),
            }])?;
            dialog.SetDefaultExtension(w!("csv"))?;
            dialog.SetFileName(&default_name)?;
            dialog.SetTitle(w!("导出当前结果"))?;
            if let Err(error) = dialog.Show(owner_window(owner)) {
                return if is_cancelled(&error) {
                    Ok(None)
                } else {
                    Err(error.into())
                };
            }
            Ok(Some(item_path(&dialog.GetResult()?)?))
        }
    })
}

/// 在资源管理器中打开目录。
pub fn open_folder(path: &str) -> Result<(), WindowsError> {
    if !Path::new(path).is_dir() {
        return Err(WindowsError::InvalidPath);
    }
    shell_open(&HSTRING::from(path))
}

/// 在资源管理器中定位并选中文件。
pub fn reveal_file(path: &str) -> Result<(), WindowsError> {
    if !Path::new(path).is_file() {
        return Err(WindowsError::InvalidPath);
    }
    let wide = HSTRING::from(path);
    run_in_sta(move || {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY：SHParseDisplayName 成功后 pidl 指向系统分配的 ID 列表，使用后用 CoTaskMemFree 释放一次。
        unsafe {
            SHParseDisplayName(&wide, None, &mut pidl, 0, None)?;
            let result = SHOpenFolderAndSelectItems(pidl, None, 0);
            CoTaskMemFree(Some(pidl as *const c_void));
            result?;
        }
        Ok(())
    })
}

/// 进入“设置 > 个性化 > 开始”，由用户自行决定是否开启“显示最近打开的项目”。程序不修改该设置。
pub fn open_start_settings() -> Result<(), WindowsError> {
    shell_open(&HSTRING::from("ms-settings:personalization-start"))
}

/// 进入 Windows“已安装的应用”设置页，由用户自行决定是否卸载。
pub fn open_application_settings() -> Result<(), WindowsError> {
    shell_open(&HSTRING::from("ms-settings:appsfeatures"))
}

fn shell_open(target: &HSTRING) -> Result<(), WindowsError> {
    // SAFETY：所有字符串参数在调用期间有效；不传参数和工作目录，目标不会被当作命令行解析。
    let result = unsafe { ShellExecuteW(None, w!("open"), target, None, None, SW_SHOWNORMAL) };
    if result.0 as isize > SHELL_EXECUTE_SUCCESS_MIN {
        Ok(())
    } else {
        Err(WindowsError::Api {
            code: result.0 as isize as i32,
        })
    }
}
