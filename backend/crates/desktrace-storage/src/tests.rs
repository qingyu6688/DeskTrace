use desktrace_core::{IntervalReason, IntervalState, ObjectType, UsageEventKind, UsageObservation};
use rusqlite::params;
use uuid::Uuid;

use crate::{Storage, TrackedObjectDraft};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn storage() -> Result<Storage, crate::StorageError> {
    let mut storage = Storage::in_memory(0)?;
    storage.register_object(&TrackedObjectDraft {
        id: Uuid::from_u128(1),
        object_type: ObjectType::Application,
        identity_key: "application-1".into(),
        display_name: "测试应用".into(),
        generation: 1,
        observation_started_at_ms: 0,
    })?;
    Ok(storage)
}

fn observation(time: i64) -> UsageObservation {
    UsageObservation {
        object_id: Uuid::from_u128(1),
        kind: UsageEventKind::AppForeground,
        generation: 1,
        observed_at_ms: time,
    }
}

#[test]
fn same_minute_merges_but_advances_recent_time() -> TestResult {
    let mut storage = storage()?;
    let first = storage.record_usage(&observation(120_001), 120_001)?;
    let original: (String, i64) =
        storage
            .connection
            .query_row("SELECT id,created_at FROM usage_events", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
    let later = storage.record_usage(&observation(179_999), 179_999)?;
    let retry = storage.record_usage(&observation(120_001), 240_000)?;
    assert!(first.inserted);
    assert!(!later.inserted);
    assert!(!retry.inserted);
    assert_eq!(retry.last_used_at_ms, 179_999);
    assert_eq!(storage.summary()?.event_count, 1);
    let merged: (String, i64, i64) = storage.connection.query_row(
        "SELECT id,created_at,observed_at FROM usage_events",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!((merged.0, merged.1), original);
    assert_eq!(merged.2, 179_999);
    Ok(())
}

#[test]
fn new_minute_and_event_kind_create_distinct_rows() -> TestResult {
    let mut storage = storage()?;
    storage.record_usage(&observation(179_999), 179_999)?;
    storage.record_usage(&observation(180_000), 180_000)?;
    let mut sample = observation(180_000);
    sample.kind = UsageEventKind::AppActiveSample;
    storage.record_usage(&sample, 180_000)?;
    assert_eq!(storage.summary()?.event_count, 3);
    Ok(())
}

#[test]
fn invalid_context_does_not_write_or_refresh() -> TestResult {
    let mut storage = storage()?;
    let mut event = observation(120_000);
    event.generation = 2;
    assert!(storage.record_usage(&event, 120_000).is_err());
    event = observation(120_000);
    event.kind = UsageEventKind::FolderBrowsed;
    assert!(storage.record_usage(&event, 120_000).is_err());
    assert!(
        storage
            .record_usage(&observation(120_000), 119_999)
            .is_err()
    );
    storage.connection.execute(
        "UPDATE tracked_objects SET observation_started_at=120001",
        [],
    )?;
    assert!(
        storage
            .record_usage(&observation(120_000), 120_000)
            .is_err()
    );
    assert_eq!(storage.summary()?.event_count, 0);
    let latest: Option<i64> =
        storage
            .connection
            .query_row("SELECT last_used_at FROM tracked_objects", [], |row| {
                row.get(0)
            })?;
    assert_eq!(latest, None);
    Ok(())
}

#[test]
fn exclusion_blocks_pending_events() -> TestResult {
    let mut storage = storage()?;
    storage.connection.execute("INSERT INTO exclusion_rules (id,rule_kind,object_id,created_at,updated_at) VALUES ('rule-1','object',?1,0,0)", [Uuid::from_u128(1).to_string()])?;
    assert!(
        storage
            .record_usage(&observation(120_000), 120_000)
            .is_err()
    );
    assert_eq!(storage.summary()?.event_count, 0);
    Ok(())
}

#[test]
fn database_enforces_all_reason_state_pairs() -> TestResult {
    let storage = storage()?;
    for reason in IntervalReason::ALL {
        let state = reason.state();
        storage.connection.execute(
            "INSERT INTO collector_intervals (id,module,interval_state,reason,started_at,verified_until,ended_at,created_at,updated_at) VALUES (?1,'applications',?2,?3,0,0,0,0,0)",
            params![reason.as_str(),state.as_str(),reason.as_str()],
        )?;
        for wrong in [
            IntervalState::Active,
            IntervalState::Inactive,
            IntervalState::Gap,
            IntervalState::Unknown,
        ] {
            if wrong == state {
                continue;
            }
            let result = storage.connection.execute(
                "INSERT INTO collector_intervals (id,module,interval_state,reason,started_at,verified_until,ended_at,created_at,updated_at) VALUES (?1,'folders',?2,?3,0,0,0,0,0)",
                params![format!("invalid-{}-{}",reason.as_str(),wrong.as_str()),wrong.as_str(),reason.as_str()],
            );
            assert!(result.is_err());
        }
    }
    assert!(storage.connection.execute("INSERT INTO collector_intervals VALUES ('free','applications','active','自由文本',0,0,0,0,0)", []).is_err());
    Ok(())
}

#[test]
fn migration_is_idempotent_and_rejects_changed_checksum() -> TestResult {
    let mut storage = storage()?;
    storage.apply_migrations(1, None)?;
    let count: u32 =
        storage
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
    assert_eq!(count, 2);
    storage
        .connection
        .execute("UPDATE schema_migrations SET checksum='changed'", [])?;
    assert!(storage.apply_migrations(2, None).is_err());
    Ok(())
}

#[test]
fn new_generation_does_not_merge_with_previous_generation() -> TestResult {
    let mut storage = storage()?;
    storage.record_usage(&observation(120_001), 120_001)?;
    storage.connection.execute(
        "UPDATE tracked_objects SET observation_generation=2, observation_started_at=120002",
        [],
    )?;
    let mut newer = observation(120_003);
    newer.generation = 2;
    assert!(storage.record_usage(&newer, 120_003)?.inserted);
    assert_eq!(storage.summary()?.event_count, 2);
    assert!(
        storage
            .record_usage(&observation(120_001), 120_004)
            .is_err()
    );
    Ok(())
}

#[test]
fn aggregate_failure_rolls_back_event_insert() -> TestResult {
    let mut storage = storage()?;
    storage.connection.execute_batch("CREATE TRIGGER reject_recent_update BEFORE UPDATE OF last_used_at ON tracked_objects BEGIN SELECT RAISE(ABORT, '测试聚合失败'); END;")?;
    assert!(
        storage
            .record_usage(&observation(120_001), 120_001)
            .is_err()
    );
    assert_eq!(storage.summary()?.event_count, 0);
    Ok(())
}
