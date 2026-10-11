//! 当前用户的开机启动项，只写 `HKCU\...\Run`，不需要管理员权限。

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::WindowsError;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("DeskTrace");

/// 开启或关闭开机启动。`command` 是完整的启动命令，例如带引号的程序路径和 `--autostart`。
pub fn set_autostart(enabled: bool, command: &str) -> Result<(), WindowsError> {
    let status = if enabled {
        let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = u32::try_from(wide.len() * 2).map_err(|_| WindowsError::InvalidPath)?;
        // SAFETY：数据指针指向以 0 结尾的 UTF-16 缓冲区，长度按字节计算，调用期间有效。
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE_NAME,
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                bytes,
            )
        }
    } else {
        // SAFETY：只删除本程序写入的值，键名与值名为静态常量。
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) } {
            status if status == ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
            status => status,
        }
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(WindowsError::Api {
            code: status.0 as i32,
        })
    }
}

/// 读取当前登记的开机启动命令，没有登记时返回 `None`。
pub fn autostart_enabled() -> Result<Option<String>, WindowsError> {
    let mut size = 0u32;
    // SAFETY：第一次只查询所需字节数，不写数据。
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(WindowsError::Api {
            code: status.0 as i32,
        });
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY：缓冲区按上一步返回的字节数分配，系统写入不会越界。
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(WindowsError::Api {
            code: status.0 as i32,
        });
    }
    let text = HSTRING::from_wide(
        &buffer[..buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len())],
    );
    Ok(Some(text.to_string()))
}
