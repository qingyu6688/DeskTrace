//! 前台应用解析与系统信号监听。
//!
//! 回调只投递一个信号到有界队列，不做解析、不访问数据库；采集线程收到信号后再读取前台窗口。
//! 锁屏/解锁与休眠/唤醒通过同一线程上的隐藏窗口接收。

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, FILETIME, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
    EnumChildWindows, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG,
    OBJID_WINDOW, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PostThreadMessageW,
    RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WM_POWERBROADCAST, WM_QUIT, WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_LOCK,
    WTS_SESSION_UNLOCK,
};
use windows::core::{BOOL, PWSTR, w};

use crate::WindowsError;

/// 系统信号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemSignal {
    /// 前台窗口切换。
    Foreground,
    /// 当前会话锁定。
    SessionLocked,
    /// 当前会话解锁。
    SessionUnlocked,
    /// 系统即将休眠。
    Suspended,
    /// 系统从休眠恢复。
    Resumed,
}

/// 前台应用的身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForegroundIdentity {
    /// 打包应用入口。
    Package {
        /// 应用用户模型 ID。
        aumid: String,
    },
    /// 普通可执行文件。
    Executable {
        /// 完整路径。
        path: String,
    },
}

/// 一次前台解析结果。进程 ID 与创建时间一起用于识别 PID 重用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForegroundApp {
    /// 身份。
    pub identity: ForegroundIdentity,
    /// 前台窗口句柄，供资源管理器探测使用。
    pub window: isize,
    /// 进程 ID。
    pub process_id: u32,
    /// 进程创建时间（FILETIME 刻度）。
    pub process_created: u64,
    /// 是否为资源管理器文件窗口所在的 explorer.exe。
    pub is_explorer: bool,
}

/// 外壳与系统界面进程：任务栏、开始菜单、锁屏等不是用户打开的应用。
const SYSTEM_PROCESSES: [&str; 7] = [
    "lockapp.exe",
    "searchhost.exe",
    "searchapp.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "textinputhost.exe",
    "shellhost.exe",
];
const MAX_PATH_CHARS: usize = 32_768;
const MAX_AUMID_CHARS: usize = 130;

/// 回调投递目标。回调不能捕获环境，只能通过静态量传递。
static SIGNAL_SINK: Mutex<Option<SyncSender<SystemSignal>>> = Mutex::new(None);
static DROPPED_SIGNALS: AtomicU64 = AtomicU64::new(0);

fn emit(signal: SystemSignal) {
    let Ok(guard) = SIGNAL_SINK.lock() else {
        return;
    };
    if let Some(sender) = guard.as_ref()
        && let Err(TrySendError::Full(_)) = sender.try_send(signal)
    {
        DROPPED_SIGNALS.fetch_add(1, Ordering::Relaxed);
    }
}

/// 解析当前前台窗口所属的应用。没有前台窗口、属于本程序或系统界面时返回 `None`。
pub fn resolve_foreground() -> Option<ForegroundApp> {
    // SAFETY：GetForegroundWindow 无参数，返回值可能为空句柄，随后检查。
    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }
    let mut process_id = 0u32;
    // SAFETY：window 是刚取得的窗口句柄，输出参数为本地变量。
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    // SAFETY：无参数。
    if process_id == 0 || process_id == unsafe { GetCurrentProcessId() } {
        return None;
    }
    let process = ProcessHandle::open(process_id)?;
    let path = process.image_path()?;
    let file_name = Path::new(&path)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    if file_name == "applicationframehost.exe" {
        // UWP 窗口由宿主进程承载，真实应用进程是某个子窗口的所有者。
        return hosted_app(window, process_id);
    }
    if SYSTEM_PROCESSES.contains(&file_name.as_str()) {
        return None;
    }
    let identity = match process.aumid() {
        Some(aumid) => ForegroundIdentity::Package { aumid },
        None => ForegroundIdentity::Executable { path },
    };
    Some(ForegroundApp {
        identity,
        window: window.0 as isize,
        process_id,
        process_created: process.created().unwrap_or(0),
        is_explorer: file_name == "explorer.exe",
    })
}

fn hosted_app(window: HWND, host_id: u32) -> Option<ForegroundApp> {
    struct Search {
        host_id: u32,
        found: u32,
    }
    unsafe extern "system" fn visit(child: HWND, state: LPARAM) -> BOOL {
        // SAFETY：state 指向 hosted_app 栈上的 Search，枚举期间一直有效。
        let search = unsafe { &mut *(state.0 as *mut Search) };
        let mut owner = 0u32;
        // SAFETY：child 由系统在枚举中传入。
        unsafe { GetWindowThreadProcessId(child, Some(&mut owner)) };
        if owner != 0 && owner != search.host_id {
            search.found = owner;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut search = Search { host_id, found: 0 };
    // SAFETY：回调只在本调用期间被同步调用，LPARAM 指向的 Search 生命周期覆盖整个枚举。
    let _ = unsafe {
        EnumChildWindows(
            Some(window),
            Some(visit),
            LPARAM((&raw mut search) as isize),
        )
    };
    if search.found == 0 {
        return None;
    }
    let process = ProcessHandle::open(search.found)?;
    Some(ForegroundApp {
        identity: ForegroundIdentity::Package {
            aumid: process.aumid()?,
        },
        window: window.0 as isize,
        process_id: search.found,
        process_created: process.created().unwrap_or(0),
        is_explorer: false,
    })
}

/// 以最小查询权限打开的进程句柄。
struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Option<Self> {
        // SAFETY：只申请受限查询权限；提升权限或受保护进程打开失败时返回 None。
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
            .ok()
            .map(Self)
    }

    fn image_path(&self) -> Option<String> {
        let mut buffer = vec![0u16; MAX_PATH_CHARS];
        let mut length = MAX_PATH_CHARS as u32;
        // SAFETY：缓冲区长度按字符数传入。
        unsafe {
            QueryFullProcessImageNameW(
                self.0,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        }
        .ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }

    fn aumid(&self) -> Option<String> {
        let mut buffer = [0u16; MAX_AUMID_CHARS];
        let mut length = MAX_AUMID_CHARS as u32;
        // SAFETY：缓冲区长度按字符数传入；没有包身份的进程返回错误码。
        let status = unsafe {
            GetApplicationUserModelId(self.0, &mut length, Some(PWSTR(buffer.as_mut_ptr())))
        };
        if status != ERROR_SUCCESS || length == 0 {
            return None;
        }
        // 返回长度包含结尾的 0。
        let text = String::from_utf16_lossy(&buffer[..length.saturating_sub(1) as usize]);
        (!text.is_empty()).then_some(text)
    }

    fn created(&self) -> Option<u64> {
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY：四个输出参数都是本地变量。
        unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY：句柄由 OpenProcess 成功返回，只关闭一次。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// 距最后一次键盘或鼠标输入的毫秒数。只读取时间，不读取输入内容。
/// 计数器约 49.7 天回绕一次，按无符号差值计算；读取失败返回 `None`。
pub fn idle_milliseconds() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY：结构大小已正确设置。
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    // SAFETY：无参数。
    let now = unsafe { GetTickCount() };
    Some(now.wrapping_sub(info.dwTime))
}

/// 在专用线程上监听前台切换和会话、电源变化。
pub struct ForegroundWatcher {
    thread_id: u32,
    handle: Option<JoinHandle<()>>,
}

impl ForegroundWatcher {
    /// 开始监听，信号投递到给定的有界队列；队列满时信号被丢弃并计数。
    pub fn start(sender: SyncSender<SystemSignal>) -> Result<Self, WindowsError> {
        if let Ok(mut sink) = SIGNAL_SINK.lock() {
            *sink = Some(sender);
        }
        let (ready, started) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("desktrace-foreground".into())
            .spawn(move || run_message_loop(&ready))
            .map_err(|_| WindowsError::ThreadFailed)?;
        let thread_id = started.recv().map_err(|_| WindowsError::ThreadFailed)??;
        Ok(Self {
            thread_id,
            handle: Some(handle),
        })
    }

    /// 因队列已满而丢弃的信号数，采集层据此记录 `queue_overflow` 缺口。
    pub fn take_dropped() -> u64 {
        DROPPED_SIGNALS.swap(0, Ordering::Relaxed)
    }
}

impl Drop for ForegroundWatcher {
    fn drop(&mut self) {
        // SAFETY：thread_id 属于本结构启动且仍在运行消息循环的线程。
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if let Ok(mut sink) = SIGNAL_SINK.lock() {
            *sink = None;
        }
    }
}

unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    _window: HWND,
    object_id: i32,
    _child_id: i32,
    _thread_id: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND && object_id == OBJID_WINDOW.0 {
        emit(SystemSignal::Foreground);
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match (message, wparam.0 as u32) {
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) => emit(SystemSignal::SessionLocked),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK) => emit(SystemSignal::SessionUnlocked),
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => emit(SystemSignal::Suspended),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => {
            emit(SystemSignal::Resumed)
        }
        _ => {}
    }
    // SAFETY：未处理的消息交给系统默认处理，参数原样传递。
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// 创建隐藏的顶层窗口（电源广播只发给顶层窗口）并挂接前台事件，然后运行消息循环直到收到 WM_QUIT。
fn run_message_loop(ready: &mpsc::Sender<Result<u32, WindowsError>>) {
    // SAFETY：本函数内创建的窗口、钩子和会话通知都在同一线程上注册与注销；
    // 窗口从不显示，过程函数只投递信号。
    unsafe {
        let setup = || -> Result<(HWND, HWINEVENTHOOK), WindowsError> {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("DeskTraceSignals"),
                ..WNDCLASSW::default()
            };
            // 重复注册同名类会失败，但已注册的类仍可使用。
            RegisterClassW(&class);
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("DeskTraceSignals"),
                w!("DeskTrace"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            // 会话通知注册失败只影响锁屏识别，空闲判断仍可工作。
            let _ = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION);
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if hook.0.is_null() {
                let _ = DestroyWindow(window);
                return Err(WindowsError::Unsupported);
            }
            Ok((window, hook))
        };
        let (window, hook) = match setup() {
            Ok(resources) => resources,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        let _ = ready.send(Ok(GetCurrentThreadId()));
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
        let _ = UnhookWinEvent(hook);
        let _ = WTSUnRegisterSessionNotification(window);
        let _ = DestroyWindow(window);
    }
}
