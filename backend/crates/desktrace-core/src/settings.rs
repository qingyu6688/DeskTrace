//! 白名单设置及取值范围，与接口设计文档第 4 节一致。

use serde::Serialize;
use thiserror::Error;

/// 默认长期阈值。
pub const DEFAULT_THRESHOLD_DAYS: u32 = 90;
/// 默认原始事件保留天数。
pub const DEFAULT_RETENTION_DAYS: u32 = 180;
/// 阈值允许范围。
pub const THRESHOLD_RANGE: std::ops::RangeInclusive<u32> = 1..=3650;
/// 保留天数允许范围。
pub const RETENTION_RANGE: std::ops::RangeInclusive<u32> = 30..=3650;

/// 采集的用户意图。从未启用和暂停要区分：前者没有观察起点，后者形成缺口。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionState {
    /// 尚未完成首次启用。
    NotStarted,
    /// 正在记录。
    Running,
    /// 用户暂停。
    Paused,
}

impl CollectionState {
    /// 返回持久化的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Running => "running",
            Self::Paused => "paused",
        }
    }

    /// 解析持久化值，未知值按尚未启用处理，避免误开始记录。
    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "paused" => Self::Paused,
            _ => Self::NotStarted,
        }
    }
}

/// 设置校验错误，文案直接展示给用户。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    /// 阈值越界。
    #[error("阈值必须为 1～3650 天的整数")]
    Threshold,
    /// 保留天数越界。
    #[error("保留天数必须为 30～3650 天的整数")]
    Retention,
}

/// 完整设置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 长期阈值天数。
    pub threshold_days: u32,
    /// 原始事件保留天数。
    pub event_retention_days: u32,
    /// 是否开机启动。
    pub auto_start_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threshold_days: DEFAULT_THRESHOLD_DAYS,
            event_retention_days: DEFAULT_RETENTION_DAYS,
            auto_start_enabled: false,
        }
    }
}

/// 设置补丁，未出现的字段保持原值。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettingsPatch {
    /// 新阈值。
    pub threshold_days: Option<u32>,
    /// 新保留天数。
    pub event_retention_days: Option<u32>,
    /// 开机启动开关。
    pub auto_start_enabled: Option<bool>,
}

/// 校验阈值。
pub fn validate_threshold(days: u32) -> Result<u32, SettingsError> {
    THRESHOLD_RANGE
        .contains(&days)
        .then_some(days)
        .ok_or(SettingsError::Threshold)
}

impl Settings {
    /// 应用补丁并校验，任一字段非法时整体不生效。
    pub fn apply(self, patch: SettingsPatch) -> Result<Self, SettingsError> {
        let threshold_days =
            validate_threshold(patch.threshold_days.unwrap_or(self.threshold_days))?;
        let event_retention_days = patch
            .event_retention_days
            .unwrap_or(self.event_retention_days);
        if !RETENTION_RANGE.contains(&event_retention_days) {
            return Err(SettingsError::Retention);
        }
        Ok(Self {
            threshold_days,
            event_retention_days,
            auto_start_enabled: patch.auto_start_enabled.unwrap_or(self.auto_start_enabled),
        })
    }
}
