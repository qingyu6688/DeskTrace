//! 每个用户只运行一个实例。后启动的实例通知已有实例显示窗口后退出，避免重复采集和多处写库。

use std::ffi::c_void;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent,
    WaitForSingleObject,
};
use windows::core::{PCWSTR, w};

use crate::WindowsError;

// `Local\` 前缀把对象限定在当前登录会话，不同用户各自运行一个实例。
const MUTEX_NAME: PCWSTR = w!("Local\\DeskTrace.Instance");
const EVENT_NAME: PCWSTR = w!("Local\\DeskTrace.Activate");

/// 实例角色。
pub enum InstanceRole {
    /// 第一个实例，持有互斥体。
    Primary(SingleInstance),
    /// 已有实例在运行，已通知其显示窗口。
    Secondary,
}

/// 主实例持有的互斥体与唤起事件。句柄以整数保存，只在本结构内使用。
pub struct SingleInstance {
    mutex: isize,
    event: isize,
}

fn handle(raw: isize) -> HANDLE {
    HANDLE(raw as *mut c_void)
}

impl SingleInstance {
    /// 尝试成为主实例；已有实例时通知它并返回 `Secondary`。
    pub fn acquire() -> Result<InstanceRole, WindowsError> {
        // SAFETY：命名对象只在当前会话可见；返回的句柄由 SingleInstance 的 Drop 关闭。
        unsafe {
            let mutex = CreateMutexW(None, true, MUTEX_NAME)?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(mutex);
                if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, EVENT_NAME) {
                    let _ = SetEvent(event);
                    let _ = CloseHandle(event);
                }
                return Ok(InstanceRole::Secondary);
            }
            let event = match CreateEventW(None, false, false, EVENT_NAME) {
                Ok(event) => event,
                Err(error) => {
                    let _ = CloseHandle(mutex);
                    return Err(error.into());
                }
            };
            Ok(InstanceRole::Primary(Self {
                mutex: mutex.0 as isize,
                event: event.0 as isize,
            }))
        }
    }

    /// 在后台线程等待其他实例的唤起通知。主实例存续期间句柄一直有效；
    /// 句柄被关闭后等待失败，线程随之结束。
    pub fn watch_activation(
        &self,
        on_activate: impl Fn() + Send + 'static,
    ) -> Result<(), WindowsError> {
        let event = self.event;
        std::thread::Builder::new()
            .name("desktrace-instance".into())
            .spawn(move || {
                loop {
                    // SAFETY：event 是主实例创建的自动重置事件，只用于等待。
                    let result = unsafe { WaitForSingleObject(handle(event), INFINITE) };
                    if result != WAIT_OBJECT_0 {
                        break;
                    }
                    on_activate();
                }
            })
            .map(|_| ())
            .map_err(|_| WindowsError::ThreadFailed)
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY：两个句柄由 acquire 创建，只在这里关闭一次。
        unsafe {
            let _ = CloseHandle(handle(self.event));
            let _ = CloseHandle(handle(self.mutex));
        }
    }
}
