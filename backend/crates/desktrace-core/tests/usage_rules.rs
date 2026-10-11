use desktrace_core::{IntervalReason, IntervalState, UsageEventKind, UsageObservation};
use uuid::Uuid;

fn observation(time: i64) -> UsageObservation {
    UsageObservation {
        object_id: Uuid::from_u128(1),
        kind: UsageEventKind::AppForeground,
        generation: 1,
        observed_at_ms: time,
    }
}

#[test]
fn minute_key_is_stable_and_versioned() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        observation(120_001).dedup_key()?,
        "v1:00000000-0000-0000-0000-000000000001:app_foreground:1:foreground_hook:2"
    );
    assert_eq!(
        observation(120_001).dedup_key()?,
        observation(179_999).dedup_key()?
    );
    assert_ne!(
        observation(179_999).dedup_key()?,
        observation(180_000).dedup_key()?
    );
    Ok(())
}

#[test]
fn object_kind_and_generation_do_not_collide() -> Result<(), Box<dyn std::error::Error>> {
    let initial = observation(120_001);
    let mut other = initial.clone();
    other.object_id = Uuid::from_u128(2);
    assert_ne!(initial.dedup_key()?, other.dedup_key()?);
    other = initial.clone();
    other.kind = UsageEventKind::AppActiveSample;
    assert_ne!(initial.dedup_key()?, other.dedup_key()?);
    other = initial.clone();
    other.generation = 2;
    assert_ne!(initial.dedup_key()?, other.dedup_key()?);
    Ok(())
}

#[test]
fn invalid_inputs_are_rejected() {
    assert!(observation(-1).dedup_key().is_err());
    let mut invalid = observation(0);
    invalid.generation = 0;
    assert!(invalid.dedup_key().is_err());
}

#[test]
fn each_reason_has_the_documented_state() {
    let expected = [
        (IntervalReason::CollectionStarted, IntervalState::Active),
        (IntervalReason::CollectionResumed, IntervalState::Active),
        (IntervalReason::CollectorRecovered, IntervalState::Active),
        (IntervalReason::SessionLocked, IntervalState::Inactive),
        (IntervalReason::SystemSuspended, IntervalState::Inactive),
        (IntervalReason::UserIdle, IntervalState::Inactive),
        (IntervalReason::UserPaused, IntervalState::Gap),
        (IntervalReason::CollectorFault, IntervalState::Gap),
        (IntervalReason::PermissionDenied, IntervalState::Gap),
        (IntervalReason::UnsupportedView, IntervalState::Gap),
        (IntervalReason::QueueOverflow, IntervalState::Gap),
        (IntervalReason::StorageError, IntervalState::Gap),
        (IntervalReason::SourceDisabled, IntervalState::Gap),
        (IntervalReason::StartupUnverified, IntervalState::Unknown),
        (IntervalReason::UnexpectedExit, IntervalState::Unknown),
        (IntervalReason::CollectorStopped, IntervalState::Unknown),
        (IntervalReason::ClockChange, IntervalState::Unknown),
    ];
    assert_eq!(expected.len(), IntervalReason::ALL.len());
    for (reason, state) in expected {
        assert_eq!(reason.state(), state);
    }
}

#[test]
fn recent_item_events_map_to_files_and_folders() {
    use desktrace_core::{ObjectType, UsageEventKind};
    assert_eq!(UsageEventKind::FileOpened.object_type(), ObjectType::File);
    assert_eq!(
        UsageEventKind::FolderContentOpened.object_type(),
        ObjectType::Folder
    );
    assert_eq!(UsageEventKind::FileOpened.source(), "recent_items");
    assert_eq!(ObjectType::parse("file"), Some(ObjectType::File));
}
