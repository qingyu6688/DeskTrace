//! 长期候选判定与记录覆盖计算，对应详细设计第 6、7 节。

use serde::Serialize;

use crate::{DAY_MS, IntervalState, ObjectType};

/// 开放区间的心跳每 15 秒推进一次，允许三次心跳的延迟仍视为已验证。
pub const HEARTBEAT_TOLERANCE_MS: i64 = 45_000;

/// 对象当前的可访问状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// 可正常访问。
    Available,
    /// 本轮无法访问，例如外置盘未连接。
    Unavailable,
    /// 完整扫描确认已不存在。
    Removed,
    /// 尚未确认。
    Unknown,
}

impl Availability {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Removed => "removed",
            Self::Unknown => "unknown",
        }
    }

    /// 从数据库值解析，未知值按 `Unknown` 处理。
    pub fn parse(value: &str) -> Self {
        match value {
            "available" => Self::Available,
            "unavailable" => Self::Unavailable,
            "removed" => Self::Removed,
            _ => Self::Unknown,
        }
    }
}

/// 对象能否被当前采集来源直接观察。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationCapability {
    /// 来源支持，且有可验证入口。
    Supported,
    /// 只有清单身份，找不到可匹配的启动入口。
    Unresolved,
    /// 来源不支持，例如目录已移出观察范围。
    Unsupported,
}

impl ObservationCapability {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unresolved => "unresolved",
            Self::Unsupported => "unsupported",
        }
    }

    /// 从数据库值解析，未知值按 `Unsupported` 处理，避免误判为可观察。
    pub fn parse(value: &str) -> Self {
        match value {
            "supported" => Self::Supported,
            "unresolved" => Self::Unresolved,
            _ => Self::Unsupported,
        }
    }
}

/// 七种候选状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    /// 阈值内有直接记录。
    Recent,
    /// 观察期足够，最后一条直接记录早于阈值。
    StaleCandidate,
    /// 观察期足够，没有直接记录。
    NeverObserved,
    /// 观察期不足阈值。
    InsufficientObservation,
    /// 身份、来源或时间不足以判断。
    Unknown,
    /// 用户明确排除。
    Excluded,
    /// 已失联或暂不可访问。
    Unavailable,
}

impl CandidateStatus {
    /// 全部状态，顺序与界面筛选一致。
    pub const ALL: [Self; 7] = [
        Self::Recent,
        Self::StaleCandidate,
        Self::NeverObserved,
        Self::InsufficientObservation,
        Self::Unknown,
        Self::Excluded,
        Self::Unavailable,
    ];

    /// 返回 IPC 使用的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::StaleCandidate => "stale_candidate",
            Self::NeverObserved => "never_observed",
            Self::InsufficientObservation => "insufficient_observation",
            Self::Unknown => "unknown",
            Self::Excluded => "excluded",
            Self::Unavailable => "unavailable",
        }
    }

    /// 解析 IPC 输入，未知值返回 `None` 由调用方报 400。
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    /// 只有两类整理候选需要展示记录覆盖。
    pub fn shows_coverage(self) -> bool {
        matches!(self, Self::StaleCandidate | Self::NeverObserved)
    }
}

/// 观察窗口内的记录覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    /// 声明支持范围内没有检测到缺口。
    Continuous,
    /// 存在已知暂停或故障。
    Partial,
    /// 存在无法验证的区间。
    Unknown,
}

impl CoverageStatus {
    /// 返回 IPC 使用的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }

    /// 解析 IPC 输入。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "continuous" => Some(Self::Continuous),
            "partial" => Some(Self::Partial),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// 判定结果的原因，用于生成可展示的中文说明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusReason {
    /// 用户排除。
    Excluded,
    /// 完整扫描确认已不存在。
    Removed,
    /// 本轮无法访问。
    Inaccessible,
    /// 可用状态尚未确认。
    AvailabilityUnknown,
    /// 清单项没有可匹配的启动入口。
    NoMatchedEntry,
    /// 可执行文件、快捷方式等文件的打开不会登记到“最近使用的项目”。
    UnobservableFileType,
    /// 系统当前关闭了“最近使用的项目”，文件打开无从记录。
    FileTrackingDisabled,
    /// 来源不支持或已移出观察范围。
    SourceUnsupported,
    /// 尚未开始有效观察。
    NotObservedYet,
    /// 时间戳晚于当前时刻等时钟异常。
    ClockAnomaly,
    /// 阈值内有记录。
    RecentUse,
    /// 观察期不足，附还需天数。
    NeedsMoreDays(u32),
    /// 观察期内没有直接记录。
    NoDirectRecord,
    /// 最近记录早于阈值，附超出天数。
    OlderThanThreshold(u32),
}

impl StatusReason {
    /// 返回界面展示的中文说明，不含路径和原始系统信息。
    pub fn label(self, threshold_days: u32) -> String {
        match self {
            Self::Excluded => "已排除，长期保留".into(),
            Self::Removed => "连续两次完整扫描未找到".into(),
            Self::Inaccessible => "暂时无法访问".into(),
            Self::AvailabilityUnknown => "可访问状态尚未确认".into(),
            Self::NoMatchedEntry => "没有可匹配的启动入口".into(),
            Self::UnobservableFileType => "这类文件的打开无法记录".into(),
            Self::FileTrackingDisabled => "系统关闭了“最近使用的项目”，无法记录文件打开".into(),
            Self::SourceUnsupported => "不在当前观察范围".into(),
            Self::NotObservedYet => "尚未开始记录".into(),
            Self::ClockAnomaly => "时间记录异常，暂时无法判断".into(),
            Self::RecentUse => format!("{threshold_days} 天内有使用记录"),
            Self::NeedsMoreDays(days) => format!("还需观察 {days} 天"),
            Self::NoDirectRecord => "观察期内未记录到使用".into(),
            Self::OlderThanThreshold(days) => {
                format!("最近记录早于 {threshold_days} 天阈值 {days} 天")
            }
        }
    }
}

/// 候选判定输入。安装日期与历史线索不参与判定，因此不在输入中。
#[derive(Clone, Copy, Debug)]
pub struct CandidateInput {
    /// 对象类型，只影响“无法判断”的说明文字。
    pub object_type: ObjectType,
    /// 是否命中排除规则。
    pub excluded: bool,
    /// 可访问状态。
    pub availability: Availability,
    /// 观察能力。
    pub capability: ObservationCapability,
    /// 当前代次观察起点。
    pub observation_started_at_ms: Option<i64>,
    /// 最近直接观察时间，可能来自旧代次。
    pub last_used_at_ms: Option<i64>,
    /// 查询时刻。
    pub now_ms: i64,
    /// 阈值天数。
    pub threshold_days: u32,
}

/// 候选判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateResult {
    /// 状态。
    pub status: CandidateStatus,
    /// 原因。
    pub reason: StatusReason,
}

impl CandidateResult {
    fn new(status: CandidateStatus, reason: StatusReason) -> Self {
        Self { status, reason }
    }
}

/// 按详细设计第 6 节的顺序判定候选状态。
pub fn classify(input: &CandidateInput) -> CandidateResult {
    use CandidateStatus as S;
    if input.excluded {
        return CandidateResult::new(S::Excluded, StatusReason::Excluded);
    }
    match input.availability {
        Availability::Removed => {
            return CandidateResult::new(S::Unavailable, StatusReason::Removed);
        }
        Availability::Unavailable => {
            return CandidateResult::new(S::Unavailable, StatusReason::Inaccessible);
        }
        Availability::Unknown => {
            return CandidateResult::new(S::Unknown, StatusReason::AvailabilityUnknown);
        }
        Availability::Available => {}
    }
    match input.capability {
        ObservationCapability::Unresolved => {
            let reason = if input.object_type == ObjectType::File {
                StatusReason::UnobservableFileType
            } else {
                StatusReason::NoMatchedEntry
            };
            return CandidateResult::new(S::Unknown, reason);
        }
        ObservationCapability::Unsupported => {
            return CandidateResult::new(S::Unknown, StatusReason::SourceUnsupported);
        }
        ObservationCapability::Supported => {}
    }
    let Some(started) = input.observation_started_at_ms else {
        return CandidateResult::new(S::Unknown, StatusReason::NotObservedYet);
    };
    // 起点或记录晚于当前时刻说明时钟异常，先报无法判断，不直接计算候选。
    if started > input.now_ms
        || input
            .last_used_at_ms
            .is_some_and(|last| last > input.now_ms)
    {
        return CandidateResult::new(S::Unknown, StatusReason::ClockAnomaly);
    }
    let cutoff = input.now_ms - i64::from(input.threshold_days) * DAY_MS;
    if let Some(last) = input.last_used_at_ms
        && last >= cutoff
    {
        return CandidateResult::new(S::Recent, StatusReason::RecentUse);
    }
    if started > cutoff {
        let remaining = ceil_days(started - cutoff);
        return CandidateResult::new(
            S::InsufficientObservation,
            StatusReason::NeedsMoreDays(remaining),
        );
    }
    match input.last_used_at_ms {
        None => CandidateResult::new(S::NeverObserved, StatusReason::NoDirectRecord),
        Some(last) => CandidateResult::new(
            S::StaleCandidate,
            StatusReason::OlderThanThreshold(floor_days(cutoff - last)),
        ),
    }
}

fn ceil_days(ms: i64) -> u32 {
    u32::try_from((ms + DAY_MS - 1).div_euclid(DAY_MS)).unwrap_or(u32::MAX)
}

fn floor_days(ms: i64) -> u32 {
    u32::try_from(ms.div_euclid(DAY_MS)).unwrap_or(u32::MAX)
}

/// 参与覆盖计算的一段采集区间。
#[derive(Clone, Copy, Debug)]
pub struct IntervalSpan {
    /// 区间状态。
    pub state: IntervalState,
    /// 开始时刻。
    pub started_at_ms: i64,
    /// 已验证到的时刻。
    pub verified_until_ms: i64,
    /// 结束时刻，开放区间为空。
    pub ended_at_ms: Option<i64>,
}

impl IntervalSpan {
    /// 区间有证据覆盖到的结束时刻。缺口和未知区间本身就是对整段时间的声明，按结束时刻（进行中到当前）；
    /// 采集与非活跃区间只认到已验证时刻，开放区间的最近心跳在容差内时视为到当前。
    fn evidence_end(&self, now_ms: i64) -> i64 {
        match (self.state, self.ended_at_ms) {
            (IntervalState::Gap | IntervalState::Unknown, Some(end)) => end,
            (IntervalState::Gap | IntervalState::Unknown, None) => now_ms,
            (_, Some(_)) => self.verified_until_ms,
            (_, None) if now_ms - self.verified_until_ms <= HEARTBEAT_TOLERANCE_MS => now_ms,
            (_, None) => self.verified_until_ms,
        }
    }
}

/// 计算 `[window_start, window_end]` 内的覆盖。`unknown` 优先于 `gap`；
/// 没有任何区间证据的空档同样按未知处理。
pub fn coverage(
    window_start_ms: i64,
    window_end_ms: i64,
    intervals: &[IntervalSpan],
) -> CoverageStatus {
    if window_end_ms <= window_start_ms {
        return CoverageStatus::Unknown;
    }
    let mut relevant: Vec<(i64, i64, IntervalState)> = intervals
        .iter()
        .filter_map(|span| {
            let start = span.started_at_ms.max(window_start_ms);
            let end = span.evidence_end(window_end_ms).min(window_end_ms);
            (end > start).then_some((start, end, span.state))
        })
        .collect();
    if relevant
        .iter()
        .any(|(_, _, state)| *state == IntervalState::Unknown)
    {
        return CoverageStatus::Unknown;
    }
    let has_gap = relevant
        .iter()
        .any(|(_, _, state)| *state == IntervalState::Gap);
    relevant.sort_by_key(|(start, _, _)| *start);
    let mut covered_until = window_start_ms;
    for (start, end, _) in &relevant {
        if *start > covered_until {
            return CoverageStatus::Unknown;
        }
        covered_until = covered_until.max(*end);
    }
    if covered_until < window_end_ms {
        CoverageStatus::Unknown
    } else if has_gap {
        CoverageStatus::Partial
    } else {
        CoverageStatus::Continuous
    }
}

/// 列出窗口内没有任何区间证据的空档，用于在轨迹上标出未验证时间。
pub fn coverage_holes(
    window_start_ms: i64,
    window_end_ms: i64,
    intervals: &[IntervalSpan],
) -> Vec<(i64, i64)> {
    let mut ranges: Vec<(i64, i64)> = intervals
        .iter()
        .filter_map(|span| {
            let start = span.started_at_ms.max(window_start_ms);
            let end = span.evidence_end(window_end_ms).min(window_end_ms);
            (end > start).then_some((start, end))
        })
        .collect();
    ranges.sort_unstable();
    let mut holes = Vec::new();
    let mut covered_until = window_start_ms;
    for (start, end) in ranges {
        if start > covered_until {
            holes.push((covered_until, start));
        }
        covered_until = covered_until.max(end);
    }
    if covered_until < window_end_ms {
        holes.push((covered_until, window_end_ms));
    }
    holes
}
