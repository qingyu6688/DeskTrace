//! 目录的稳定身份：卷序列号加 128 位文件 ID。重命名和同卷移动后保持不变。

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
};
use windows::core::HSTRING;

/// 读取目录的 `卷序列号:文件 ID`；无法打开或文件系统不提供时返回 `None`，调用方改用路径身份。
pub fn folder_file_identity(path: &str) -> Option<String> {
    let wide = HSTRING::from(path);
    // SAFETY：只申请读取属性权限并共享全部访问，不会阻塞其他程序；
    // 返回的句柄在本函数内用完并关闭。
    let handle = unsafe {
        CreateFileW(
            &wide,
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .ok()?;
    let identity = read_identity(handle);
    // SAFETY：handle 由上面的 CreateFileW 成功返回，只关闭一次。
    let _ = unsafe { CloseHandle(handle) };
    identity
}

fn read_identity(handle: HANDLE) -> Option<String> {
    let mut info = FILE_ID_INFO::default();
    // SAFETY：缓冲区是本函数栈上的 FILE_ID_INFO，大小与信息类别一致。
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).ok()?,
        )
        .ok()?;
    }
    let file_id: String = info
        .FileId
        .Identifier
        .iter()
        .rev()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    Some(format!("{:016X}:{file_id}", info.VolumeSerialNumber))
}
