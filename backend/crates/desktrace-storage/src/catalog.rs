//! 应用清单同步。清单身份（卸载项、包入口）与启动入口分开保存；
//! 一次读取失败不等于应用被卸载，只有连续两次完整扫描缺失才标记失联。

use std::collections::HashSet;

use desktrace_core::{InstallDateEvidence, NormalizedPath, ObjectType};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use uuid::Uuid;

use crate::objects::{insert_identifier, insert_object};
use crate::{Storage, StorageError};

/// 连续缺失多少次完整扫描后标记为已失联。
pub const MISSING_SCANS_BEFORE_REMOVED: i64 = 2;

/// 一条清单记录。
#[derive(Clone, Debug)]
pub enum CatalogEntry {
    /// 卸载注册表中的 Win32 应用。
    Win32 {
        /// `HKLM\64\{GUID}` 形式的卸载项身份。
        uninstall_key: String,
        /// 展示名称。
        display_name: String,
        /// 发布者。
        publisher: Option<String>,
        /// 版本。
        version: Option<String>,
        /// 安装日期证据。
        install_date: InstallDateEvidence,
        /// 从 `DisplayIcon` 等字段确认存在的主程序；为空时清单项无法匹配前台使用。
        entry_executable: Option<NormalizedPath>,
    },
    /// 当前用户的打包应用入口。
    Package {
        /// 应用用户模型 ID。
        aumid: String,
        /// 展示名称。
        display_name: String,
        /// 发布者。
        publisher: Option<String>,
        /// 版本。
        version: Option<String>,
        /// 包安装或更新时间证据。
        install_date: InstallDateEvidence,
    },
}

/// 同步结果计数。
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSyncSummary {
    /// 新发现的应用数。
    pub added: usize,
    /// 已存在并刷新的应用数。
    pub refreshed: usize,
    /// 本次标记为已失联的应用数。
    pub removed: usize,
}

impl Storage {
    /// 同步一次清单读取结果。`complete` 为假（部分来源失败）时只登记和刷新，不累计缺失次数。
    pub fn sync_catalog(
        &mut self,
        entries: &[CatalogEntry],
        complete: bool,
        running: bool,
        now_ms: i64,
    ) -> Result<CatalogSyncSummary, StorageError> {
        let transaction = self.connection.transaction()?;
        let mut summary = CatalogSyncSummary::default();
        let mut seen = HashSet::new();
        for entry in entries {
            let (id, created) = upsert_entry(&transaction, entry, running, now_ms)?;
            if created {
                summary.added += 1
            } else {
                summary.refreshed += 1
            }
            seen.insert(id);
        }
        if complete {
            summary.removed = mark_missing(&transaction, &seen, now_ms)?;
        }
        transaction.commit()?;
        Ok(summary)
    }
}

fn upsert_entry(
    transaction: &Transaction<'_>,
    entry: &CatalogEntry,
    running: bool,
    now_ms: i64,
) -> Result<(String, bool), StorageError> {
    let (kind, key, identity_prefix, name, publisher, version, evidence, app_kind) = match entry {
        CatalogEntry::Win32 {
            uninstall_key,
            display_name,
            publisher,
            version,
            install_date,
            ..
        } => (
            "uninstall_key",
            uninstall_key,
            "uninstall",
            display_name,
            publisher,
            version,
            install_date,
            "win32",
        ),
        CatalogEntry::Package {
            aumid,
            display_name,
            publisher,
            version,
            install_date,
        } => (
            "package_entry",
            aumid,
            "aumid",
            display_name,
            publisher,
            version,
            install_date,
            "msix",
        ),
    };
    let existing: Option<String> = transaction
        .query_row(
            "SELECT object_id FROM object_identifiers WHERE identifier_kind=?1 AND identifier_key=?2",
            params![kind, key],
            |row| row.get(0),
        )
        .optional()?;
    let created = existing.is_none();
    let id = match existing {
        Some(id) => {
            transaction.execute(
                "UPDATE tracked_objects SET display_name=?1, availability=CASE WHEN availability='removed' THEN 'available' ELSE availability END, updated_at=?2 WHERE id=?3",
                params![name, now_ms, id],
            )?;
            id
        }
        None => {
            let id = Uuid::new_v4().to_string();
            insert_object(
                transaction,
                &id,
                ObjectType::Application,
                &format!("{identity_prefix}:{key}"),
                name,
                None,
                running,
                now_ms,
            )?;
            insert_identifier(transaction, &id, kind, key, "exact", "catalog", now_ms)?;
            id
        }
    };
    transaction.execute(
        "INSERT INTO application_details (id,application_kind,publisher,display_version,last_catalog_seen_at,missing_complete_scans,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,0,?5,?5) ON CONFLICT(id) DO UPDATE SET application_kind=excluded.application_kind, publisher=excluded.publisher, display_version=excluded.display_version, last_catalog_seen_at=excluded.last_catalog_seen_at, missing_complete_scans=0, updated_at=excluded.updated_at",
        params![id, app_kind, publisher, version, now_ms],
    )?;
    if let CatalogEntry::Win32 {
        entry_executable, ..
    } = entry
    {
        resolve_entry_executable(transaction, &id, entry_executable.as_ref(), running, now_ms)?;
    }
    save_evidence(transaction, &id, key, evidence, now_ms)?;
    Ok((id, created))
}

/// 主程序路径能确认时登记为已验证入口，对象才可观察；否则保持“无法匹配入口”。
/// 同一可执行文件已归属其他对象时不自动合并，避免把使用时间算到错误的应用上。
fn resolve_entry_executable(
    connection: &Connection,
    id: &str,
    executable: Option<&NormalizedPath>,
    running: bool,
    now_ms: i64,
) -> Result<(), StorageError> {
    let attached = match executable {
        Some(path) => {
            let owner: Option<String> = connection
                .query_row(
                    "SELECT object_id FROM object_identifiers WHERE identifier_kind='executable' AND identifier_key=?1",
                    [&path.key],
                    |row| row.get(0),
                )
                .optional()?;
            match owner {
                Some(owner) => owner == id,
                None => {
                    insert_identifier(connection, id, "executable", &path.key, "verified", "catalog", now_ms)?;
                    connection.execute(
                        "UPDATE tracked_objects SET current_path=?1 WHERE id=?2",
                        params![path.display, id],
                    )?;
                    true
                }
            }
        }
        None => connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM object_identifiers WHERE object_id=?1 AND identifier_kind='executable')",
            [id],
            |row| row.get(0),
        )?,
    };
    if attached {
        connection.execute(
            "UPDATE tracked_objects SET observation_capability='supported', observation_started_at=COALESCE(observation_started_at,?1), updated_at=?2 WHERE id=?3 AND observation_capability<>'supported'",
            params![running.then_some(now_ms), now_ms, id],
        )?;
    } else {
        connection.execute(
            "UPDATE tracked_objects SET observation_capability='unresolved', observation_started_at=NULL, updated_at=?1 WHERE id=?2 AND observation_capability<>'unresolved'",
            params![now_ms, id],
        )?;
    }
    Ok(())
}

/// 同一来源的日期变化时写入新快照；与最近一条相同则不重复写。
fn save_evidence(
    connection: &Connection,
    id: &str,
    source_key: &str,
    evidence: &InstallDateEvidence,
    now_ms: i64,
) -> Result<(), StorageError> {
    let latest: Option<(Option<String>, Option<String>, String)> = connection
        .query_row(
            "SELECT raw_value,date_value,validation_status FROM installation_evidence WHERE object_id=?1 AND source=?2 ORDER BY observed_at DESC LIMIT 1",
            params![id, evidence.source.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let unchanged = latest.is_some_and(|(raw, date, status)| {
        raw == evidence.raw_value
            && date == evidence.date_value
            && status == evidence.validation.as_str()
    });
    if unchanged {
        return Ok(());
    }
    connection.execute(
        "INSERT INTO installation_evidence (id,object_id,source,source_key,raw_value,date_value,date_precision,date_meaning,validation_status,observed_at,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,?10)",
        params![
            Uuid::new_v4().to_string(),
            id,
            evidence.source.as_str(),
            source_key,
            evidence.raw_value,
            evidence.date_value,
            evidence.precision.as_str(),
            evidence.meaning.as_str(),
            evidence.validation.as_str(),
            now_ms
        ],
    )?;
    Ok(())
}

/// 完整扫描中没出现的清单应用累计缺失次数，达到阈值后标记为已失联。前台发现的便携应用不参与。
fn mark_missing(
    transaction: &Transaction<'_>,
    seen: &HashSet<String>,
    now_ms: i64,
) -> Result<usize, StorageError> {
    let mut statement = transaction.prepare(
        "SELECT DISTINCT o.id FROM tracked_objects o JOIN object_identifiers i ON i.object_id=o.id WHERE o.object_type='application' AND i.source='catalog' AND i.identifier_kind IN ('uninstall_key','package_entry')",
    )?;
    let ids: Vec<String> = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    drop(statement);
    let mut removed = 0;
    for id in ids.iter().filter(|id| !seen.contains(*id)) {
        transaction.execute(
            "UPDATE application_details SET missing_complete_scans=missing_complete_scans+1, updated_at=?1 WHERE id=?2",
            params![now_ms, id],
        )?;
        removed += transaction.execute(
            "UPDATE tracked_objects SET availability='removed', updated_at=?1 WHERE id=?2 AND availability<>'removed' AND (SELECT missing_complete_scans FROM application_details WHERE id=?2)>=?3",
            params![now_ms, id, MISSING_SCANS_BEFORE_REMOVED],
        )?;
    }
    Ok(removed)
}
