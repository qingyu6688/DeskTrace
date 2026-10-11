use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

/// 同类观察按 UTC 分钟聚合，不能据此统计精确打开次数。
pub const DEDUP_WINDOW_MS: i64 = 60_000;

/// 核心规则校验错误。
#[derive(Debug, Error)]
pub enum RuleError {
    /// 输入的 UTC 时间无法参与非负分钟桶计算。
    #[error("观察时间必须是非负的 UTC Unix 毫秒")]
    InvalidTimestamp,
    /// 清除和恢复观察后必须使用有效代次。
    #[error("观察代次必须大于零")]
    InvalidGeneration,
}

/// 可观察对象的业务类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectType {
    /// 本地文件系统目录。
    Folder,
    /// 已识别的应用入口。
    Application,
    /// 观察范围内的单个文件，例如桌面上的文档。
    File,
}

impl ObjectType {
    /// 返回持久化所用的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Application => "application",
            Self::File => "file",
        }
    }

    /// 解析持久化值或 IPC 输入。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "application" => Some(Self::Application),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

/// 直接观察到的使用信号，不表示精确启动次数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageEventKind {
    /// 应用进入前台。
    AppForeground,
    /// 前台应用处于活跃使用状态。
    AppActiveSample,
    /// 活动文件夹发生切换或恢复观察。
    FolderBrowsed,
    /// 文件夹持续处于活动视图。
    FolderVisibleSample,
    /// 系统“最近使用的项目”登记了范围内文件被打开。
    FileOpened,
    /// 系统“最近使用的项目”登记了目录内的内容被打开，记到所在的范围内目录（归入文件夹模块）。
    FolderContentOpened,
}

impl UsageEventKind {
    /// 返回持久化的事件名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppForeground => "app_foreground",
            Self::AppActiveSample => "app_active_sample",
            Self::FolderBrowsed => "folder_browsed",
            Self::FolderVisibleSample => "folder_visible_sample",
            Self::FileOpened => "file_opened",
            Self::FolderContentOpened => "folder_content_opened",
        }
    }

    /// 来源由类型导出，避免输入制造不合法的类型与来源组合。
    pub fn source(self) -> &'static str {
        match self {
            Self::AppForeground => "foreground_hook",
            Self::AppActiveSample => "foreground_sample",
            Self::FolderBrowsed | Self::FolderVisibleSample => "explorer_shell",
            Self::FileOpened | Self::FolderContentOpened => "recent_items",
        }
    }

    /// 返回该信号可以更新的对象类型。
    pub fn object_type(self) -> ObjectType {
        match self {
            Self::AppForeground | Self::AppActiveSample => ObjectType::Application,
            Self::FolderBrowsed | Self::FolderVisibleSample | Self::FolderContentOpened => {
                ObjectType::Folder
            }
            Self::FileOpened => ObjectType::File,
        }
    }
}

/// 采集器送入存储层的原始观察时间与代次。
#[derive(Clone, Debug)]
pub struct UsageObservation {
    /// 已完成身份解析的对象 UUID。
    pub object_id: Uuid,
    /// 固定的直接事件类型。
    pub kind: UsageEventKind,
    /// 当前对象的观察代次。
    pub generation: u32,
    /// 观察发生时的 UTC Unix 毫秒，重试不得改写。
    pub observed_at_ms: i64,
}

impl UsageObservation {
    /// 校验与分钟桶有关的基本输入。
    pub fn validate(&self) -> Result<(), RuleError> {
        if self.observed_at_ms < 0 {
            return Err(RuleError::InvalidTimestamp);
        }
        if self.generation == 0 {
            return Err(RuleError::InvalidGeneration);
        }
        Ok(())
    }

    /// 生成版本化幂等键，覆盖对象、类型、代次、来源与 UTC 分钟。
    pub fn dedup_key(&self) -> Result<String, RuleError> {
        self.validate()?;
        Ok(format!(
            "v1:{}:{}:{}:{}:{}",
            self.object_id.hyphenated(),
            self.kind.as_str(),
            self.generation,
            self.kind.source(),
            self.observed_at_ms / DEDUP_WINDOW_MS
        ))
    }
}
