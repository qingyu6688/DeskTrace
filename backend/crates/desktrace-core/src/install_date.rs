//! 应用安装日期证据的解析与含义标注，对应详细设计第 5 节。

use serde::Serialize;

use crate::time::{days_from_civil, format_date, format_utc, is_valid_date};

/// 日期证据来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    /// 卸载注册表项的 `InstallDate`。
    UninstallRegistry,
    /// 当前用户包管理接口的 `InstalledDate`。
    PackageApi,
}

impl DateSource {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UninstallRegistry => "uninstall_registry",
            Self::PackageApi => "package_api",
        }
    }
}

/// 日期精度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatePrecision {
    /// 只到日。
    Day,
    /// 精确时刻。
    Instant,
    /// 无有效值。
    Unknown,
}

impl DatePrecision {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Instant => "instant",
            Self::Unknown => "unknown",
        }
    }
}

/// 日期含义；系统给出的日期可能是安装、更新或修复时间。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DateMeaning {
    /// MSI 安装或维护日期。
    InstallOrService,
    /// 包安装或最后更新日期。
    InstallOrUpdate,
    /// 无法确认提供方语义的系统登记日期。
    Registered,
    /// 无有效值。
    Unknown,
}

impl DateMeaning {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstallOrService => "install_or_service",
            Self::InstallOrUpdate => "install_or_update",
            Self::Registered => "registered",
            Self::Unknown => "unknown",
        }
    }
}

/// 日期校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DateValidation {
    /// 合法。
    Valid,
    /// 缺失。
    Missing,
    /// 格式或日期非法。
    Invalid,
    /// 晚于当前日期。
    Future,
}

impl DateValidation {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Future => "future",
        }
    }
}

/// 一条待保存的日期证据。无效值的 `date_value` 一律为空，不回填任何替代时间。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallDateEvidence {
    /// 来源。
    pub source: DateSource,
    /// 原始值，供诊断。
    pub raw_value: Option<String>,
    /// 日期级为 `YYYY-MM-DD`，时刻级为 UTC RFC 3339。
    pub date_value: Option<String>,
    /// 精度。
    pub precision: DatePrecision,
    /// 含义。
    pub meaning: DateMeaning,
    /// 校验结果。
    pub validation: DateValidation,
}

impl InstallDateEvidence {
    fn invalid(source: DateSource, raw: Option<String>, validation: DateValidation) -> Self {
        Self {
            source,
            raw_value: raw,
            date_value: None,
            precision: DatePrecision::Unknown,
            meaning: DateMeaning::Unknown,
            validation,
        }
    }
}

/// 解析卸载项 `InstallDate`。只接受严格的 `yyyyMMdd`；
/// `is_msi` 来自 `WindowsInstaller=1`，决定标为安装/维护日期还是系统登记日期。
/// `today_local_days` 是本地今天的天数，晚于今天的日期标为未来值。
pub fn parse_registry_install_date(
    raw: Option<&str>,
    is_msi: bool,
    today_local_days: i64,
) -> InstallDateEvidence {
    let source = DateSource::UninstallRegistry;
    let Some(text) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return InstallDateEvidence::invalid(source, None, DateValidation::Missing);
    };
    let raw_value = Some(text.to_string());
    let parsed = (text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| {
            let year = text.get(0..4)?.parse::<i32>().ok()?;
            let month = text.get(4..6)?.parse::<u32>().ok()?;
            let day = text.get(6..8)?.parse::<u32>().ok()?;
            is_valid_date(year, month, day).then(|| days_from_civil(year, month, day))
        })
        .flatten();
    let Some(days) = parsed else {
        return InstallDateEvidence::invalid(source, raw_value, DateValidation::Invalid);
    };
    if days > today_local_days {
        return InstallDateEvidence::invalid(source, raw_value, DateValidation::Future);
    }
    InstallDateEvidence {
        source,
        raw_value,
        date_value: Some(format_date(days)),
        precision: DatePrecision::Day,
        meaning: if is_msi {
            DateMeaning::InstallOrService
        } else {
            DateMeaning::Registered
        },
        validation: DateValidation::Valid,
    }
}

/// 解析包管理接口的安装时刻（UTC 毫秒）。允许 5 分钟时钟误差，再晚算未来值。
pub fn package_install_date(installed_at_ms: Option<i64>, now_ms: i64) -> InstallDateEvidence {
    const CLOCK_SKEW_MS: i64 = 5 * 60_000;
    let source = DateSource::PackageApi;
    let Some(ms) = installed_at_ms.filter(|value| *value > 0) else {
        return InstallDateEvidence::invalid(source, None, DateValidation::Missing);
    };
    let raw_value = Some(ms.to_string());
    if ms > now_ms + CLOCK_SKEW_MS {
        return InstallDateEvidence::invalid(source, raw_value, DateValidation::Future);
    }
    InstallDateEvidence {
        source,
        raw_value,
        date_value: Some(format_utc(ms)),
        precision: DatePrecision::Instant,
        meaning: DateMeaning::InstallOrUpdate,
        validation: DateValidation::Valid,
    }
}
