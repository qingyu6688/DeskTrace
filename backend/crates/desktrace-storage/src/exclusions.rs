//! 排除规则。排除只影响本工具的记录与判断，不操作任何真实文件或应用。

use desktrace_core::{ObjectType, is_same_or_descendant, normalize_local_path};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::objects::ExclusionMode;
use crate::{Storage, StorageError};

/// 一条排除规则及其展示信息。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionRule {
    /// 规则 ID。
    pub id: String,
    /// 规则类型。
    pub mode: ExclusionMode,
    /// 精确排除的对象 ID。
    pub object_id: Option<String>,
    /// 展示名称：对象名或目录路径。
    pub label: String,
    /// 对象类型；子树规则固定为目录。
    pub object_type: ObjectType,
    /// 建立时刻（UTC 毫秒）。
    #[serde(skip)]
    pub created_at_ms: i64,
}

impl Storage {
    /// 新增排除。子树模式只用于有路径的目录。同一对象或同一子树重复排除返回 `Conflict`。
    pub fn add_exclusion(
        &mut self,
        object_id: &str,
        mode: ExclusionMode,
        now_ms: i64,
    ) -> Result<ExclusionRule, StorageError> {
        let record = self
            .object_record(object_id)?
            .ok_or(StorageError::NotFound)?;
        let id = Uuid::new_v4().to_string();
        let rule = match mode {
            ExclusionMode::Object => {
                let inserted = self.connection.execute(
                    "INSERT INTO exclusion_rules (id,rule_kind,object_id,created_at,updated_at) VALUES (?1,'object',?2,?3,?3) ON CONFLICT DO NOTHING",
                    params![id, object_id, now_ms],
                )?;
                if inserted == 0 {
                    return Err(StorageError::Conflict);
                }
                ExclusionRule {
                    id,
                    mode,
                    object_id: Some(object_id.into()),
                    label: record.display_name,
                    object_type: record.object_type,
                    created_at_ms: now_ms,
                }
            }
            ExclusionMode::FolderTree => {
                if record.object_type != ObjectType::Folder {
                    return Err(StorageError::NotApplicable);
                }
                let path = record
                    .current_path
                    .as_deref()
                    .and_then(|path| normalize_local_path(path).ok())
                    .ok_or(StorageError::NotApplicable)?;
                let inserted = self.connection.execute(
                    "INSERT INTO exclusion_rules (id,rule_kind,path,normalized_key,created_at,updated_at) VALUES (?1,'folder_tree',?2,?3,?4,?4) ON CONFLICT DO NOTHING",
                    params![id, path.display, path.key, now_ms],
                )?;
                if inserted == 0 {
                    return Err(StorageError::Conflict);
                }
                ExclusionRule {
                    id,
                    mode,
                    object_id: None,
                    label: path.display,
                    object_type: ObjectType::Folder,
                    created_at_ms: now_ms,
                }
            }
        };
        Ok(rule)
    }

    /// 列出全部排除规则，新的在前。
    pub fn exclusions(&self) -> Result<Vec<ExclusionRule>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT r.id,r.rule_kind,r.object_id,COALESCE(o.display_name,r.path),COALESCE(o.object_type,'folder'),r.created_at FROM exclusion_rules r LEFT JOIN tracked_objects o ON o.id=r.object_id ORDER BY r.created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let kind: String = row.get(1)?;
            let object_type: String = row.get(4)?;
            Ok(ExclusionRule {
                id: row.get(0)?,
                mode: if kind == "object" {
                    ExclusionMode::Object
                } else {
                    ExclusionMode::FolderTree
                },
                object_id: row.get(2)?,
                label: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                object_type: ObjectType::parse(&object_type).unwrap_or(ObjectType::Folder),
                created_at_ms: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 删除排除规则，恢复的对象从当前时刻开始新的观察代次；旧记录保留但不计入新观察期。
    pub fn remove_exclusion(
        &mut self,
        rule_id: &str,
        running: bool,
        now_ms: i64,
    ) -> Result<usize, StorageError> {
        let transaction = self.connection.transaction()?;
        let rule: Option<(String, Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT rule_kind,object_id,normalized_key FROM exclusion_rules WHERE id=?1",
                [rule_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (kind, object_id, tree_key) = rule.ok_or(StorageError::NotFound)?;
        transaction.execute("DELETE FROM exclusion_rules WHERE id=?1", [rule_id])?;
        let affected: Vec<String> = if kind == "object" {
            object_id.into_iter().collect()
        } else {
            let parent = tree_key.unwrap_or_default();
            let mut statement = transaction.prepare(
                "SELECT id,current_path FROM tracked_objects WHERE object_type IN ('folder','file') AND current_path IS NOT NULL",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut ids = Vec::new();
            for row in rows {
                let (id, path) = row?;
                if normalize_local_path(&path)
                    .is_ok_and(|path| is_same_or_descendant(&path.key, &parent))
                {
                    ids.push(id);
                }
            }
            ids
        };
        let mut restarted = 0;
        for id in &affected {
            let key = transaction
                .query_row(
                    "SELECT current_path FROM tracked_objects WHERE id=?1",
                    [id],
                    |row| row.get::<_, Option<String>>(0),
                )?
                .and_then(|path| normalize_local_path(&path).ok())
                .map(|path| path.key);
            if crate::objects::object_excluded(&transaction, id, key.as_deref())? {
                continue;
            }
            restarted += transaction.execute(
                "UPDATE tracked_objects SET observation_generation=observation_generation+1, observation_started_at=CASE WHEN ?1 AND observation_capability='supported' THEN ?2 ELSE NULL END, updated_at=?2 WHERE id=?3",
                params![running, now_ms, id],
            )?;
        }
        transaction.commit()?;
        Ok(restarted)
    }
}
