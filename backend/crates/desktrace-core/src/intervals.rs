use serde::Serialize;

/// 一个模块在区间内的证据状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalState {
    /// 来源健康且正在采集。
    Active,
    /// 有证据确认处于本口径下的非活跃期。
    Inactive,
    /// 确认存在采集或持久化缺口。
    Gap,
    /// 无法验证区间。
    Unknown,
}

impl IntervalState {
    /// 返回数据库中的固定状态值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Gap => "gap",
            Self::Unknown => "unknown",
        }
    }
}

/// 采集区间主因，每个原因只对应一个合法状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalReason {
    /// 首次启用且模块就绪。
    CollectionStarted,
    /// 恢复观察且模块就绪。
    CollectionResumed,
    /// 故障恢复并通过健康检查。
    CollectorRecovered,
    /// 当前用户会话已锁定。
    SessionLocked,
    /// 系统已确认休眠。
    SystemSuspended,
    /// 当前用户会话空闲。
    UserIdle,
    /// 用户主动暂停。
    UserPaused,
    /// 采集模块故障。
    CollectorFault,
    /// 必要来源权限不足。
    PermissionDenied,
    /// 活动视图无法可靠解析。
    UnsupportedView,
    /// 事件队列溢出造成丢失。
    QueueOverflow,
    /// 数据无法可靠持久化。
    StorageError,
    /// 系统关闭了所需来源，例如“显示最近打开的项目”。
    SourceDisabled,
    /// 启动前区间未验证。
    StartupUnverified,
    /// 上次运行异常退出。
    UnexpectedExit,
    /// 明确退出后的区间。
    CollectorStopped,
    /// 墙上时间发生异常变化。
    ClockChange,
}

impl IntervalReason {
    /// 所有原因的完整集合，供状态约束与界面元信息使用。
    pub const ALL: [Self; 17] = [
        Self::CollectionStarted,
        Self::CollectionResumed,
        Self::CollectorRecovered,
        Self::SessionLocked,
        Self::SystemSuspended,
        Self::UserIdle,
        Self::UserPaused,
        Self::CollectorFault,
        Self::PermissionDenied,
        Self::UnsupportedView,
        Self::QueueOverflow,
        Self::StorageError,
        Self::SourceDisabled,
        Self::StartupUnverified,
        Self::UnexpectedExit,
        Self::CollectorStopped,
        Self::ClockChange,
    ];

    /// 返回原因对应的唯一合法状态。
    pub fn state(self) -> IntervalState {
        match self {
            Self::CollectionStarted | Self::CollectionResumed | Self::CollectorRecovered => {
                IntervalState::Active
            }
            Self::SessionLocked | Self::SystemSuspended | Self::UserIdle => IntervalState::Inactive,
            Self::UserPaused
            | Self::CollectorFault
            | Self::PermissionDenied
            | Self::UnsupportedView
            | Self::QueueOverflow
            | Self::StorageError
            | Self::SourceDisabled => IntervalState::Gap,
            Self::StartupUnverified
            | Self::UnexpectedExit
            | Self::CollectorStopped
            | Self::ClockChange => IntervalState::Unknown,
        }
    }

    /// 返回数据库中的原因值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CollectionStarted => "collection_started",
            Self::CollectionResumed => "collection_resumed",
            Self::CollectorRecovered => "collector_recovered",
            Self::SessionLocked => "session_locked",
            Self::SystemSuspended => "system_suspended",
            Self::UserIdle => "user_idle",
            Self::UserPaused => "user_paused",
            Self::CollectorFault => "collector_fault",
            Self::PermissionDenied => "permission_denied",
            Self::UnsupportedView => "unsupported_view",
            Self::QueueOverflow => "queue_overflow",
            Self::StorageError => "storage_error",
            Self::SourceDisabled => "source_disabled",
            Self::StartupUnverified => "startup_unverified",
            Self::UnexpectedExit => "unexpected_exit",
            Self::CollectorStopped => "collector_stopped",
            Self::ClockChange => "clock_change",
        }
    }
}

impl IntervalState {
    /// 从数据库值解析，未知值按 `Unknown` 处理。
    pub fn parse(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "inactive" => Self::Inactive,
            "gap" => Self::Gap,
            _ => Self::Unknown,
        }
    }
}

impl IntervalReason {
    /// 从数据库值解析。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }

    /// 返回界面展示的中文说明。
    pub fn label(self) -> &'static str {
        match self {
            Self::CollectionStarted => "开始记录",
            Self::CollectionResumed => "恢复记录",
            Self::CollectorRecovered => "故障已恢复",
            Self::SessionLocked => "会话已锁定",
            Self::SystemSuspended => "系统休眠",
            Self::UserIdle => "长时间无操作",
            Self::UserPaused => "手动暂停",
            Self::CollectorFault => "采集模块故障",
            Self::PermissionDenied => "权限不足",
            Self::UnsupportedView => "无法识别当前视图",
            Self::QueueOverflow => "事件过多已丢失",
            Self::StorageError => "无法保存记录",
            Self::SourceDisabled => "系统关闭了最近使用的项目",
            Self::StartupUnverified => "启动前状态未知",
            Self::UnexpectedExit => "程序未正常退出",
            Self::CollectorStopped => "程序已退出",
            Self::ClockChange => "系统时间变化",
        }
    }
}

/// 分别保存区间的两个采集模块。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorModule {
    /// 应用前台使用。
    Applications,
    /// 资源管理器文件夹浏览。
    Folders,
    /// “最近使用的项目”中的文件打开。
    Files,
}

impl CollectorModule {
    /// 两个模块。
    pub const ALL: [Self; 3] = [Self::Applications, Self::Folders, Self::Files];

    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applications => "applications",
            Self::Folders => "folders",
            Self::Files => "files",
        }
    }

    /// 从数据库值解析。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|module| module.as_str() == value)
    }

    /// 对象类型对应的模块。
    pub fn for_object(object_type: crate::ObjectType) -> Self {
        match object_type {
            crate::ObjectType::Application => Self::Applications,
            crate::ObjectType::Folder => Self::Folders,
            crate::ObjectType::File => Self::Files,
        }
    }
}
