//! 与桌面框架无关的对象、事件、采集区间与候选规则。

mod candidate;
mod install_date;
mod intervals;
mod paths;
mod settings;
mod time;
mod track;
mod usage;

pub use candidate::{
    Availability, CandidateInput, CandidateResult, CandidateStatus, CoverageStatus,
    HEARTBEAT_TOLERANCE_MS, IntervalSpan, ObservationCapability, StatusReason, classify, coverage,
    coverage_holes,
};
pub use install_date::{
    DateMeaning, DatePrecision, DateSource, DateValidation, InstallDateEvidence,
    package_install_date, parse_registry_install_date,
};
pub use intervals::{CollectorModule, IntervalReason, IntervalState};
pub use paths::{
    NormalizedPath, PathError, ScopeMatch, ScopeRule, display_name, file_open_observable,
    is_descendant, is_same_or_descendant, normalize_local_path, resolve_in_scopes, top_level_child,
};
pub use settings::{
    CollectionState, DEFAULT_RETENTION_DAYS, DEFAULT_THRESHOLD_DAYS, RETENTION_RANGE, Settings,
    SettingsError, SettingsPatch, THRESHOLD_RANGE, validate_threshold,
};
pub use time::{
    DAY_MS, civil_from_days, days_from_civil, format_date, format_utc, format_with_offset,
    is_valid_date, local_day, parse_date,
};
pub use track::{TRACK_BUCKET_MS, local_usage_days};
pub use usage::{DEDUP_WINDOW_MS, ObjectType, RuleError, UsageEventKind, UsageObservation};
