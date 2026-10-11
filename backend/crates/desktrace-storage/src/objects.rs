//! 对象读取与按精确身份登记。只按身份标识合并，不按名称或安装目录猜测归属。

use desktrace_core::{
    Availability, CollectorModule, NormalizedPath, ObjectType, ObservationCapability, ScopeMatch,
    ScopeRule, is_same_or_descendant, normalize_local_path, resolve_in_scopes,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use uuid::Uuid;

use crate::{Storage, StorageError};

/// 排除规则的类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionMode {
    /// 只排除这个对象。
    Object,
    /// 排除目录及其子目录。
    FolderTree,
}

impl ExclusionMode {
    /// 返回数据库中的固定值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::FolderTree => "folder_tree",
        }
    }
}

/// 对象命中的排除规则。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionRef {
    /// 规则 ID。
    pub rule_id: String,
    /// 规则类型。
    pub mode: ExclusionMode,
}

/// 当前选用的安装日期证据。
#[derive(Clone, Debug)]
pub struct EvidenceRecord {
    /// 来源。
    pub source: String,
    /// 原始值。
    pub raw_value: Option<String>,
    /// 日期值。
    pub date_value: Option<String>,
    /// 精度。
    pub precision: String,
    /// 含义。
    pub meaning: String,
    /// 校验结果。
    pub validation: String,
    /// 读取时刻。
    pub observed_at_ms: i64,
}

/// 从数据库读出的对象及其关联信息，候选状态在查询时计算，不落库。
#[derive(Clone, Debug)]
pub struct ObjectRecord {
    /// UUID 文本。
    pub id: String,
    /// 类型。
    pub object_type: ObjectType,
    /// 展示名称。
    pub display_name: String,
    /// 当前路径或入口。
    pub current_path: Option<String>,
    /// 可访问状态。
    pub availability: Availability,
    /// 观察能力。
    pub capability: ObservationCapability,
    /// 首次发现。
    pub first_seen_at_ms: i64,
    /// 当前代次观察起点。
    pub observation_started_at_ms: Option<i64>,
    /// 最近直接观察。
    pub last_used_at_ms: Option<i64>,
    /// 观察代次。
    pub generation: u32,
    /// 命中的排除规则。
    pub exclusion: Option<ExclusionRef>,
    /// 应用类型。
    pub application_kind: Option<String>,
    /// 发布者。
    pub publisher: Option<String>,
    /// 版本。
    pub display_version: Option<String>,
    /// 当前安装日期证据。
    pub installation: Option<EvidenceRecord>,
}

impl ObjectRecord {
    /// 对象所属的采集模块。
    pub fn module(&self) -> CollectorModule {
        CollectorModule::for_object(self.object_type)
    }
}

const RECORD_SELECT: &str = "SELECT o.id,o.object_type,o.display_name,o.current_path,o.availability,o.observation_capability,o.first_seen_at,o.observation_started_at,o.last_used_at,o.observation_generation,r.id,d.application_kind,d.publisher,d.display_version,e.source,e.raw_value,e.date_value,e.date_precision,e.date_meaning,e.validation_status,e.observed_at FROM tracked_objects o LEFT JOIN exclusion_rules r ON r.rule_kind='object' AND r.object_id=o.id LEFT JOIN application_details d ON d.id=o.id LEFT JOIN (SELECT object_id,source,raw_value,date_value,date_precision,date_meaning,validation_status,observed_at,ROW_NUMBER() OVER (PARTITION BY object_id ORDER BY validation_status='valid' DESC, observed_at DESC) AS position FROM installation_evidence) e ON e.object_id=o.id AND e.position=1";

/// 应用的可观察身份，由前台窗口所属进程解析得到。
#[derive(Clone, Debug)]
pub enum AppIdentity {
    /// 打包应用入口（AUMID）。
    Package {
        /// 应用用户模型 ID。
        aumid: String,
        /// 展示名称。
        display_name: String,
    },
    /// 普通可执行文件。
    Executable {
        /// 规范化后的可执行文件路径。
        path: NormalizedPath,
        /// 展示名称，通常取文件描述或文件名。
        display_name: String,
    },
}

/// 一个已浏览的目录，文件 ID 可读时优先作为身份。
#[derive(Clone, Debug)]
pub struct FolderTarget {
    /// 规范化路径。
    pub path: NormalizedPath,
    /// `卷序列号:文件 ID` 形式的稳定身份，不可读时为空。
    pub file_identity: Option<String>,
}

/// 文件系统对象的种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// 目录。
    Folder,
    /// 文件。
    File,
}

impl PathKind {
    /// 对应的对象类型。
    pub fn object_type(self) -> ObjectType {
        match self {
            Self::Folder => ObjectType::Folder,
            Self::File => ObjectType::File,
        }
    }

    fn file_id_kind(self) -> &'static str {
        match self {
            Self::Folder => "folder_file_id",
            Self::File => "file_id",
        }
    }

    fn path_kind(self) -> &'static str {
        match self {
            Self::Folder => "folder_path",
            Self::File => "file_path",
        }
    }
}

/// 要登记的文件或目录。
#[derive(Clone, Debug)]
pub struct PathTarget {
    /// 种类。
    pub kind: PathKind,
    /// 规范化路径。
    pub path: NormalizedPath,
    /// `卷序列号:文件 ID` 形式的稳定身份，不可读时为空。
    pub file_identity: Option<String>,
}

/// 可以写入事件的对象上下文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservableObject {
    /// 对象 ID。
    pub id: Uuid,
    /// 当前代次。
    pub generation: u32,
}

impl Storage {
    /// 读取某类对象的全部记录。
    pub fn object_records(
        &self,
        object_type: ObjectType,
    ) -> Result<Vec<ObjectRecord>, StorageError> {
        let tree_rules = tree_rules(&self.connection)?;
        let mut statement = self
            .connection
            .prepare(&format!("{RECORD_SELECT} WHERE o.object_type=?1"))?;
        let rows = statement.query_map([object_type.as_str()], read_record)?;
        let mut records = Vec::new();
        for row in rows {
            let mut record = row?;
            apply_tree_rules(&mut record, &tree_rules);
            records.push(record);
        }
        Ok(records)
    }

    /// 按 ID 读取一个对象。
    pub fn object_record(&self, id: &str) -> Result<Option<ObjectRecord>, StorageError> {
        let tree_rules = tree_rules(&self.connection)?;
        let record = self
            .connection
            .query_row(&format!("{RECORD_SELECT} WHERE o.id=?1"), [id], read_record)
            .optional()?;
        Ok(record.map(|mut record| {
            apply_tree_rules(&mut record, &tree_rules);
            record
        }))
    }

    /// 登记或找到一个已浏览目录。不在启用范围内或已被排除时返回 `None`，调用方不写事件。
    /// 范围不包含子目录时，更深处的浏览记到范围根下的直接子项；此时传入的文件 ID 不属于该子项，不使用。
    pub fn ensure_folder(
        &mut self,
        target: &FolderTarget,
        running: bool,
        now_ms: i64,
    ) -> Result<Option<ObservableObject>, StorageError> {
        let Some(found) = self.resolve_scope_path(&target.path)? else {
            return Ok(None);
        };
        let file_identity = if found.direct {
            target.file_identity.clone()
        } else {
            None
        };
        let path = PathTarget {
            kind: PathKind::Folder,
            path: found.target,
            file_identity,
        };
        self.ensure_path_object(&path, true, running, now_ms)
    }

    /// 已登记的文件与目录路径键，扫描范围时只为新路径读取文件 ID。
    pub fn path_identifier_keys(&self) -> Result<std::collections::HashSet<String>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT identifier_key FROM object_identifiers WHERE identifier_kind IN ('folder_path','file_path')",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 按启用的观察范围解析路径归属，不在任何范围内时返回 `None`。
    pub fn resolve_scope_path(
        &self,
        path: &NormalizedPath,
    ) -> Result<Option<ScopeMatch>, StorageError> {
        let scopes = enabled_scopes(&self.connection)?;
        Ok(resolve_with(path, &scopes))
    }

    /// 登记或找到一个文件或目录对象，调用方已确认它属于观察范围。命中排除时返回 `None`。
    /// `observable` 为假表示这类文件的打开记录不到，对象只列出、不参与候选判断。
    pub fn ensure_path_object(
        &mut self,
        target: &PathTarget,
        observable: bool,
        running: bool,
        now_ms: i64,
    ) -> Result<Option<ObservableObject>, StorageError> {
        // 先查子树排除再登记，被排除目录下的内容不留下新对象。
        if tree_rules(&self.connection)?
            .iter()
            .any(|(_, parent)| is_same_or_descendant(&target.path.key, parent))
        {
            return Ok(None);
        }
        let transaction = self.connection.transaction()?;
        let object = upsert_path(&transaction, target, observable, running, now_ms)?;
        let result = if object_excluded(&transaction, &object.id.to_string(), None)? {
            None
        } else {
            Some(object)
        };
        transaction.commit()?;
        Ok(result)
    }

    /// 登记或找到前台应用对应的对象。已排除时返回 `None`。
    pub fn ensure_application(
        &mut self,
        identity: &AppIdentity,
        running: bool,
        now_ms: i64,
    ) -> Result<Option<ObservableObject>, StorageError> {
        let (kind, key, display, path, app_kind) = match identity {
            AppIdentity::Package {
                aumid,
                display_name,
            } => (
                "package_entry",
                aumid.clone(),
                display_name.clone(),
                None,
                "msix",
            ),
            AppIdentity::Executable { path, display_name } => (
                "executable",
                path.key.clone(),
                display_name.clone(),
                Some(path.display.clone()),
                "portable",
            ),
        };
        let transaction = self.connection.transaction()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT object_id FROM object_identifiers WHERE identifier_kind=?1 AND identifier_key=?2",
                params![kind, key],
                |row| row.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => {
                make_supported(&transaction, &id, running, now_ms)?;
                id
            }
            None => {
                let id = Uuid::new_v4().to_string();
                let identity_key = format!(
                    "{}:{key}",
                    if kind == "executable" { "exe" } else { "aumid" }
                );
                insert_object(
                    &transaction,
                    &id,
                    ObjectType::Application,
                    &identity_key,
                    &display,
                    path.as_deref(),
                    running,
                    now_ms,
                )?;
                insert_identifier(&transaction, &id, kind, &key, "exact", "foreground", now_ms)?;
                transaction.execute(
                    "INSERT INTO application_details (id,application_kind,last_catalog_seen_at,created_at,updated_at) VALUES (?1,?2,?3,?3,?3)",
                    params![id, app_kind, now_ms],
                )?;
                id
            }
        };
        let excluded = object_excluded(&transaction, &id, None)?;
        let generation = generation(&transaction, &id)?;
        transaction.commit()?;
        Ok((!excluded).then(|| ObservableObject {
            id: parse_uuid(&id),
            generation,
        }))
    }

    /// 更新对象的可访问状态，例如外置盘断开或重新连接。
    pub fn set_availability(
        &mut self,
        id: &str,
        availability: Availability,
        now_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "UPDATE tracked_objects SET availability=?1, updated_at=?2 WHERE id=?3",
            params![availability.as_str(), now_ms, id],
        )?;
        Ok(())
    }

    /// 首次启用或恢复记录时，为尚无观察起点的可观察对象建立起点。
    pub fn start_pending_observations(&mut self, now_ms: i64) -> Result<usize, StorageError> {
        Ok(self.connection.execute(
            "UPDATE tracked_objects SET observation_started_at=?1, updated_at=?1 WHERE observation_started_at IS NULL AND observation_capability='supported' AND availability='available'",
            [now_ms],
        )?)
    }
}

fn read_record(row: &Row<'_>) -> rusqlite::Result<ObjectRecord> {
    let object_type: String = row.get(1)?;
    let availability: String = row.get(4)?;
    let capability: String = row.get(5)?;
    let rule_id: Option<String> = row.get(10)?;
    let source: Option<String> = row.get(14)?;
    let installation = match source {
        Some(source) => Some(EvidenceRecord {
            source,
            raw_value: row.get(15)?,
            date_value: row.get(16)?,
            precision: row.get(17)?,
            meaning: row.get(18)?,
            validation: row.get(19)?,
            observed_at_ms: row.get(20)?,
        }),
        None => None,
    };
    Ok(ObjectRecord {
        id: row.get(0)?,
        object_type: ObjectType::parse(&object_type).unwrap_or(ObjectType::Application),
        display_name: row.get(2)?,
        current_path: row.get(3)?,
        availability: Availability::parse(&availability),
        capability: ObservationCapability::parse(&capability),
        first_seen_at_ms: row.get(6)?,
        observation_started_at_ms: row.get(7)?,
        last_used_at_ms: row.get(8)?,
        generation: row.get(9)?,
        exclusion: rule_id.map(|rule_id| ExclusionRef {
            rule_id,
            mode: ExclusionMode::Object,
        }),
        application_kind: row.get(11)?,
        publisher: row.get(12)?,
        display_version: row.get(13)?,
        installation,
    })
}

pub(crate) fn tree_rules(connection: &Connection) -> Result<Vec<(String, String)>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id,normalized_key FROM exclusion_rules WHERE rule_kind='folder_tree' ORDER BY length(normalized_key)",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn apply_tree_rules(record: &mut ObjectRecord, rules: &[(String, String)]) {
    if record.exclusion.is_some() || record.object_type == ObjectType::Application {
        return;
    }
    let Some(key) = record
        .current_path
        .as_deref()
        .and_then(|path| normalize_local_path(path).ok())
    else {
        return;
    };
    if let Some((rule_id, _)) = rules
        .iter()
        .find(|(_, parent)| is_same_or_descendant(&key.key, parent))
    {
        record.exclusion = Some(ExclusionRef {
            rule_id: rule_id.clone(),
            mode: ExclusionMode::FolderTree,
        });
    }
}

/// 检查对象是否命中精确排除，目录另查子树排除。
pub(crate) fn object_excluded(
    connection: &Connection,
    id: &str,
    folder_key: Option<&str>,
) -> Result<bool, StorageError> {
    let exact: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM exclusion_rules WHERE rule_kind='object' AND object_id=?1)",
        [id],
        |row| row.get(0),
    )?;
    if exact {
        return Ok(true);
    }
    let Some(key) = folder_key else {
        return Ok(false);
    };
    Ok(tree_rules(connection)?
        .iter()
        .any(|(_, parent)| is_same_or_descendant(key, parent)))
}

/// 启用的观察范围：根路径与是否包含子目录。
pub(crate) fn enabled_scopes(
    connection: &Connection,
) -> Result<Vec<(NormalizedPath, bool)>, StorageError> {
    let mut statement =
        connection.prepare("SELECT path,include_descendants FROM folder_scopes WHERE enabled=1")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
    })?;
    let mut scopes = Vec::new();
    for row in rows {
        let (path, include_descendants) = row?;
        if let Ok(path) = normalize_local_path(&path) {
            scopes.push((path, include_descendants));
        }
    }
    Ok(scopes)
}

pub(crate) fn resolve_with(
    path: &NormalizedPath,
    scopes: &[(NormalizedPath, bool)],
) -> Option<ScopeMatch> {
    let rules: Vec<ScopeRule<'_>> = scopes
        .iter()
        .map(|(root, include_descendants)| ScopeRule {
            root,
            include_descendants: *include_descendants,
        })
        .collect();
    resolve_in_scopes(path, &rules)
}

/// 路径是否正是某个范围下的观察对象：范围根、范围的直接子项，或包含子目录时的任意子项。
pub(crate) fn is_scope_target(path: &NormalizedPath, scopes: &[(NormalizedPath, bool)]) -> bool {
    resolve_with(path, scopes).is_some_and(|found| found.direct)
}

fn upsert_path(
    connection: &Connection,
    target: &PathTarget,
    observable: bool,
    running: bool,
    now_ms: i64,
) -> Result<ObservableObject, StorageError> {
    let kind = target.kind;
    let by_file: Option<String> = match &target.file_identity {
        Some(file) => connection
            .query_row(
                "SELECT object_id FROM object_identifiers WHERE identifier_kind=?1 AND identifier_key=?2",
                params![kind.file_id_kind(), file],
                |row| row.get(0),
            )
            .optional()?,
        None => None,
    };
    let by_path: Option<String> = connection
        .query_row(
            "SELECT object_id FROM object_identifiers WHERE identifier_kind=?1 AND identifier_key=?2",
            params![kind.path_kind(), target.path.key],
            |row| row.get(0),
        )
        .optional()?;
    let id = match (by_file, by_path) {
        // 文件 ID 一致说明是同一对象，路径变化视为同卷内重命名或移动，路径标识跟着更新。
        (Some(id), path_owner) => {
            if path_owner.as_deref() != Some(id.as_str()) {
                connection.execute(
                    "DELETE FROM object_identifiers WHERE object_id=?1 AND identifier_kind=?2",
                    params![id, kind.path_kind()],
                )?;
                if path_owner.is_none() {
                    insert_identifier(
                        connection,
                        &id,
                        kind.path_kind(),
                        &target.path.key,
                        "exact",
                        "filesystem",
                        now_ms,
                    )?;
                }
            }
            id
        }
        (None, Some(id)) => {
            if let Some(file) = &target.file_identity {
                attach_file_identity(connection, &id, kind, file, now_ms)?;
            }
            id
        }
        (None, None) => {
            let id = Uuid::new_v4().to_string();
            let identity_key = match &target.file_identity {
                Some(file) => format!("fileid:{file}"),
                None => format!("path:{}", target.path.key),
            };
            let name = desktrace_core::display_name(&target.path.display);
            insert_object(
                connection,
                &id,
                kind.object_type(),
                &identity_key,
                &name,
                Some(&target.path.display),
                running,
                now_ms,
            )?;
            insert_identifier(
                connection,
                &id,
                kind.path_kind(),
                &target.path.key,
                "exact",
                "filesystem",
                now_ms,
            )?;
            if let Some(file) = &target.file_identity {
                insert_identifier(
                    connection,
                    &id,
                    kind.file_id_kind(),
                    file,
                    "exact",
                    "filesystem",
                    now_ms,
                )?;
            }
            id
        }
    };
    connection.execute(
        "UPDATE tracked_objects SET current_path=?1, display_name=?2, availability='available', updated_at=?3 WHERE id=?4 AND (current_path IS NOT ?1 OR availability<>'available')",
        params![target.path.display, desktrace_core::display_name(&target.path.display), now_ms, id],
    )?;
    if observable {
        make_supported(connection, &id, running, now_ms)?;
    } else {
        connection.execute(
            "UPDATE tracked_objects SET observation_capability='unresolved', observation_started_at=NULL, updated_at=?1 WHERE id=?2 AND observation_capability<>'unresolved'",
            params![now_ms, id],
        )?;
    }
    Ok(ObservableObject {
        id: parse_uuid(&id),
        generation: generation(connection, &id)?,
    })
}

fn attach_file_identity(
    connection: &Connection,
    id: &str,
    kind: PathKind,
    file: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO object_identifiers (id,object_id,identifier_kind,identifier_key,match_quality,source,created_at,updated_at) VALUES (?1,?2,?3,?4,'exact','filesystem',?5,?5) ON CONFLICT(identifier_kind,identifier_key) DO NOTHING",
        params![Uuid::new_v4().to_string(), id, kind.file_id_kind(), file, now_ms],
    )?;
    Ok(())
}

/// 对象重新变为可观察：此前不受支持的对象开始新的观察代次，旧历史保留但不计入新观察期。
fn make_supported(
    connection: &Connection,
    id: &str,
    running: bool,
    now_ms: i64,
) -> Result<(), StorageError> {
    let start = running.then_some(now_ms);
    connection.execute(
        "UPDATE tracked_objects SET observation_capability='supported', observation_generation=observation_generation+1, observation_started_at=?1, updated_at=?2 WHERE id=?3 AND observation_capability<>'supported'",
        params![start, now_ms, id],
    )?;
    if running {
        connection.execute(
            "UPDATE tracked_objects SET observation_started_at=?1, updated_at=?1 WHERE id=?2 AND observation_started_at IS NULL",
            params![now_ms, id],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_object(
    connection: &Connection,
    id: &str,
    object_type: ObjectType,
    identity_key: &str,
    display_name: &str,
    current_path: Option<&str>,
    running: bool,
    now_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO tracked_objects (id,object_type,identity_key,display_name,current_path,observation_capability,first_seen_at,observation_started_at,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'supported',?6,?7,?6,?6)",
        params![id, object_type.as_str(), identity_key, display_name, current_path, now_ms, running.then_some(now_ms)],
    )?;
    Ok(())
}

pub(crate) fn insert_identifier(
    connection: &Connection,
    object_id: &str,
    kind: &str,
    key: &str,
    quality: &str,
    source: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO object_identifiers (id,object_id,identifier_kind,identifier_key,match_quality,source,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
        params![Uuid::new_v4().to_string(), object_id, kind, key, quality, source, now_ms],
    )?;
    Ok(())
}

fn generation(connection: &Connection, id: &str) -> Result<u32, StorageError> {
    Ok(connection.query_row(
        "SELECT observation_generation FROM tracked_objects WHERE id=?1",
        [id],
        |row| row.get(0),
    )?)
}

/// 数据库中的 ID 都由本程序以 UUID 写入；解析失败只可能来自外部篡改，按空 UUID 处理后写入会被上下文校验拒绝。
pub(crate) fn parse_uuid(id: &str) -> Uuid {
    Uuid::parse_str(id).unwrap_or_default()
}
