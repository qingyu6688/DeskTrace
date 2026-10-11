use desktrace_core::{
    CandidateStatus, CollectionState, CollectorModule, CoverageStatus, DAY_MS, IntervalReason,
    ObjectType, Settings, SettingsPatch, UsageEventKind, UsageObservation, normalize_local_path,
    package_install_date, parse_registry_install_date,
};
use uuid::Uuid;

use crate::{
    AppIdentity, CatalogEntry, ExclusionMode, FolderTarget, ObjectQuery, ObjectSummary, PathKind,
    PathTarget, ScopeItem, SortField, Storage, StorageError,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const T0: i64 = 1_780_000_000_000;
const NOW: i64 = T0 + 131 * DAY_MS;

fn query(object_type: ObjectType) -> ObjectQuery {
    ObjectQuery {
        object_types: vec![object_type],
        keyword: None,
        statuses: Vec::new(),
        coverage: None,
        threshold_days: 90,
        sort_by: SortField::LastUsedAt,
        descending: false,
        page: 1,
        page_size: 50,
    }
}

fn find<'a>(items: &'a [ObjectSummary], name: &str) -> Option<&'a ObjectSummary> {
    items.iter().find(|item| item.display_name == name)
}

fn path(text: &str) -> Result<desktrace_core::NormalizedPath, desktrace_core::PathError> {
    normalize_local_path(text)
}

fn win32(
    key: &str,
    name: &str,
    exe: Option<&str>,
) -> Result<CatalogEntry, desktrace_core::PathError> {
    Ok(CatalogEntry::Win32 {
        uninstall_key: key.into(),
        display_name: name.into(),
        publisher: Some("测试发布者".into()),
        version: Some("1.0".into()),
        install_date: parse_registry_install_date(Some("20260302"), true, 20_600),
        entry_executable: exe.map(path).transpose()?,
    })
}

/// 把开放区间的心跳推进到给定时刻，模拟一直正常运行。
fn verify_until(storage: &Storage, at: i64) -> Result<(), rusqlite::Error> {
    storage.connection.execute(
        "UPDATE collector_intervals SET verified_until=?1 WHERE ended_at IS NULL",
        [at],
    )?;
    Ok(())
}

fn start_collection(storage: &mut Storage, at: i64) -> Result<(), StorageError> {
    storage.save_collection_state(CollectionState::Running, at)?;
    storage.start_pending_observations(at)?;
    for module in CollectorModule::ALL {
        storage.transition_interval(module, IntervalReason::CollectionStarted, at)?;
    }
    Ok(())
}

fn record(
    storage: &mut Storage,
    id: Uuid,
    generation: u32,
    kind: UsageEventKind,
    at: i64,
) -> Result<(), StorageError> {
    storage.record_usage(
        &UsageObservation {
            object_id: id,
            kind,
            generation,
            observed_at_ms: at,
        },
        at,
    )?;
    Ok(())
}

#[test]
fn catalog_lifecycle_produces_expected_candidates() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    let entries = vec![
        win32(
            r"HKLM\64\Blender",
            "Blender",
            Some(r"C:\Program Files\Blender\blender.exe"),
        )?,
        win32(r"HKLM\64\Python", "Python", None)?,
        CatalogEntry::Package {
            aumid: "Clipchamp_xyz!App".into(),
            display_name: "Clipchamp".into(),
            publisher: None,
            version: Some("4.3".into()),
            install_date: package_install_date(Some(T0 - DAY_MS), T0),
        },
    ];
    storage.sync_catalog(&entries, true, false, T0)?;
    // 尚未启用记录：可观察对象也没有观察起点，只能“无法判断”。
    let before = storage.list_objects(&query(ObjectType::Application), T0)?;
    assert!(
        before
            .items
            .iter()
            .all(|item| item.candidate_status == CandidateStatus::Unknown)
    );

    start_collection(&mut storage, T0)?;
    let blender = storage
        .ensure_application(
            &AppIdentity::Executable {
                path: path(r"c:\program files\blender\BLENDER.exe")?,
                display_name: "blender.exe".into(),
            },
            true,
            T0 + DAY_MS,
        )?
        .ok_or("Blender 应匹配到清单对象")?;
    record(
        &mut storage,
        blender.id,
        blender.generation,
        UsageEventKind::AppForeground,
        T0 + DAY_MS,
    )?;
    verify_until(&storage, T0 + 50 * DAY_MS)?;
    storage.transition_interval(
        CollectorModule::Applications,
        IntervalReason::UserPaused,
        T0 + 50 * DAY_MS,
    )?;
    storage.transition_interval(
        CollectorModule::Applications,
        IntervalReason::CollectionResumed,
        T0 + 53 * DAY_MS,
    )?;
    verify_until(&storage, NOW)?;

    let page = storage.list_objects(&query(ObjectType::Application), NOW)?;
    assert_eq!(page.total, 3);
    let blender = find(&page.items, "Blender").ok_or("缺少 Blender")?;
    assert_eq!(
        (blender.candidate_status, blender.coverage_status),
        (CandidateStatus::StaleCandidate, CoverageStatus::Partial)
    );
    assert_eq!(blender.installation_date.as_deref(), Some("2026-03-02"));
    let python = find(&page.items, "Python").ok_or("缺少 Python")?;
    assert_eq!(
        (python.candidate_status, python.status_reason.as_str()),
        (CandidateStatus::Unknown, "没有可匹配的启动入口")
    );
    let clipchamp = find(&page.items, "Clipchamp").ok_or("缺少 Clipchamp")?;
    assert_eq!(clipchamp.candidate_status, CandidateStatus::NeverObserved);
    assert_eq!(clipchamp.installation_meaning, "install_or_update");
    Ok(())
}

#[test]
fn unmatched_executable_becomes_portable_object() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    let identity = AppIdentity::Executable {
        path: path(r"D:\Tools\drawio.exe")?,
        display_name: "draw.io".into(),
    };
    let first = storage
        .ensure_application(&identity, true, T0)?
        .ok_or("应登记便携应用")?;
    let again = storage
        .ensure_application(&identity, true, T0 + 1)?
        .ok_or("应找到同一对象")?;
    assert_eq!(first, again);
    let page = storage.list_objects(&query(ObjectType::Application), T0 + 20 * DAY_MS)?;
    assert_eq!(page.items[0].application_kind.as_deref(), Some("portable"));
    assert_eq!(
        page.items[0].candidate_status,
        CandidateStatus::InsufficientObservation
    );
    assert_eq!(page.items[0].remaining_observation_days, Some(70));
    Ok(())
}

#[test]
fn catalog_marks_removed_only_after_two_complete_scans() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    let entry = win32(r"HKCU\XMind", "XMind", None)?;
    storage.sync_catalog(std::slice::from_ref(&entry), true, false, T0)?;
    assert_eq!(storage.sync_catalog(&[], false, false, T0 + 1)?.removed, 0);
    assert_eq!(storage.sync_catalog(&[], true, false, T0 + 2)?.removed, 0);
    assert_eq!(storage.sync_catalog(&[], true, false, T0 + 3)?.removed, 1);
    let removed = storage.list_objects(&query(ObjectType::Application), T0 + 3)?;
    assert_eq!(
        removed.items[0].candidate_status,
        CandidateStatus::Unavailable
    );
    storage.sync_catalog(std::slice::from_ref(&entry), true, false, T0 + 4)?;
    let back = storage.object_records(ObjectType::Application)?;
    assert_eq!(
        back[0].availability,
        desktrace_core::Availability::Available
    );
    Ok(())
}

#[test]
fn install_evidence_snapshots_only_on_change() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    let entry = win32(r"HKLM\64\App", "App", None)?;
    storage.sync_catalog(std::slice::from_ref(&entry), true, false, T0)?;
    storage.sync_catalog(std::slice::from_ref(&entry), true, false, T0 + 1)?;
    let mut changed = entry.clone();
    if let CatalogEntry::Win32 { install_date, .. } = &mut changed {
        *install_date = parse_registry_install_date(Some("20260401"), true, 20_600);
    }
    storage.sync_catalog(&[changed], true, false, T0 + 2)?;
    let rows: i64 =
        storage
            .connection
            .query_row("SELECT COUNT(*) FROM installation_evidence", [], |row| {
                row.get(0)
            })?;
    assert_eq!(rows, 2);
    let current = storage.list_objects(&query(ObjectType::Application), T0 + 3)?;
    assert_eq!(
        current.items[0].installation_date.as_deref(),
        Some("2026-04-01")
    );
    Ok(())
}

#[test]
fn folder_scope_controls_observation() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    let scope = storage.add_folder_scope(&path(r"D:\Projects")?, None, true, true, T0)?;
    assert!(matches!(
        storage.add_folder_scope(&path(r"d:\projects\")?, None, false, true, T0),
        Err(StorageError::ScopeExists)
    ));
    let child = FolderTarget {
        path: path(r"D:\Projects\旧网站")?,
        file_identity: Some("0001:00AB".into()),
    };
    assert!(storage.ensure_folder(&child, true, T0 + 1)?.is_some());
    let nested = FolderTarget {
        path: path(r"D:\Projects\旧网站\src")?,
        file_identity: None,
    };
    assert!(storage.ensure_folder(&nested, true, T0 + 1)?.is_some());
    let outside = FolderTarget {
        path: path(r"D:\ProjectsArchive")?,
        file_identity: None,
    };
    assert!(storage.ensure_folder(&outside, true, T0 + 1)?.is_none());

    storage.update_folder_scope(&scope.id, true, false, true, T0 + 2)?;
    let folders = storage.list_objects(&query(ObjectType::Folder), T0 + 3)?;
    // 不包含子目录时，直接子项仍是对象，更深的对象移出范围。
    assert_eq!(
        find(&folders.items, "src")
            .ok_or("缺少深层目录")?
            .status_reason,
        "不在当前观察范围"
    );
    assert_eq!(
        find(&folders.items, "旧网站")
            .ok_or("缺少子目录")?
            .candidate_status,
        CandidateStatus::InsufficientObservation
    );
    assert_eq!(
        find(&folders.items, "Projects")
            .ok_or("缺少范围根")?
            .candidate_status,
        CandidateStatus::InsufficientObservation
    );

    // 同一文件 ID 换了路径：视为同卷重命名，延续同一对象。
    storage.update_folder_scope(&scope.id, true, true, true, T0 + 4)?;
    let renamed = FolderTarget {
        path: path(r"D:\Projects\旧网站-归档")?,
        file_identity: Some("0001:00AB".into()),
    };
    storage.ensure_folder(&renamed, true, T0 + 5)?;
    let folders = storage.object_records(ObjectType::Folder)?;
    assert_eq!(folders.len(), 3);
    assert!(
        folders
            .iter()
            .any(|item| item.display_name == "旧网站-归档")
    );

    storage.remove_folder_scope(&scope.id, true, T0 + 6)?;
    let all_unknown = storage.list_objects(&query(ObjectType::Folder), T0 + 7)?;
    assert!(
        all_unknown
            .items
            .iter()
            .all(|item| item.candidate_status == CandidateStatus::Unknown)
    );
    Ok(())
}

#[test]
fn tree_exclusion_blocks_descendants_and_restore_restarts_observation() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.add_folder_scope(&path(r"C:\Work")?, None, true, true, T0)?;
    let parent = storage
        .ensure_folder(
            &FolderTarget {
                path: path(r"C:\Work\a")?,
                file_identity: None,
            },
            true,
            T0,
        )?
        .ok_or("父目录")?;
    let child = storage
        .ensure_folder(
            &FolderTarget {
                path: path(r"C:\Work\a\b")?,
                file_identity: None,
            },
            true,
            T0,
        )?
        .ok_or("子目录")?;
    let sibling = storage
        .ensure_folder(
            &FolderTarget {
                path: path(r"C:\Work\ab")?,
                file_identity: None,
            },
            true,
            T0,
        )?
        .ok_or("相邻目录")?;
    let rule = storage.add_exclusion(&parent.id.to_string(), ExclusionMode::FolderTree, T0 + 1)?;
    assert!(matches!(
        storage.add_exclusion(&parent.id.to_string(), ExclusionMode::FolderTree, T0 + 1),
        Err(StorageError::Conflict)
    ));
    assert!(
        storage
            .ensure_folder(
                &FolderTarget {
                    path: path(r"C:\Work\a\b\c")?,
                    file_identity: None
                },
                true,
                T0 + 2
            )?
            .is_none()
    );
    let blocked = storage.record_usage(
        &UsageObservation {
            object_id: child.id,
            kind: UsageEventKind::FolderBrowsed,
            generation: child.generation,
            observed_at_ms: T0 + 2,
        },
        T0 + 2,
    );
    assert!(matches!(blocked, Err(StorageError::ObjectExcluded)));
    record(
        &mut storage,
        sibling.id,
        sibling.generation,
        UsageEventKind::FolderBrowsed,
        T0 + 2,
    )?;

    let listed = storage.list_objects(
        &ObjectQuery {
            statuses: vec![CandidateStatus::Excluded],
            ..query(ObjectType::Folder)
        },
        T0 + 3,
    )?;
    assert_eq!(listed.total, 2);
    assert_eq!(storage.exclusions()?.len(), 1);

    assert_eq!(storage.remove_exclusion(&rule.id, true, T0 + 10)?, 2);
    let restored = storage
        .object_record(&child.id.to_string())?
        .ok_or("子目录")?;
    assert_eq!(
        (restored.generation, restored.observation_started_at_ms),
        (child.generation + 1, Some(T0 + 10))
    );
    // 新代次之前的写入请求必须被拒绝。
    assert!(
        storage
            .record_usage(
                &UsageObservation {
                    object_id: child.id,
                    kind: UsageEventKind::FolderBrowsed,
                    generation: child.generation,
                    observed_at_ms: T0 + 11
                },
                T0 + 11
            )
            .is_err()
    );
    assert!(matches!(
        storage.add_exclusion(&Uuid::nil().to_string(), ExclusionMode::Object, T0),
        Err(StorageError::NotFound)
    ));
    Ok(())
}

#[test]
fn sorting_keeps_unknown_times_last_and_paginates() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    let mut ids = Vec::new();
    for (index, name) in ["甲", "乙", "丙", "丁"].iter().enumerate() {
        let item = storage
            .ensure_application(
                &AppIdentity::Executable {
                    path: path(&format!(r"C:\Apps\{index}.exe"))?,
                    display_name: (*name).into(),
                },
                true,
                T0,
            )?
            .ok_or("应登记")?;
        ids.push(item);
    }
    record(
        &mut storage,
        ids[0].id,
        ids[0].generation,
        UsageEventKind::AppForeground,
        T0 + 3 * DAY_MS,
    )?;
    record(
        &mut storage,
        ids[1].id,
        ids[1].generation,
        UsageEventKind::AppForeground,
        T0 + DAY_MS,
    )?;
    let now = T0 + 4 * DAY_MS;
    let ascending = storage.matching_objects(&query(ObjectType::Application), now)?;
    let names: Vec<_> = ascending
        .iter()
        .map(|item| item.display_name.as_str())
        .collect();
    assert_eq!(&names[..2], ["乙", "甲"]);
    let descending = storage.matching_objects(
        &ObjectQuery {
            descending: true,
            ..query(ObjectType::Application)
        },
        now,
    )?;
    let names: Vec<_> = descending
        .iter()
        .map(|item| item.display_name.as_str())
        .collect();
    assert_eq!(&names[..2], ["甲", "乙"]);
    assert!(descending[2].last_used_at.is_none() && descending[3].last_used_at.is_none());
    let second = storage.list_objects(
        &ObjectQuery {
            page: 2,
            page_size: 3,
            ..query(ObjectType::Application)
        },
        now,
    )?;
    assert_eq!((second.total, second.items.len()), (4, 1));
    let keyword = storage.list_objects(
        &ObjectQuery {
            keyword: Some(r"apps\2".into()),
            ..query(ObjectType::Application)
        },
        now,
    )?;
    assert_eq!(keyword.total, 1);
    Ok(())
}

#[test]
fn startup_recovery_marks_unverified_time() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.heartbeat(CollectorModule::Applications, T0 + 15_000)?;
    // 文件夹模块正常退出。
    storage.transition_interval(
        CollectorModule::Folders,
        IntervalReason::CollectorStopped,
        T0 + 15_000,
    )?;
    storage.recover_open_intervals(T0 + 3_600_000)?;
    let gaps = storage.gaps(None, T0, T0 + 3_600_000, 10)?;
    // 应用模块与没有心跳的文件模块都按异常退出处理，文件夹模块是正常退出。
    assert_eq!(gaps.len(), 3);
    let crash = gaps
        .iter()
        .find(|gap| gap.reason == "unexpected_exit" && gap.module == CollectorModule::Applications)
        .ok_or("缺少异常退出区间")?;
    assert_eq!(
        (crash.started_at_ms, crash.ended_at_ms),
        (T0 + 15_000, Some(T0 + 3_600_000))
    );
    let open: i64 = storage.connection.query_row(
        "SELECT COUNT(*) FROM collector_intervals WHERE ended_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(open, 0);
    Ok(())
}

#[test]
fn late_heartbeat_leaves_unverified_hole() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    let app = storage
        .ensure_application(
            &AppIdentity::Package {
                aumid: "A!App".into(),
                display_name: "A".into(),
            },
            true,
            T0,
        )?
        .ok_or("应登记")?;
    storage.heartbeat(CollectorModule::Applications, T0 + 30_000)?;
    // 系统挂起两小时后才恢复心跳。
    storage.heartbeat(CollectorModule::Applications, T0 + 7_200_000)?;
    let now = T0 + 7_210_000;
    let item = storage.list_objects(&query(ObjectType::Application), now)?;
    assert_eq!(item.items[0].id, app.id.to_string());
    assert_eq!(item.items[0].coverage_status, CoverageStatus::Unknown);
    Ok(())
}

#[test]
fn continuous_coverage_when_only_inactive_breaks() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.ensure_application(
        &AppIdentity::Package {
            aumid: "B!App".into(),
            display_name: "B".into(),
        },
        true,
        T0,
    )?;
    verify_until(&storage, T0 + 10_000)?;
    storage.transition_interval(
        CollectorModule::Applications,
        IntervalReason::SessionLocked,
        T0 + 10_000,
    )?;
    storage.transition_interval(
        CollectorModule::Applications,
        IntervalReason::CollectionResumed,
        T0 + 20_000,
    )?;
    verify_until(&storage, NOW)?;
    let page = storage.list_objects(
        &ObjectQuery {
            coverage: Some(CoverageStatus::Continuous),
            ..query(ObjectType::Application)
        },
        NOW,
    )?;
    assert_eq!(page.total, 1);
    assert_eq!(
        page.items[0].candidate_status,
        CandidateStatus::NeverObserved
    );
    Ok(())
}

#[test]
fn clear_and_purge_keep_settings_and_recent_time() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.add_folder_scope(&path(r"E:\素材库")?, None, true, true, T0)?;
    let app = storage
        .ensure_application(
            &AppIdentity::Package {
                aumid: "C!App".into(),
                display_name: "C".into(),
            },
            true,
            T0,
        )?
        .ok_or("应登记")?;
    record(
        &mut storage,
        app.id,
        app.generation,
        UsageEventKind::AppForeground,
        T0 + DAY_MS,
    )?;
    record(
        &mut storage,
        app.id,
        app.generation,
        UsageEventKind::AppForeground,
        T0 + 200 * DAY_MS,
    )?;
    assert_eq!(storage.purge_expired_events(180, T0 + 200 * DAY_MS)?, 1);
    let kept = storage.object_record(&app.id.to_string())?.ok_or("应用")?;
    assert_eq!(kept.last_used_at_ms, Some(T0 + 200 * DAY_MS));

    let revision = storage.data_revision()?;
    assert_eq!(storage.objects_with_usage()?, 1);
    storage.clear_usage_data(true, T0 + 201 * DAY_MS)?;
    assert_ne!(storage.data_revision()?, revision);
    let cleared = storage.object_record(&app.id.to_string())?.ok_or("应用")?;
    assert_eq!(
        (
            cleared.last_used_at_ms,
            cleared.generation,
            cleared.observation_started_at_ms
        ),
        (None, app.generation + 1, Some(T0 + 201 * DAY_MS))
    );
    assert_eq!(storage.summary()?.event_count, 0);
    assert_eq!(storage.folder_scopes()?.len(), 1);
    Ok(())
}

#[test]
fn detail_track_uses_local_days_and_retention_window() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    let app = storage
        .ensure_application(
            &AppIdentity::Package {
                aumid: "D!App".into(),
                display_name: "D".into(),
            },
            true,
            T0,
        )?
        .ok_or("应登记")?;
    // 同一 UTC 日的 15:59 与 16:01 在东八区分属两天。
    let day = T0 - T0.rem_euclid(DAY_MS) + 10 * DAY_MS;
    record(
        &mut storage,
        app.id,
        app.generation,
        UsageEventKind::AppForeground,
        day + 15 * 3_600_000 + 59 * 60_000,
    )?;
    record(
        &mut storage,
        app.id,
        app.generation,
        UsageEventKind::AppForeground,
        day + 16 * 3_600_000 + 60_000,
    )?;
    verify_until(&storage, T0 + 20 * DAY_MS)?;
    storage.transition_interval(
        CollectorModule::Applications,
        IntervalReason::UserPaused,
        T0 + 20 * DAY_MS,
    )?;
    storage.transition_interval(
        CollectorModule::Applications,
        IntervalReason::CollectionResumed,
        T0 + 21 * DAY_MS,
    )?;
    // 恢复后没有心跳：21 天之后到查询时刻是未验证空档。
    let now = T0 + 40 * DAY_MS;
    let detail = storage.object_detail(&app.id.to_string(), 90, 180, now, &|_| 480)?;
    let track = detail.track.ok_or("应有轨迹")?;
    assert_eq!(track.usage_days.len(), 2);
    assert_eq!(track.gaps.len(), 2, "暂停区间与心跳空档都应列出");
    assert_eq!(detail.recent_events.len(), 2);
    assert_eq!(detail.recent_events[0].label, "进入前台");
    // 保留期只有 30 天时，轨迹起点被裁剪，早期记录不再出现在日期中。
    let clipped = storage
        .object_detail(&app.id.to_string(), 90, 30, now, &|_| 480)?
        .track
        .ok_or("应有轨迹")?;
    assert!(clipped.usage_days.is_empty());
    assert!(clipped.last_used_at.is_some());
    assert!(matches!(
        storage.object_detail("missing", 90, 180, now, &|_| 0),
        Err(StorageError::NotFound)
    ));
    Ok(())
}

#[test]
fn settings_and_collection_state_round_trip() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    assert_eq!(storage.settings()?, Settings::default());
    assert_eq!(storage.collection_state()?, CollectionState::NotStarted);
    let updated = storage.settings()?.apply(SettingsPatch {
        threshold_days: Some(30),
        ..SettingsPatch::default()
    })?;
    storage.save_settings(&updated, T0)?;
    storage.save_collection_state(CollectionState::Paused, T0)?;
    assert_eq!(storage.settings()?.threshold_days, 30);
    assert_eq!(storage.collection_state()?, CollectionState::Paused);
    storage.connection.execute(
        "UPDATE settings SET value_json='99999' WHERE setting_key='threshold_days'",
        [],
    )?;
    assert_eq!(
        storage.settings()?.threshold_days,
        90,
        "越界的外部修改回落到默认值"
    );
    Ok(())
}

#[test]
fn overview_counts_every_status() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    storage.sync_catalog(&[win32(r"HKLM\64\X", "X", None)?], true, false, T0)?;
    start_collection(&mut storage, T0)?;
    let overview = storage.overview(90, T0 + DAY_MS)?;
    assert_eq!(overview.applications.total, 1);
    assert_eq!(overview.applications.counts.len(), 7);
    assert_eq!(
        overview.applications.counts.get(&CandidateStatus::Unknown),
        Some(&1)
    );
    assert!(overview.observation_started_at.is_some());
    Ok(())
}

fn scope_item(
    kind: PathKind,
    text: &str,
    identity: Option<&str>,
) -> Result<ScopeItem, desktrace_core::PathError> {
    let path = path(text)?;
    let observable = kind == PathKind::Folder
        || desktrace_core::file_open_observable(&desktrace_core::display_name(&path.display));
    Ok(ScopeItem {
        kind,
        path,
        file_identity: identity.map(str::to_string),
        observable,
    })
}

#[test]
fn desktop_items_are_registered_and_judged() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.add_folder_scope(&path(r"C:\Users\me\Desktop")?, None, false, true, T0)?;
    let items = vec![
        scope_item(
            PathKind::File,
            r"C:\Users\me\Desktop\报价.xlsx",
            Some("0001:0001"),
        )?,
        scope_item(
            PathKind::File,
            r"C:\Users\me\Desktop\Chrome.lnk",
            Some("0001:0002"),
        )?,
        scope_item(
            PathKind::Folder,
            r"C:\Users\me\Desktop\旧资料",
            Some("0001:0003"),
        )?,
        // 不属于任何范围的项被忽略。
        scope_item(PathKind::File, r"D:\其他\a.txt", None)?,
    ];
    assert_eq!(storage.sync_scope_items(&items, true, T0)?, 3);
    assert_eq!(
        storage.sync_scope_items(&items, true, T0 + 1)?,
        3,
        "重复同步不新增对象"
    );
    assert_eq!(storage.object_records(ObjectType::File)?.len(), 2);

    let deep = path(r"C:\Users\me\Desktop\旧资料\2024\合同.docx")?;
    let found = storage.resolve_scope_path(&deep)?.ok_or("应命中桌面范围")?;
    assert_eq!(
        (found.target.display.as_str(), found.direct),
        (r"C:\Users\me\Desktop\旧资料", false)
    );
    let folder = storage
        .ensure_path_object(
            &PathTarget {
                kind: PathKind::Folder,
                path: found.target,
                file_identity: None,
            },
            true,
            true,
            T0 + DAY_MS,
        )?
        .ok_or("应找到旧资料")?;
    record(
        &mut storage,
        folder.id,
        folder.generation,
        UsageEventKind::FolderContentOpened,
        T0 + DAY_MS,
    )?;
    verify_until(&storage, NOW)?;

    let page = storage.list_objects(
        &ObjectQuery {
            object_types: vec![ObjectType::Folder, ObjectType::File],
            ..query(ObjectType::File)
        },
        NOW,
    )?;
    assert_eq!(page.total, 4, "桌面根、两个文件和一个文件夹");
    let quote = find(&page.items, "报价.xlsx").ok_or("缺少报价")?;
    assert_eq!(quote.candidate_status, CandidateStatus::NeverObserved);
    let shortcut = find(&page.items, "Chrome.lnk").ok_or("缺少快捷方式")?;
    assert_eq!(
        (shortcut.candidate_status, shortcut.status_reason.as_str()),
        (CandidateStatus::Unknown, "这类文件的打开无法记录")
    );
    let old = find(&page.items, "旧资料").ok_or("缺少旧资料")?;
    assert_eq!(old.candidate_status, CandidateStatus::StaleCandidate);
    let overview = storage.overview(90, NOW)?;
    assert_eq!(overview.files.total, 2);
    Ok(())
}

#[test]
fn file_open_records_and_respects_tree_exclusion() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.add_folder_scope(&path(r"E:\桌面")?, None, true, true, T0)?;
    let file = storage
        .ensure_path_object(
            &PathTarget {
                kind: PathKind::File,
                path: path(r"E:\桌面\项目\需求.docx")?,
                file_identity: None,
            },
            true,
            true,
            T0,
        )?
        .ok_or("应登记文件")?;
    record(
        &mut storage,
        file.id,
        file.generation,
        UsageEventKind::FileOpened,
        T0 + 1_000,
    )?;
    assert!(
        storage
            .record_usage(
                &UsageObservation {
                    object_id: file.id,
                    kind: UsageEventKind::FolderBrowsed,
                    generation: file.generation,
                    observed_at_ms: T0 + 2_000
                },
                T0 + 2_000
            )
            .is_err(),
        "目录事件不能写到文件对象"
    );
    let project = storage
        .ensure_folder(
            &FolderTarget {
                path: path(r"E:\桌面\项目")?,
                file_identity: None,
            },
            true,
            T0,
        )?
        .ok_or("项目目录")?;
    storage.add_exclusion(
        &project.id.to_string(),
        ExclusionMode::FolderTree,
        T0 + 3_000,
    )?;
    let blocked = storage.record_usage(
        &UsageObservation {
            object_id: file.id,
            kind: UsageEventKind::FileOpened,
            generation: file.generation,
            observed_at_ms: T0 + 4_000,
        },
        T0 + 4_000,
    );
    assert!(matches!(blocked, Err(StorageError::ObjectExcluded)));
    let files = storage.list_objects(&query(ObjectType::File), T0 + 5_000)?;
    assert_eq!(files.items[0].candidate_status, CandidateStatus::Excluded);
    Ok(())
}

#[test]
fn version_one_database_upgrades_with_data_kept() -> TestResult {
    let connection = rusqlite::Connection::open_in_memory()?;
    connection.execute_batch(crate::INITIAL_MIGRATION)?;
    let checksum: String = {
        use sha2::{Digest, Sha256};
        Sha256::digest(crate::INITIAL_MIGRATION.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    connection.execute(
        "INSERT INTO schema_migrations (id,name,checksum,applied_at,created_at,updated_at) VALUES (1,'0001_initial',?1,0,0,0)",
        [&checksum],
    )?;
    connection.pragma_update(None, "user_version", 1)?;
    connection.execute(
        "INSERT INTO tracked_objects (id,object_type,identity_key,display_name,observation_capability,first_seen_at,observation_started_at,created_at,updated_at) VALUES ('00000000-0000-0000-0000-000000000001','application','exe:X','X','supported',0,0,0,0)",
        [],
    )?;
    connection.execute(
        "INSERT INTO usage_events (id,object_id,event_kind,source,observed_at,observation_generation,dedup_key,created_at,updated_at) VALUES ('e1','00000000-0000-0000-0000-000000000001','app_foreground','foreground_hook',5,1,'k1',5,5)",
        [],
    )?;
    let mut storage = Storage::initialize(connection, T0, None)?;
    assert_eq!(
        storage.summary()?.schema_version,
        crate::LATEST_SCHEMA_VERSION
    );
    assert_eq!(
        (
            storage.summary()?.object_count,
            storage.summary()?.event_count
        ),
        (1, 1)
    );
    let violations: i64 = storage.connection.query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_check",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(violations, 0);
    let foreign_keys: bool = storage
        .connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    assert!(foreign_keys, "迁移后必须重新开启外键");
    // 新类型可以写入，级联删除仍然有效。
    storage.add_folder_scope(&path(r"C:\Desk")?, None, false, true, T0)?;
    storage.sync_scope_items(
        &[scope_item(PathKind::File, r"C:\Desk\a.txt", None)?],
        true,
        T0,
    )?;
    storage.connection.execute(
        "DELETE FROM tracked_objects WHERE object_type='application'",
        [],
    )?;
    assert_eq!(storage.summary()?.event_count, 0);
    Ok(())
}

#[test]
fn newer_schema_is_rejected() -> TestResult {
    let connection = rusqlite::Connection::open_in_memory()?;
    connection.pragma_update(None, "user_version", crate::LATEST_SCHEMA_VERSION + 1)?;
    assert!(matches!(
        Storage::initialize(connection, T0, None),
        Err(StorageError::MigrationMismatch)
    ));
    Ok(())
}

#[test]
fn file_backed_upgrade_leaves_snapshot() -> TestResult {
    let directory = std::env::temp_dir().join(format!("desktrace-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory)?;
    let database = directory.join("desktrace.sqlite3");
    {
        let connection = rusqlite::Connection::open(&database)?;
        connection.execute_batch(crate::INITIAL_MIGRATION)?;
        let checksum: String = {
            use sha2::{Digest, Sha256};
            Sha256::digest(crate::INITIAL_MIGRATION.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        };
        connection.execute("INSERT INTO schema_migrations (id,name,checksum,applied_at,created_at,updated_at) VALUES (1,'0001_initial',?1,0,0,0)", [&checksum])?;
        connection.pragma_update(None, "user_version", 1)?;
    }
    let storage = Storage::open(&database, T0)?;
    assert_eq!(storage.summary()?.schema_version, 2);
    drop(storage);
    let snapshots = std::fs::read_dir(directory.join("backups"))?.count();
    assert_eq!(snapshots, 1);
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

#[test]
fn flags_are_one_time_markers() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    assert!(!storage.flag("desktop_scope_seeded")?);
    storage.set_flag("desktop_scope_seeded", T0)?;
    assert!(storage.flag("desktop_scope_seeded")?);
    assert_eq!(
        storage.settings()?,
        Settings::default(),
        "标记不影响白名单设置"
    );
    Ok(())
}

#[test]
fn disabled_recent_items_make_file_conclusions_unknown() -> TestResult {
    let mut storage = Storage::in_memory(T0)?;
    start_collection(&mut storage, T0)?;
    storage.add_folder_scope(&path(r"C:\Desk")?, None, false, true, T0)?;
    storage.sync_scope_items(
        &[
            scope_item(PathKind::File, r"C:\Desk\a.txt", None)?,
            scope_item(PathKind::Folder, r"C:\Desk\b", None)?,
        ],
        true,
        T0,
    )?;
    verify_until(&storage, T0 + 50 * DAY_MS)?;
    storage.transition_interval(
        CollectorModule::Files,
        IntervalReason::SourceDisabled,
        T0 + 50 * DAY_MS,
    )?;
    verify_until(&storage, NOW)?;
    let items = storage.list_objects(
        &ObjectQuery {
            object_types: vec![ObjectType::Folder, ObjectType::File],
            ..query(ObjectType::File)
        },
        NOW,
    )?;
    let file = find(&items.items, "a.txt").ok_or("缺少文件")?;
    assert_eq!(
        (file.candidate_status, file.status_reason.as_str()),
        (
            CandidateStatus::Unknown,
            "系统关闭了“最近使用的项目”，无法记录文件打开"
        )
    );
    // 文件夹浏览不依赖该来源，照常判断。
    assert_eq!(
        find(&items.items, "b")
            .ok_or("缺少文件夹")?
            .candidate_status,
        CandidateStatus::NeverObserved
    );
    // 重新开启后，暂停期间按缺口计，结论恢复但带记录中断标识。
    storage.transition_interval(
        CollectorModule::Files,
        IntervalReason::CollectionResumed,
        NOW,
    )?;
    let after = storage.list_objects(&query(ObjectType::File), NOW + 1)?;
    assert_eq!(
        (
            after.items[0].candidate_status,
            after.items[0].coverage_status
        ),
        (CandidateStatus::NeverObserved, CoverageStatus::Partial)
    );
    Ok(())
}
