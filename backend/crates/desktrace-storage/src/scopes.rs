//! 文件夹观察范围。范围变化后重新计算目录对象能否被观察。

use desktrace_core::{NormalizedPath, ObjectType, file_open_observable, normalize_local_path};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::objects::{FolderTarget, PathKind, PathTarget, enabled_scopes, is_scope_target};
use crate::{Storage, StorageError};

/// 一个观察范围。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderScope {
    /// 范围 ID。
    pub id: String,
    /// 展示路径。
    pub path: String,
    /// 是否包含子目录。
    pub include_descendants: bool,
    /// 是否启用。
    pub enabled: bool,
    /// 是否为当前用户的系统桌面，由调用方按系统已知文件夹标记。
    pub is_system_desktop: bool,
}

/// 范围根目录下的一个直接子项，由调用方从文件系统读出。
#[derive(Clone, Debug)]
pub struct ScopeItem {
    /// 文件或目录。
    pub kind: PathKind,
    /// 规范化路径。
    pub path: NormalizedPath,
    /// 稳定身份。
    pub file_identity: Option<String>,
    /// 打开时能否被观察到；程序、快捷方式等为假。
    pub observable: bool,
}

impl Storage {
    /// 列出全部范围。
    pub fn folder_scopes(&self) -> Result<Vec<FolderScope>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT id,path,include_descendants,enabled FROM folder_scopes ORDER BY path COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(FolderScope {
                id: row.get(0)?,
                path: row.get(1)?,
                include_descendants: row.get(2)?,
                enabled: row.get(3)?,
                is_system_desktop: false,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 新增范围，并为范围根目录建立对象，让从未浏览的根目录也能成为“观察期内未记录”。
    /// 路径已存在时返回 `ScopeExists`。
    pub fn add_folder_scope(
        &mut self,
        path: &NormalizedPath,
        file_identity: Option<String>,
        include_descendants: bool,
        running: bool,
        now_ms: i64,
    ) -> Result<FolderScope, StorageError> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM folder_scopes WHERE normalized_key=?1)",
            [&path.key],
            |row| row.get(0),
        )?;
        if exists {
            return Err(StorageError::ScopeExists);
        }
        let scope = FolderScope {
            id: Uuid::new_v4().to_string(),
            path: path.display.clone(),
            include_descendants,
            enabled: true,
            is_system_desktop: false,
        };
        self.connection.execute(
            "INSERT INTO folder_scopes (id,path,normalized_key,include_descendants,enabled,created_at,updated_at) VALUES (?1,?2,?3,?4,1,?5,?5)",
            params![scope.id, scope.path, path.key, include_descendants, now_ms],
        )?;
        self.ensure_folder(
            &FolderTarget {
                path: path.clone(),
                file_identity,
            },
            running,
            now_ms,
        )?;
        refresh_folder_capabilities(&self.connection, running, now_ms)?;
        Ok(scope)
    }

    /// 修改范围的启用状态和子目录设置。
    pub fn update_folder_scope(
        &mut self,
        id: &str,
        enabled: bool,
        include_descendants: bool,
        running: bool,
        now_ms: i64,
    ) -> Result<FolderScope, StorageError> {
        let changed = self.connection.execute(
            "UPDATE folder_scopes SET enabled=?1, include_descendants=?2, updated_at=?3 WHERE id=?4",
            params![enabled, include_descendants, now_ms, id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        refresh_folder_capabilities(&self.connection, running, now_ms)?;
        self.folder_scopes()?
            .into_iter()
            .find(|scope| scope.id == id)
            .ok_or(StorageError::NotFound)
    }

    /// 删除范围。范围内对象停止观察，既有历史保留。
    pub fn remove_folder_scope(
        &mut self,
        id: &str,
        running: bool,
        now_ms: i64,
    ) -> Result<(), StorageError> {
        if self
            .connection
            .execute("DELETE FROM folder_scopes WHERE id=?1", [id])?
            == 0
        {
            return Err(StorageError::NotFound);
        }
        refresh_folder_capabilities(&self.connection, running, now_ms)
    }

    /// 登记范围根目录下的直接子项（文件和文件夹）。不在任何启用范围内的项被忽略；
    /// 已消失的项由可访问性检查标记，不在这里删除。返回登记的数量。
    pub fn sync_scope_items(
        &mut self,
        items: &[ScopeItem],
        running: bool,
        now_ms: i64,
    ) -> Result<usize, StorageError> {
        let scopes = enabled_scopes(&self.connection)?;
        let mut registered = 0;
        for item in items {
            if !is_scope_target(&item.path, &scopes) {
                continue;
            }
            let target = PathTarget {
                kind: item.kind,
                path: item.path.clone(),
                file_identity: item.file_identity.clone(),
            };
            if self
                .ensure_path_object(&target, item.observable, running, now_ms)?
                .is_some()
            {
                registered += 1;
            }
        }
        Ok(registered)
    }

    /// 范围 ID 对应的规范化路径。
    pub fn folder_scope_path(&self, id: &str) -> Result<Option<String>, StorageError> {
        Ok(self
            .connection
            .query_row("SELECT path FROM folder_scopes WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }
}

/// 按当前范围更新全部目录对象的观察能力。移出范围的对象改为不受支持并结束观察；
/// 重新进入范围的对象开始新的观察代次。
fn refresh_folder_capabilities(
    connection: &Connection,
    running: bool,
    now_ms: i64,
) -> Result<(), StorageError> {
    let scopes = enabled_scopes(connection)?;
    let mut statement = connection.prepare(
        "SELECT id,object_type,current_path,observation_capability FROM tracked_objects WHERE object_type IN ('folder','file')",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut updates = Vec::new();
    for row in rows {
        let (id, object_type, path, capability) = row?;
        let normalized = path.as_deref().map(normalize_local_path);
        let wanted = match normalized {
            Some(Ok(path)) if is_scope_target(&path, &scopes) => {
                let observable = object_type != ObjectType::File.as_str()
                    || file_open_observable(&desktrace_core::display_name(&path.display));
                if observable {
                    "supported"
                } else {
                    "unresolved"
                }
            }
            _ => "unsupported",
        };
        if wanted != capability {
            updates.push((id, wanted));
        }
    }
    drop(statement);
    for (id, wanted) in updates {
        if wanted == "supported" {
            // 重新进入范围的对象开始新的观察代次，旧历史保留但不计入新观察期。
            connection.execute(
                "UPDATE tracked_objects SET observation_capability='supported', observation_generation=observation_generation+1, observation_started_at=?1, updated_at=?2 WHERE id=?3",
                params![running.then_some(now_ms), now_ms, id],
            )?;
        } else {
            connection.execute(
                "UPDATE tracked_objects SET observation_capability=?1, observation_started_at=NULL, updated_at=?2 WHERE id=?3",
                params![wanted, now_ms, id],
            )?;
        }
    }
    Ok(())
}
