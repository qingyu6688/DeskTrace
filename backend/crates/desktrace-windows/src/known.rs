//! 系统已知文件夹与目录直接子项。桌面路径按当前用户的已知文件夹读取，
//! 已被 OneDrive 或组策略重定向时也能取到实际位置。

use std::ffi::c_void;
use std::os::windows::fs::MetadataExt;

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Recent, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::GUID;

use crate::WindowsError;

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// 目录下的一个直接子项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryItem {
    /// 完整路径。
    pub path: String,
    /// 是否为目录。
    pub is_directory: bool,
}

/// 当前用户的桌面目录。
pub fn desktop_folder() -> Option<String> {
    known_folder(&FOLDERID_Desktop)
}

/// 当前用户的“最近使用的项目”目录。
pub(crate) fn recent_folder() -> Option<String> {
    known_folder(&FOLDERID_Recent)
}

fn known_folder(id: &GUID) -> Option<String> {
    // SAFETY：返回的字符串由系统用 CoTaskMemAlloc 分配，转成 String 后释放一次；不使用其他用户的令牌。
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const c_void));
        text.filter(|path| !path.is_empty())
    }
}

/// 列出目录的直接子项。跳过隐藏和系统项（如 `desktop.ini`），也跳过符号链接和目录联接，
/// 不跟随重解析点到范围之外。
pub fn list_directory(path: &str) -> Result<Vec<DirectoryItem>, WindowsError> {
    let entries = std::fs::read_dir(path).map_err(|_| WindowsError::InvalidPath)?;
    let mut items = Vec::new();
    for entry in entries.flatten() {
        // DirEntry 的元数据不跟随重解析点。
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let attributes = metadata.file_attributes();
        if attributes
            & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_REPARSE_POINT)
            != 0
        {
            continue;
        }
        items.push(DirectoryItem {
            path: entry.path().to_string_lossy().into_owned(),
            is_directory: metadata.is_dir(),
        });
    }
    Ok(items)
}
