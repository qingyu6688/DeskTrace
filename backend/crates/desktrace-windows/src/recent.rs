//! 监听“最近使用的项目”。用户通过资源管理器或支持最近文档的程序打开文件时，
//! Windows 会在该目录写入或更新一个快捷方式；这里只在程序运行期间实时观察这些变化，
//! 解析出目标路径后交给采集层判断是否属于观察范围。启用前留下的历史不读取。
//!
//! 局限：用户在系统设置中关闭“显示最近打开的项目”、程序不登记最近文档、
//! 或打开的是程序与快捷方式时，都不会产生记录。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_RENAMED_NEW_NAME,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface, PCWSTR, w};

use crate::WindowsError;
use crate::com::ComApartment;
use crate::known::recent_folder;

/// 通知缓冲区大小（字节）。溢出时系统丢弃这一批通知，下一次照常继续。
const NOTIFY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_PATH_CHARS: usize = 32_768;
/// 快捷方式可能还没写完，解析失败时稍等再试一次。
const RESOLVE_RETRY_DELAY: Duration = Duration::from_millis(150);

static DROPPED_OPENS: AtomicU64 = AtomicU64::new(0);

/// 一次被登记的打开：快捷方式指向的目标路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentOpen {
    /// 目标文件或目录的完整路径。
    pub target: String,
}

/// 后台监听线程。离开作用域时通知线程退出并等待结束。
pub struct RecentWatcher {
    stop_event: isize,
    handle: Option<JoinHandle<()>>,
}

fn handle(raw: isize) -> HANDLE {
    HANDLE(raw as *mut c_void)
}

impl RecentWatcher {
    /// 开始监听，结果投递到给定的有界队列；队列满时丢弃并计数。
    pub fn start(sender: SyncSender<RecentOpen>) -> Result<Self, WindowsError> {
        let folder = recent_folder().ok_or(WindowsError::Unsupported)?;
        // SAFETY：手动重置事件只用于通知线程退出，由 Drop 关闭。
        let stop_event = unsafe { CreateEventW(None, true, false, None)? }.0 as isize;
        let (ready, started) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("desktrace-recent".into())
            .spawn(move || watch(&folder, stop_event, &sender, &ready));
        let handle = match spawned {
            Ok(handle) => handle,
            Err(_) => {
                // SAFETY：事件刚创建，线程没有启动，只在这里关闭一次。
                let _ = unsafe { CloseHandle(self::handle(stop_event)) };
                return Err(WindowsError::ThreadFailed);
            }
        };
        let watcher = Self {
            stop_event,
            handle: Some(handle),
        };
        started.recv().map_err(|_| WindowsError::ThreadFailed)??;
        Ok(watcher)
    }

    /// 因队列已满而丢弃的打开记录数。
    pub fn take_dropped() -> u64 {
        DROPPED_OPENS.swap(0, Ordering::Relaxed)
    }
}

impl Drop for RecentWatcher {
    fn drop(&mut self) {
        // SAFETY：stop_event 由 start 创建；线程结束后才关闭。
        unsafe {
            let _ = SetEvent(handle(self.stop_event));
        }
        if let Some(thread) = self.handle.take() {
            let _ = thread.join();
        }
        // SAFETY：线程已退出，不再使用该事件。
        let _ = unsafe { CloseHandle(handle(self.stop_event)) };
    }
}

/// 线程内持有的目录与 I/O 事件句柄，离开作用域时关闭。
struct WatchHandles {
    directory: HANDLE,
    io_event: HANDLE,
}

impl Drop for WatchHandles {
    fn drop(&mut self) {
        // SAFETY：两个句柄由 open_handles 创建，只在这里关闭一次。
        unsafe {
            let _ = CloseHandle(self.directory);
            let _ = CloseHandle(self.io_event);
        }
    }
}

fn open_handles(folder: &str) -> Result<WatchHandles, WindowsError> {
    // SAFETY：以列目录权限打开并共享全部访问，不影响系统写入；失败时已创建的句柄被关闭。
    unsafe {
        let directory = CreateFileW(
            &HSTRING::from(folder),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            None,
        )?;
        match CreateEventW(None, true, false, None) {
            Ok(io_event) => Ok(WatchHandles {
                directory,
                io_event,
            }),
            Err(error) => {
                let _ = CloseHandle(directory);
                Err(error.into())
            }
        }
    }
}

fn watch(
    folder: &str,
    stop_event: isize,
    sender: &SyncSender<RecentOpen>,
    ready: &mpsc::Sender<Result<(), WindowsError>>,
) {
    let setup = ComApartment::single_threaded()
        .and_then(|apartment| Ok((apartment, open_handles(folder)?)));
    let (_apartment, handles) = match setup {
        Ok(resources) => resources,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    // 用 u32 数组保证通知记录按 DWORD 对齐。
    let mut buffer = vec![0u32; NOTIFY_BUFFER_BYTES / 4];
    loop {
        let mut overlapped = OVERLAPPED {
            hEvent: handles.io_event,
            ..OVERLAPPED::default()
        };
        // SAFETY：缓冲区与 overlapped 在本次等待结束（完成或取消后确认）之前一直有效。
        let started = unsafe {
            ReadDirectoryChangesW(
                handles.directory,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 4) as u32,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
                None,
                Some(&mut overlapped),
                None,
            )
        };
        if started.is_err() {
            return;
        }
        // SAFETY：等待的两个句柄在本线程存续期间都有效。
        let woke = unsafe {
            WaitForMultipleObjects(&[handles.io_event, handle(stop_event)], false, INFINITE)
        };
        let mut transferred = 0u32;
        if woke != WAIT_OBJECT_0 {
            // 退出：取消挂起的读取，并等它确实结束后再释放缓冲区。
            // SAFETY：取消的是本线程发起、仍在使用 buffer 的那次读取。
            unsafe {
                let _ = CancelIoEx(handles.directory, Some(&overlapped));
                let _ = GetOverlappedResult(handles.directory, &overlapped, &mut transferred, true);
            }
            return;
        }
        // SAFETY：事件已触发，读取已完成。
        if unsafe { GetOverlappedResult(handles.directory, &overlapped, &mut transferred, false) }
            .is_err()
        {
            return;
        }
        // 传输量为 0 表示缓冲区溢出，这一批通知已丢失。
        if transferred == 0 {
            DROPPED_OPENS.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        for name in shortcut_names(&buffer, transferred as usize) {
            let path = format!("{folder}\\{name}");
            if let Some(target) = resolve_shortcut(&path)
                && let Err(TrySendError::Full(_)) = sender.try_send(RecentOpen { target })
            {
                DROPPED_OPENS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// 从通知缓冲区取出新增、修改或改名后的 `.lnk` 文件名。
fn shortcut_names(buffer: &[u32], length: usize) -> Vec<String> {
    let base = buffer.as_ptr().cast::<u8>();
    let mut names = Vec::new();
    let mut offset = 0usize;
    while offset + size_of::<FILE_NOTIFY_INFORMATION>() <= length {
        // SAFETY：offset 来自系统写入的 NextEntryOffset，始终位于已传输的字节范围内且 DWORD 对齐；
        // 文件名长度以字节给出，读取不超过记录本身。
        let (action, name, next) = unsafe {
            let info = &*base.add(offset).cast::<FILE_NOTIFY_INFORMATION>();
            let units = info.FileNameLength as usize / 2;
            let name = std::slice::from_raw_parts(info.FileName.as_ptr(), units);
            (
                info.Action,
                String::from_utf16_lossy(name),
                info.NextEntryOffset as usize,
            )
        };
        let relevant = action == FILE_ACTION_ADDED
            || action == FILE_ACTION_MODIFIED
            || action == FILE_ACTION_RENAMED_NEW_NAME;
        if relevant && name.to_ascii_lowercase().ends_with(".lnk") && !names.contains(&name) {
            names.push(name);
        }
        if next == 0 {
            break;
        }
        offset += next;
    }
    names
}

/// 读取快捷方式的目标路径；不是文件系统目标（例如控制面板项）时返回 `None`。
fn resolve_shortcut(path: &str) -> Option<String> {
    read_target(path).or_else(|| {
        std::thread::sleep(RESOLVE_RETRY_DELAY);
        read_target(path)
    })
}

fn read_target(path: &str) -> Option<String> {
    // SAFETY：COM 对象在本线程的 STA 中创建和使用；目标缓冲区为本地数组。
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(path), STGM_READ).ok()?;
        let mut buffer = vec![0u16; MAX_PATH_CHARS];
        link.GetPath(&mut buffer, std::ptr::null_mut(), 0).ok()?;
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        let target = String::from_utf16_lossy(&buffer[..end]);
        (!target.is_empty()).then_some(target)
    }
}

const ADVANCED_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");
const POLICY_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer");

fn read_dword(root: HKEY, key: PCWSTR, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY：只读一个 DWORD，输出缓冲区为本地变量，大小匹配。
    let status = unsafe {
        RegGetValueW(
            root,
            key,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

/// 系统是否在记录“最近打开的项目”。用户在“设置 > 个性化 > 开始”中关闭，或组策略禁止时返回假；
/// 此时打开文件不会产生任何记录。没有登记时按系统默认（开启）处理。
pub fn recent_tracking_enabled() -> bool {
    let disabled_by_policy = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .any(|root| read_dword(root, POLICY_KEY, w!("NoRecentDocsHistory")) == Some(1));
    let disabled_by_user =
        read_dword(HKEY_CURRENT_USER, ADVANCED_KEY, w!("Start_TrackDocs")) == Some(0);
    !(disabled_by_policy || disabled_by_user)
}
