//! 采集调度：接收系统信号，解析前台应用、资源管理器活动目录与“最近使用的项目”，
//! 登记观察范围内的直接子项，维护两个模块的采集区间。
//!
//! 区间主因按详细设计 7.1 节的优先级选择：时钟异常 → 存储/队列/采集故障 → 用户暂停 →
//! 休眠/锁屏/空闲 → 无法识别视图 → 正常采集。所有数据库操作经数据库工作线程串行执行。

mod explorer_worker;

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use desktrace_core::{
    Availability, CollectionState, CollectorModule, IntervalReason, IntervalState, ObjectType,
    UsageEventKind, UsageObservation, display_name, file_open_observable, format_utc,
    normalize_local_path,
};
use desktrace_storage::{
    AppIdentity, ObservableObject, PathKind, PathTarget, ScopeItem, StorageError,
};
use desktrace_windows::{
    ExplorerView, ForegroundApp, ForegroundIdentity, ForegroundWatcher, RecentOpen, RecentWatcher,
    SystemSignal, folder_file_identity, idle_milliseconds, list_directory, recent_tracking_enabled,
    resolve_foreground,
};
use serde::Serialize;

use crate::services::clock::now_ms;
use crate::services::notifier::Notifier;
use crate::services::storage::{ServiceError, StorageService};
use explorer_worker::{ExplorerWorker, ProbeError};

/// 等待信号的最长时间，也是定时检查的粒度。
const TICK: Duration = Duration::from_millis(250);
/// 有界信号队列容量，满时丢弃并形成 `queue_overflow` 缺口。
const SIGNAL_QUEUE_CAPACITY: usize = 4096;
/// “最近使用的项目”队列容量；一次打开通常只产生一两条。
const RECENT_QUEUE_CAPACITY: usize = 256;
const HEARTBEAT_INTERVAL_MS: i64 = 15_000;
const FOREGROUND_CHECK_MS: i64 = 1_000;
const ACTIVE_SAMPLE_MS: i64 = 60_000;
const IDLE_THRESHOLD_MS: u32 = 300_000;
const EXPLORER_PROBE_MS: i64 = 2_000;
const EXPLORER_TIMEOUT: Duration = Duration::from_secs(2);
const EXPLORER_BACKOFF_MS: i64 = 30_000;
/// 墙上时间与单调时钟的偏差超过 5 分钟视为系统时间变化。
const CLOCK_JUMP_MS: i64 = 300_000;
/// 休眠恢复后的这段时间内不把时间跳变当作时钟异常。
const POWER_GRACE_MS: i64 = 120_000;
/// 队列溢出和时钟异常的缺口至少保留 1 秒，避免零长度区间被忽略。
const MARKER_SPAN_MS: i64 = 1_000;
const PURGE_INTERVAL_MS: i64 = 3_600_000;
/// 范围直接子项扫描与可访问性检查的间隔。
const AVAILABILITY_INTERVAL_MS: i64 = 600_000;
const CATALOG_INTERVAL_MS: i64 = 6 * 3_600_000;
/// 检查“显示最近打开的项目”开关的间隔，用户在系统设置里改动后半分钟内生效。
const TRACKING_CHECK_MS: i64 = 30_000;
/// 系统关闭“最近使用的项目”时附加的限制说明。
const TRACKING_DISABLED_NOTE: &str =
    "系统关闭了“显示最近打开的项目”，桌面文件的打开无法记录；可在“设置 > 个性化 > 开始”中开启";

/// 一个模块的当前状态。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModuleStatus {
    pub(crate) module: CollectorModule,
    /// 区间状态；尚未启用记录时为空。
    pub(crate) state: Option<IntervalState>,
    pub(crate) reason: Option<&'static str>,
    pub(crate) reason_label: Option<&'static str>,
    /// 来源是否健康。
    pub(crate) healthy: bool,
    pub(crate) last_heartbeat_at: Option<String>,
}

/// 最近一次清单刷新的结果。
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogStatus {
    pub(crate) last_refreshed_at: Option<String>,
    pub(crate) complete: Option<bool>,
    pub(crate) running_job_id: Option<String>,
}

/// 采集状态快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CollectorStatus {
    pub(crate) collection_state: CollectionState,
    pub(crate) implemented: bool,
    pub(crate) modules: Vec<ModuleStatus>,
    pub(crate) catalog: CatalogStatus,
    pub(crate) limitations: Vec<&'static str>,
}

impl CollectorStatus {
    fn initial() -> Self {
        Self {
            collection_state: CollectionState::NotStarted,
            implemented: desktrace_windows::collector_implemented(),
            modules: CollectorModule::ALL
                .into_iter()
                .map(|module| ModuleStatus {
                    module,
                    state: None,
                    reason: None,
                    reason_label: None,
                    healthy: true,
                    last_heartbeat_at: None,
                })
                .collect(),
            catalog: CatalogStatus::default(),
            limitations: vec![
                "只记录应用进入前台和资源管理器活动标签页，后台运行不算使用",
                "资源管理器多标签页识别尚未完成实机验证，识别不了时记为“无法识别当前视图”",
                "启用前的使用历史无法恢复",
                "文件打开依靠系统“最近使用的项目”：程序、快捷方式和不登记最近文档的软件记录不到",
            ],
        }
    }
}

enum Control {
    SetState(CollectionState),
    /// 清除记录后区间表已清空，重新建立区间与观察。
    Reset,
    /// 观察范围有变化，立即重新扫描范围直接子项。
    RescanScopes,
    Shutdown(Sender<()>),
}

/// 采集线程句柄。
pub(crate) struct Collector {
    control: Sender<Control>,
    status: Arc<Mutex<CollectorStatus>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Collector {
    /// 启动采集线程。`on_catalog_due` 在需要定时刷新清单时被调用。
    pub(crate) fn start(
        storage: StorageService,
        notifier: Arc<Notifier>,
        on_catalog_due: Box<dyn Fn() + Send>,
    ) -> Result<Self, std::io::Error> {
        let (control, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(CollectorStatus::initial()));
        let shared = status.clone();
        let thread = std::thread::Builder::new()
            .name("desktrace-collector".into())
            .spawn(move || run(storage, notifier, shared, on_catalog_due, &receiver))?;
        Ok(Self {
            control,
            status,
            thread: Mutex::new(Some(thread)),
        })
    }

    pub(crate) fn status(&self) -> CollectorStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| CollectorStatus::initial())
    }

    /// 更新清单刷新状态。
    pub(crate) fn update_catalog(&self, change: impl FnOnce(&mut CatalogStatus)) {
        if let Ok(mut status) = self.status.lock() {
            change(&mut status.catalog);
        }
    }

    /// 通知采集线程新的用户意图；持久化由调用方先完成。
    pub(crate) fn set_state(&self, state: CollectionState) {
        let _ = self.control.send(Control::SetState(state));
    }

    /// 观察范围变化后立即重新扫描。
    pub(crate) fn rescan_scopes(&self) {
        let _ = self.control.send(Control::RescanScopes);
    }

    /// 清除记录后重置区间状态。
    pub(crate) fn reset(&self) {
        let _ = self.control.send(Control::Reset);
    }

    /// 结束采集：把开放区间切换为“程序已退出”，最多等待 3 秒。
    pub(crate) fn shutdown(&self) {
        let (reply, done) = mpsc::channel();
        if self.control.send(Control::Shutdown(reply)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(3));
        }
        if let Ok(mut thread) = self.thread.lock()
            && let Some(handle) = thread.take()
            && handle.is_finished()
        {
            let _ = handle.join();
        }
    }
}

struct CurrentApp {
    identity: ForegroundIdentity,
    process_id: u32,
    process_created: u64,
    window: isize,
    is_explorer: bool,
    object: Option<ObservableObject>,
    last_record_ms: i64,
}

struct CurrentFolder {
    key: String,
    object: Option<ObservableObject>,
    last_record_ms: i64,
}

struct Loop {
    storage: StorageService,
    notifier: Arc<Notifier>,
    status: Arc<Mutex<CollectorStatus>>,
    on_catalog_due: Box<dyn Fn() + Send>,
    watcher: Option<ForegroundWatcher>,
    explorer: Option<ExplorerWorker>,
    recent: Option<RecentWatcher>,
    recent_opens: Receiver<RecentOpen>,
    recent_tracking: bool,
    last_tracking_check_ms: i64,
    collection: CollectionState,
    first_start: bool,
    locked: bool,
    suspended: bool,
    idle: bool,
    unsupported_view: bool,
    explorer_backoff_until: i64,
    storage_fault: bool,
    overflow_until: i64,
    clock_change_until: i64,
    power_event_ms: i64,
    current_reason: HashMap<CollectorModule, IntervalReason>,
    current_app: Option<CurrentApp>,
    current_folder: Option<CurrentFolder>,
    last_heartbeat_ms: i64,
    last_foreground_check_ms: i64,
    last_probe_ms: i64,
    last_purge_ms: i64,
    last_availability_ms: i64,
    last_catalog_ms: i64,
    clock_reference: (i64, Instant),
}

fn run(
    storage: StorageService,
    notifier: Arc<Notifier>,
    status: Arc<Mutex<CollectorStatus>>,
    on_catalog_due: Box<dyn Fn() + Send>,
    control: &Receiver<Control>,
) {
    let (signal_sender, signals) = mpsc::sync_channel(SIGNAL_QUEUE_CAPACITY);
    let (recent_sender, recent_opens) = mpsc::sync_channel(RECENT_QUEUE_CAPACITY);
    let now = now_ms();
    let collection = storage
        .call(|storage| storage.collection_state())
        .unwrap_or(CollectionState::NotStarted);
    let mut state = Loop {
        watcher: ForegroundWatcher::start(signal_sender).ok(),
        explorer: ExplorerWorker::start(),
        recent: RecentWatcher::start(recent_sender).ok(),
        recent_opens,
        recent_tracking: recent_tracking_enabled(),
        last_tracking_check_ms: now,
        storage,
        notifier,
        status,
        on_catalog_due,
        collection,
        first_start: false,
        locked: false,
        suspended: false,
        idle: false,
        unsupported_view: false,
        explorer_backoff_until: 0,
        storage_fault: false,
        overflow_until: 0,
        clock_change_until: 0,
        power_event_ms: 0,
        current_reason: HashMap::new(),
        current_app: None,
        current_folder: None,
        last_heartbeat_ms: now,
        last_foreground_check_ms: 0,
        last_probe_ms: 0,
        last_purge_ms: 0,
        // 置 0 让第一次定时检查立即扫描范围直接子项。
        last_availability_ms: 0,
        // 启动后先等清单首次刷新完成，定时刷新从现在开始计时。
        last_catalog_ms: now,
        clock_reference: (now, Instant::now()),
    };
    // 首次同步前先读空闲状态，避免启动时写出一段零长度的正常采集区间。
    state.refresh_idle();
    state.sync_intervals(now);
    loop {
        while let Ok(command) = control.try_recv() {
            match command {
                Control::SetState(next) => state.set_collection(next),
                Control::Reset => state.reset(),
                Control::RescanScopes => state.last_availability_ms = 0,
                Control::Shutdown(reply) => {
                    state.shutdown();
                    let _ = reply.send(());
                    return;
                }
            }
        }
        match signals.recv_timeout(TICK) {
            Ok(signal) => state.on_signal(signal),
            Err(RecvTimeoutError::Timeout) => {}
            // 监听线程退出后仍保持定时检查，应用模块会显示故障。
            Err(RecvTimeoutError::Disconnected) => std::thread::sleep(TICK),
        }
        state.tick();
    }
}

impl Loop {
    fn active(&self) -> bool {
        self.collection == CollectionState::Running && !self.locked && !self.suspended
    }

    fn set_collection(&mut self, next: CollectionState) {
        if self.collection == CollectionState::NotStarted && next == CollectionState::Running {
            self.first_start = true;
        }
        self.collection = next;
        if next != CollectionState::Running {
            self.current_app = None;
            self.current_folder = None;
        }
        self.sync_intervals(now_ms());
    }

    fn reset(&mut self) {
        self.current_reason.clear();
        self.current_app = None;
        self.current_folder = None;
        self.first_start = self.collection == CollectionState::Running;
        self.sync_intervals(now_ms());
    }

    fn on_signal(&mut self, signal: SystemSignal) {
        let now = now_ms();
        match signal {
            SystemSignal::Foreground => self.check_foreground(now),
            SystemSignal::SessionLocked => self.locked = true,
            SystemSignal::SessionUnlocked => self.locked = false,
            SystemSignal::Suspended => {
                self.suspended = true;
                self.power_event_ms = now;
            }
            SystemSignal::Resumed => {
                self.suspended = false;
                self.power_event_ms = now;
                // 恢复后单调时钟与墙上时间的关系可能变化，重新取基准。
                self.clock_reference = (now, Instant::now());
            }
        }
        if !self.active() {
            self.current_app = None;
            self.current_folder = None;
        }
        self.sync_intervals(now);
    }

    fn tick(&mut self) {
        let now = now_ms();
        self.detect_clock_change(now);
        if ForegroundWatcher::take_dropped() + RecentWatcher::take_dropped() > 0 {
            self.overflow_until = now + MARKER_SPAN_MS;
        }
        // 读不到空闲时间时按未知处理：不写活跃采样，也不据此生成非活跃区间。
        self.refresh_idle();
        if now - self.last_foreground_check_ms >= FOREGROUND_CHECK_MS {
            self.last_foreground_check_ms = now;
            self.check_foreground(now);
        }
        self.probe_explorer(now);
        if now - self.last_tracking_check_ms >= TRACKING_CHECK_MS {
            self.last_tracking_check_ms = now;
            self.recent_tracking = recent_tracking_enabled();
        }
        while let Ok(open) = self.recent_opens.try_recv() {
            self.on_recent_open(&open.target, now);
        }
        self.sync_intervals(now);
        self.maintenance(now);
        self.notifier.flush(true);
    }

    /// 读不到空闲时间时按未知处理：不据此生成非活跃区间，但也不把它当作有人在用。
    fn refresh_idle(&mut self) {
        self.idle = idle_milliseconds().is_some_and(|value| value >= IDLE_THRESHOLD_MS);
    }

    fn detect_clock_change(&mut self, now: i64) {
        let (wall, instant) = self.clock_reference;
        let monotonic = i64::try_from(instant.elapsed().as_millis()).unwrap_or(i64::MAX);
        let drift = (now - wall) - monotonic;
        self.clock_reference = (now, Instant::now());
        // 时间倒退一定是异常；向前跳变在休眠恢复附近属于正常现象。
        let backwards = drift < -CLOCK_JUMP_MS;
        let forwards = drift > CLOCK_JUMP_MS && now - self.power_event_ms > POWER_GRACE_MS;
        if backwards || forwards {
            self.clock_change_until = now + MARKER_SPAN_MS;
        }
    }

    /// 资源管理器探测不可用或正在退避时，文件夹模块算故障。
    fn folders_faulty(&self, now: i64) -> bool {
        self.explorer.is_none() || now < self.explorer_backoff_until
    }

    /// 按优先级选择模块当前应处的区间原因；尚未启用记录时为 `None`。
    fn desired_reason(&self, module: CollectorModule, now: i64) -> Option<IntervalReason> {
        use IntervalReason as R;
        match self.collection {
            CollectionState::NotStarted => return None,
            CollectionState::Paused => return Some(R::UserPaused),
            CollectionState::Running => {}
        }
        let module_fault = match module {
            CollectorModule::Applications => self.watcher.is_none(),
            CollectorModule::Folders => self.folders_faulty(now),
            CollectorModule::Files => self.recent.is_none(),
        };
        Some(if now < self.clock_change_until {
            R::ClockChange
        } else if self.storage_fault {
            R::StorageError
        } else if now < self.overflow_until {
            R::QueueOverflow
        } else if module_fault {
            R::CollectorFault
        } else if module == CollectorModule::Files && !self.recent_tracking {
            R::SourceDisabled
        } else if self.suspended {
            R::SystemSuspended
        } else if self.locked {
            R::SessionLocked
        } else if self.idle {
            R::UserIdle
        } else if module == CollectorModule::Folders && self.unsupported_view {
            R::UnsupportedView
        } else if self.first_start {
            R::CollectionStarted
        } else if self.current_reason.get(&module) == Some(&R::CollectorFault) {
            R::CollectorRecovered
        } else {
            R::CollectionResumed
        })
    }

    fn sync_intervals(&mut self, now: i64) {
        let mut changed = false;
        for module in CollectorModule::ALL {
            let Some(desired) = self.desired_reason(module, now) else {
                continue;
            };
            let current = self.current_reason.get(&module).copied();
            // 两个正常采集原因之间切换不需要拆分区间。
            let equivalent = current.is_some_and(|current| {
                current == desired
                    || (current.state() == IntervalState::Active
                        && desired.state() == IntervalState::Active)
            });
            if equivalent {
                continue;
            }
            match self
                .storage
                .call(move |storage| storage.transition_interval(module, desired, now))
            {
                Ok(()) => {
                    self.current_reason.insert(module, desired);
                    changed = true;
                }
                Err(error) => self.on_storage_error(&error),
            }
        }
        if self.first_start
            && self
                .current_reason
                .values()
                .all(|reason| reason.state() == IntervalState::Active)
        {
            self.first_start = false;
        }
        if self.collection != CollectionState::NotStarted
            && now - self.last_heartbeat_ms >= HEARTBEAT_INTERVAL_MS
        {
            self.last_heartbeat_ms = now;
            let result = self.storage.call(move |storage| {
                for module in CollectorModule::ALL {
                    storage.heartbeat(module, now)?;
                }
                Ok(())
            });
            match result {
                Ok(()) => self.storage_fault = false,
                Err(error) => self.on_storage_error(&error),
            }
            changed = true;
        }
        if changed {
            self.publish_status(now);
        }
    }

    fn on_storage_error(&mut self, error: &ServiceError) {
        // 业务校验失败不算存储故障；数据库本身不可用时停止刷新证据并显示状态。
        if matches!(
            error,
            ServiceError::Stopped
                | ServiceError::Unavailable(_)
                | ServiceError::Storage(StorageError::Database(_))
        ) {
            self.storage_fault = true;
        }
    }

    fn publish_status(&self, now: i64) {
        let Ok(mut status) = self.status.lock() else {
            return;
        };
        let before: Vec<Option<&'static str>> =
            status.modules.iter().map(|module| module.reason).collect();
        status.collection_state = self.collection;
        for module in &mut status.modules {
            let reason = self.current_reason.get(&module.module).copied();
            module.state = reason.map(IntervalReason::state);
            module.reason = reason.map(IntervalReason::as_str);
            module.reason_label = reason.map(IntervalReason::label);
            module.healthy = !self.storage_fault
                && match module.module {
                    CollectorModule::Applications => self.watcher.is_some(),
                    CollectorModule::Folders => !self.folders_faulty(now),
                    CollectorModule::Files => self.recent.is_some() && self.recent_tracking,
                };
            if reason.is_some() {
                module.last_heartbeat_at = Some(format_utc(now));
            }
        }
        status
            .limitations
            .retain(|note| *note != TRACKING_DISABLED_NOTE);
        if !self.recent_tracking {
            status.limitations.push(TRACKING_DISABLED_NOTE);
        }
        let after: Vec<Option<&'static str>> =
            status.modules.iter().map(|module| module.reason).collect();
        let snapshot = status.clone();
        drop(status);
        if before != after {
            self.notifier.emit("collector-status-changed", snapshot);
        }
    }

    fn check_foreground(&mut self, now: i64) {
        if !self.active() {
            self.current_app = None;
            return;
        }
        // 前台可能被程序抢走（计划任务、安装程序）。重新读取空闲时间，用户不在场时只更新当前对象、不写事件。
        self.refresh_idle();
        let Some(app) = resolve_foreground() else {
            self.current_app = None;
            self.current_folder = None;
            return;
        };
        let same_process = self.current_app.as_ref().is_some_and(|current| {
            current.identity == app.identity
                && current.process_id == app.process_id
                && current.process_created == app.process_created
        });
        if same_process {
            self.sample_current_app(&app, now);
            return;
        }
        let object = self.ensure_app(&app.identity, now);
        let recorded = !self.idle && object.is_some();
        if let (Some(object), true) = (object, recorded) {
            self.record(
                object,
                UsageEventKind::AppForeground,
                ObjectType::Application,
                now,
            );
        }
        // 前台换到新窗口时立即探测一次资源管理器。
        self.last_probe_ms = 0;
        self.current_folder = None;
        self.current_app = Some(CurrentApp {
            identity: app.identity,
            process_id: app.process_id,
            process_created: app.process_created,
            window: app.window,
            is_explorer: app.is_explorer,
            object,
            // 空闲中没有写入时，用户恢复操作后立即补一次活跃采样。
            last_record_ms: if recorded { now } else { 0 },
        });
    }

    fn sample_current_app(&mut self, app: &ForegroundApp, now: i64) {
        let Some(current) = self.current_app.as_mut() else {
            return;
        };
        if current.window != app.window {
            current.window = app.window;
            self.last_probe_ms = 0;
        }
        if self.idle || now - current.last_record_ms < ACTIVE_SAMPLE_MS {
            return;
        }
        current.last_record_ms = now;
        let object = current.object;
        if let Some(object) = object {
            self.record(
                object,
                UsageEventKind::AppActiveSample,
                ObjectType::Application,
                now,
            );
        }
    }

    fn ensure_app(&mut self, identity: &ForegroundIdentity, now: i64) -> Option<ObservableObject> {
        let identity = app_identity(identity)?;
        match self
            .storage
            .call(move |storage| storage.ensure_application(&identity, true, now))
        {
            Ok(object) => object,
            Err(error) => {
                self.on_storage_error(&error);
                None
            }
        }
    }

    /// 写入一条直接观察。代次变化或被排除时作废缓存，下次重新登记。
    fn record(
        &mut self,
        object: ObservableObject,
        kind: UsageEventKind,
        object_type: ObjectType,
        now: i64,
    ) {
        let observation = UsageObservation {
            object_id: object.id,
            kind,
            generation: object.generation,
            observed_at_ms: now,
        };
        match self
            .storage
            .call(move |storage| storage.record_usage(&observation, now_ms().max(now)))
        {
            Ok(_) => {
                self.storage_fault = false;
                self.notifier.objects_changed(object_type.as_str());
            }
            Err(ServiceError::Storage(
                StorageError::ObjectExcluded
                | StorageError::InvalidObservation
                | StorageError::ObjectNotObservable,
            )) => match object_type {
                ObjectType::Application => {
                    if let Some(current) = self.current_app.as_mut() {
                        current.object = None;
                        current.process_id = 0;
                    }
                }
                ObjectType::Folder => self.current_folder = None,
                ObjectType::File => {}
            },
            Err(error) => self.on_storage_error(&error),
        }
    }

    fn probe_explorer(&mut self, now: i64) {
        let window = match self.current_app.as_ref() {
            Some(app) if app.is_explorer && self.active() => app.window,
            _ => {
                self.unsupported_view = false;
                self.current_folder = None;
                return;
            }
        };
        if now - self.last_probe_ms < EXPLORER_PROBE_MS || now < self.explorer_backoff_until {
            return;
        }
        self.last_probe_ms = now;
        let Some(explorer) = self.explorer.as_ref() else {
            return;
        };
        match explorer.probe(window, EXPLORER_TIMEOUT) {
            Ok(ExplorerView::Folder(path)) => {
                self.unsupported_view = false;
                self.observe_folder(&path, now);
            }
            Ok(ExplorerView::Virtual | ExplorerView::NotExplorer) => {
                self.unsupported_view = false;
                self.current_folder = None;
            }
            Ok(ExplorerView::Unsupported) => {
                self.unsupported_view = true;
                self.current_folder = None;
            }
            Err(ProbeError::Busy) => self.explorer_backoff_until = now + EXPLORER_BACKOFF_MS,
            Err(ProbeError::Stopped) => self.explorer = None,
        }
    }

    fn observe_folder(&mut self, path: &str, now: i64) {
        let Ok(path) = normalize_local_path(path) else {
            // 网络路径等不进入长期判断。
            self.current_folder = None;
            return;
        };
        if let Some(current) = self.current_folder.as_mut()
            && current.key == path.key
        {
            if self.idle || now - current.last_record_ms < ACTIVE_SAMPLE_MS {
                return;
            }
            current.last_record_ms = now;
            if let Some(object) = current.object {
                self.record(
                    object,
                    UsageEventKind::FolderVisibleSample,
                    ObjectType::Folder,
                    now,
                );
            }
            return;
        }
        // 范围不包含子目录时，深处的浏览记到范围根下的直接子项。
        let object = self.ensure_in_scope(&path, Some(PathKind::Folder), now);
        let recorded = !self.idle && object.is_some();
        self.current_folder = Some(CurrentFolder {
            key: path.key,
            object,
            last_record_ms: if recorded { now } else { 0 },
        });
        if let (Some(object), true) = (object, recorded) {
            self.record(
                object,
                UsageEventKind::FolderBrowsed,
                ObjectType::Folder,
                now,
            );
        }
    }

    /// 按观察范围找到路径应记到的对象并登记。`kind` 为空时按文件系统判断目标是文件还是目录。
    /// 不在范围内、被排除或这类文件打开记录不到时返回 `None`。
    fn ensure_in_scope(
        &mut self,
        path: &desktrace_core::NormalizedPath,
        kind: Option<PathKind>,
        now: i64,
    ) -> Option<ObservableObject> {
        let lookup = path.clone();
        let found = match self
            .storage
            .call(move |storage| storage.resolve_scope_path(&lookup))
        {
            Ok(Some(found)) => found,
            Ok(None) => return None,
            Err(error) => {
                self.on_storage_error(&error);
                return None;
            }
        };
        // 只有目标就是这条路径本身、且它不是目录时才是文件对象；记到上层时一定是目录。
        let kind = match kind {
            Some(kind) => kind,
            None if found.direct && !Path::new(&found.target.display).is_dir() => PathKind::File,
            None => PathKind::Folder,
        };
        let observable =
            kind == PathKind::Folder || file_open_observable(&display_name(&found.target.display));
        let target = PathTarget {
            kind,
            file_identity: folder_file_identity(&found.target.display),
            path: found.target,
        };
        match self
            .storage
            .call(move |storage| storage.ensure_path_object(&target, observable, true, now))
        {
            Ok(object) => object.filter(|_| observable),
            Err(error) => {
                self.on_storage_error(&error);
                None
            }
        }
    }

    /// “最近使用的项目”登记了一次打开：范围内的文件记到文件本身，目录或更深处的内容记到所在的范围对象。
    fn on_recent_open(&mut self, target: &str, now: i64) {
        if !self.active() {
            return;
        }
        // 与前台切换一样，用户不在场时的登记不算使用。
        self.refresh_idle();
        if self.idle {
            return;
        }
        let Ok(path) = normalize_local_path(target) else {
            return;
        };
        let Some(object) = self.ensure_in_scope(&path, None, now) else {
            return;
        };
        let is_file = self.storage.call(move |storage| {
            Ok(storage
                .object_record(&object.id.to_string())?
                .map(|record| record.object_type)
                == Some(ObjectType::File))
        });
        let (kind, object_type) = match is_file {
            Ok(true) => (UsageEventKind::FileOpened, ObjectType::File),
            Ok(false) => (UsageEventKind::FolderContentOpened, ObjectType::Folder),
            Err(error) => {
                self.on_storage_error(&error);
                return;
            }
        };
        self.record(object, kind, object_type, now);
    }

    /// 登记所有启用范围根目录下的直接子项。只为新出现的路径读取文件 ID，避免每次扫描打开全部文件。
    fn scan_scopes(&mut self, now: i64) {
        let known = self
            .storage
            .call(|storage| Ok((storage.folder_scopes()?, storage.path_identifier_keys()?)));
        let (scopes, known_keys) = match known {
            Ok(result) => result,
            Err(error) => {
                self.on_storage_error(&error);
                return;
            }
        };
        let mut items = Vec::new();
        for scope in scopes.iter().filter(|scope| scope.enabled) {
            // 范围根暂时无法访问（外置盘未连接等）时跳过，由可访问性检查标记。
            let Ok(entries) = list_directory(&scope.path) else {
                continue;
            };
            for entry in entries {
                let Ok(path) = normalize_local_path(&entry.path) else {
                    continue;
                };
                let kind = if entry.is_directory {
                    PathKind::Folder
                } else {
                    PathKind::File
                };
                let observable =
                    entry.is_directory || file_open_observable(&display_name(&path.display));
                let file_identity = if known_keys.contains(&path.key) {
                    None
                } else {
                    folder_file_identity(&path.display)
                };
                items.push(ScopeItem {
                    kind,
                    path,
                    file_identity,
                    observable,
                });
            }
        }
        if items.is_empty() {
            return;
        }
        let running = self.collection == CollectionState::Running;
        match self
            .storage
            .call(move |storage| storage.sync_scope_items(&items, running, now))
        {
            Ok(_) => {
                self.notifier.objects_changed(ObjectType::Folder.as_str());
                self.notifier.objects_changed(ObjectType::File.as_str());
            }
            Err(error) => self.on_storage_error(&error),
        }
    }

    fn maintenance(&mut self, now: i64) {
        if now - self.last_purge_ms >= PURGE_INTERVAL_MS {
            self.last_purge_ms = now;
            let result = self.storage.call(move |storage| {
                let retention = storage.settings()?.event_retention_days;
                storage.purge_expired_events(retention, now)
            });
            if let Err(error) = result {
                self.on_storage_error(&error);
            }
        }
        if now - self.last_availability_ms >= AVAILABILITY_INTERVAL_MS {
            self.last_availability_ms = now;
            self.scan_scopes(now);
            self.refresh_path_availability(now);
        }
        if now - self.last_catalog_ms >= CATALOG_INTERVAL_MS {
            self.last_catalog_ms = now;
            (self.on_catalog_due)();
        }
    }

    /// 检查文件与目录对象是否仍可访问：盘符不在为暂不可访问，盘符在但对象不在为已失联。
    fn refresh_path_availability(&mut self, now: i64) {
        let records = self.storage.call(|storage| {
            let mut records = storage.object_records(ObjectType::Folder)?;
            records.extend(storage.object_records(ObjectType::File)?);
            Ok(records)
        });
        let Ok(records) = records else {
            return;
        };
        let mut changes = Vec::new();
        for record in records {
            let Some(path) = record.current_path.as_deref() else {
                continue;
            };
            let drive = path.get(0..3).unwrap_or_default();
            let present = match record.object_type {
                ObjectType::File => Path::new(path).is_file(),
                _ => Path::new(path).is_dir(),
            };
            let availability = if present {
                Availability::Available
            } else if Path::new(drive).exists() {
                Availability::Removed
            } else {
                Availability::Unavailable
            };
            if availability != record.availability {
                changes.push((record.id, availability));
            }
        }
        if changes.is_empty() {
            return;
        }
        let result = self.storage.call(move |storage| {
            for (id, availability) in &changes {
                storage.set_availability(id, *availability, now)?;
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                self.notifier.objects_changed(ObjectType::Folder.as_str());
                self.notifier.objects_changed(ObjectType::File.as_str());
            }
            Err(error) => self.on_storage_error(&error),
        }
    }

    fn shutdown(&mut self) {
        if self.collection == CollectionState::NotStarted {
            return;
        }
        let now = now_ms();
        let _ = self.storage.call(move |storage| {
            for module in CollectorModule::ALL {
                storage.heartbeat(module, now)?;
                storage.transition_interval(module, IntervalReason::CollectorStopped, now)?;
            }
            Ok(())
        });
        self.watcher = None;
    }
}

/// 把前台身份转换为存储层身份；网络路径等不受支持的位置返回 `None`。
fn app_identity(identity: &ForegroundIdentity) -> Option<AppIdentity> {
    match identity {
        ForegroundIdentity::Package { aumid } => Some(AppIdentity::Package {
            display_name: aumid.split(['_', '!']).next().unwrap_or(aumid).to_string(),
            aumid: aumid.clone(),
        }),
        ForegroundIdentity::Executable { path } => {
            let normalized = normalize_local_path(path).ok()?;
            let display_name = Path::new(&normalized.display)
                .file_stem()?
                .to_string_lossy()
                .into_owned();
            Some(AppIdentity::Executable {
                path: normalized,
                display_name,
            })
        }
    }
}
