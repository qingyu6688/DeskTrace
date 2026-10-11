use desktrace_core::{
    Availability, CandidateInput, CandidateStatus, CoverageStatus, DAY_MS, IntervalSpan,
    IntervalState, ObjectType, ObservationCapability, StatusReason, classify, coverage,
    days_from_civil,
};

const NOW: i64 = 1_791_684_000_000;

fn input(started_days_ago: Option<i64>, last_days_ago: Option<i64>) -> CandidateInput {
    CandidateInput {
        object_type: ObjectType::Application,
        excluded: false,
        availability: Availability::Available,
        capability: ObservationCapability::Supported,
        observation_started_at_ms: started_days_ago.map(|days| NOW - days * DAY_MS),
        last_used_at_ms: last_days_ago.map(|days| NOW - days * DAY_MS),
        now_ms: NOW,
        threshold_days: 90,
    }
}

#[test]
fn design_examples_match_expected_status() {
    assert_eq!(
        classify(&input(Some(10), None)).status,
        CandidateStatus::InsufficientObservation
    );
    assert_eq!(
        classify(&input(Some(120), Some(100))).status,
        CandidateStatus::StaleCandidate
    );
    assert_eq!(
        classify(&input(Some(120), None)).status,
        CandidateStatus::NeverObserved
    );
    let mut unresolved = input(Some(1200), None);
    unresolved.capability = ObservationCapability::Unresolved;
    assert_eq!(classify(&unresolved).status, CandidateStatus::Unknown);
    assert_eq!(
        classify(&input(Some(3), Some(1))).status,
        CandidateStatus::Recent
    );
}

#[test]
fn cutoff_boundary_counts_as_recent() {
    let mut item = input(Some(200), None);
    item.last_used_at_ms = Some(NOW - 90 * DAY_MS);
    assert_eq!(classify(&item).status, CandidateStatus::Recent);
    item.last_used_at_ms = Some(NOW - 90 * DAY_MS - 1);
    assert_eq!(classify(&item).status, CandidateStatus::StaleCandidate);
}

#[test]
fn recent_use_wins_over_short_observation() {
    // 刚开始观察但阈值内已有记录，按近期处理而不是观察不足。
    assert_eq!(
        classify(&input(Some(2), Some(1))).status,
        CandidateStatus::Recent
    );
}

#[test]
fn priority_order_is_exclusion_then_availability_then_capability() {
    let mut item = input(Some(200), None);
    item.excluded = true;
    item.availability = Availability::Removed;
    assert_eq!(classify(&item).status, CandidateStatus::Excluded);
    item.excluded = false;
    assert_eq!(classify(&item).reason, StatusReason::Removed);
    item.availability = Availability::Unavailable;
    item.capability = ObservationCapability::Unsupported;
    assert_eq!(classify(&item).status, CandidateStatus::Unavailable);
    item.availability = Availability::Available;
    assert_eq!(classify(&item).reason, StatusReason::SourceUnsupported);
}

#[test]
fn missing_start_or_future_time_is_unknown() {
    assert_eq!(
        classify(&input(None, Some(1))).reason,
        StatusReason::NotObservedYet
    );
    let mut future = input(Some(100), None);
    future.last_used_at_ms = Some(NOW + 1);
    assert_eq!(classify(&future).reason, StatusReason::ClockAnomaly);
    future.last_used_at_ms = None;
    future.observation_started_at_ms = Some(NOW + DAY_MS);
    assert_eq!(classify(&future).reason, StatusReason::ClockAnomaly);
}

#[test]
fn reasons_carry_remaining_and_overdue_days() {
    assert_eq!(
        classify(&input(Some(20), None)).reason,
        StatusReason::NeedsMoreDays(70)
    );
    assert_eq!(
        classify(&input(Some(131), Some(115))).reason,
        StatusReason::OlderThanThreshold(25)
    );
    assert_eq!(StatusReason::NeedsMoreDays(70).label(90), "还需观察 70 天");
}

#[test]
fn restored_observation_keeps_old_history_but_needs_new_window() {
    // 恢复观察后旧代次最近时间早于新起点：观察不足，不能直接成为候选。
    assert_eq!(
        classify(&input(Some(5), Some(200))).status,
        CandidateStatus::InsufficientObservation
    );
}

fn span(state: IntervalState, start: i64, end: Option<i64>, verified: i64) -> IntervalSpan {
    IntervalSpan {
        state,
        started_at_ms: start,
        verified_until_ms: verified,
        ended_at_ms: end,
    }
}

#[test]
fn coverage_prefers_unknown_then_partial() {
    let active = span(IntervalState::Active, 0, Some(100), 100);
    let paused = span(IntervalState::Gap, 100, Some(150), 150);
    let resumed = span(IntervalState::Active, 150, Some(300), 300);
    assert_eq!(
        coverage(0, 300, &[active, resumed, paused]),
        CoverageStatus::Partial
    );
    let unknown = span(IntervalState::Unknown, 150, Some(160), 150);
    assert_eq!(
        coverage(0, 300, &[active, paused, unknown, resumed]),
        CoverageStatus::Unknown
    );
    let locked = span(IntervalState::Inactive, 100, Some(150), 150);
    assert_eq!(
        coverage(0, 300, &[active, locked, resumed]),
        CoverageStatus::Continuous
    );
}

#[test]
fn coverage_hole_or_stale_heartbeat_is_unknown() {
    let early = span(IntervalState::Active, 0, Some(100), 100);
    let late = span(IntervalState::Active, 120, Some(300), 300);
    assert_eq!(coverage(0, 300, &[early, late]), CoverageStatus::Unknown);
    let open_fresh = span(IntervalState::Active, 0, None, 280_000);
    assert_eq!(
        coverage(0, 300_000, &[open_fresh]),
        CoverageStatus::Continuous
    );
    let open_stale = span(IntervalState::Active, 0, None, 200_000);
    assert_eq!(coverage(0, 300_000, &[open_stale]), CoverageStatus::Unknown);
}

#[test]
fn gaps_outside_window_do_not_affect_coverage() {
    let paused = span(IntervalState::Gap, 0, Some(100), 100);
    let active = span(IntervalState::Active, 100, Some(500), 500);
    assert_eq!(
        coverage(200, 500, &[paused, active]),
        CoverageStatus::Continuous
    );
}

#[test]
fn civil_conversion_round_trips() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(
        days_from_civil(2026, 10, 11) - days_from_civil(2026, 6, 2),
        131
    );
    assert_eq!(
        desktrace_core::civil_from_days(days_from_civil(2024, 2, 29)),
        (2024, 2, 29)
    );
    assert_eq!(desktrace_core::format_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(
        desktrace_core::format_with_offset(0, 480),
        "1970-01-01T08:00:00+08:00"
    );
    assert_eq!(
        desktrace_core::format_with_offset(0, -330),
        "1969-12-31T18:30:00-05:30"
    );
}

#[test]
fn closed_active_interval_counts_only_until_verified() {
    // 进程被挂起后才关闭的区间：结束在 300，但只验证到 100，中间不能算作连续。
    let suspended = span(IntervalState::Active, 0, Some(300), 100);
    let resumed = span(IntervalState::Active, 300, Some(500), 500);
    assert_eq!(
        coverage(0, 500, &[suspended, resumed]),
        CoverageStatus::Unknown
    );
    assert_eq!(
        desktrace_core::coverage_holes(0, 500, &[suspended, resumed]),
        [(100, 300)]
    );
    // 进行中的暂停区间一直算到当前时刻。
    let paused = span(IntervalState::Gap, 100, None, 100);
    let active = span(IntervalState::Active, 0, Some(100), 100);
    assert_eq!(coverage(0, 500, &[active, paused]), CoverageStatus::Partial);
}

#[test]
fn unresolved_file_explains_unobservable_type() {
    let mut item = input(Some(200), None);
    item.object_type = ObjectType::File;
    item.capability = ObservationCapability::Unresolved;
    assert_eq!(classify(&item).reason, StatusReason::UnobservableFileType);
    assert_eq!(
        StatusReason::UnobservableFileType.label(90),
        "这类文件的打开无法记录"
    );
    item.capability = ObservationCapability::Supported;
    assert_eq!(classify(&item).status, CandidateStatus::NeverObserved);
}
