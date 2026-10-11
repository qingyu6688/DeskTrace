//! 按版本依次执行的数据库迁移。已发布的迁移不可回改：已应用迁移的校验和不一致时拒绝打开。

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::{Storage, StorageError};

/// 已发布后不可回改的首个数据库结构。
pub const INITIAL_MIGRATION: &str = include_str!("../../../migrations/0001_initial.sql");
/// 文件对象与“最近使用的项目”来源。
pub const DESKTOP_FILES_MIGRATION: &str =
    include_str!("../../../migrations/0002_desktop_files.sql");
/// 当前程序支持的结构版本。
pub const LATEST_SCHEMA_VERSION: u32 = 2;

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
    /// 需要重建表的迁移要在事务外关闭外键检查，提交前再做一致性检查。
    rebuilds_tables: bool,
}

const MIGRATIONS: [Migration; 2] = [
    Migration {
        version: 1,
        name: "0001_initial",
        sql: INITIAL_MIGRATION,
        rebuilds_tables: false,
    },
    Migration {
        version: 2,
        name: "0002_desktop_files",
        sql: DESKTOP_FILES_MIGRATION,
        rebuilds_tables: true,
    },
];

fn checksum(sql: &str) -> String {
    Sha256::digest(sql.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Storage {
    /// 校验已应用的迁移并执行剩余迁移。已有数据的库升级前在 `backup_dir` 留一份一致快照。
    pub(crate) fn apply_migrations(
        &mut self,
        now_ms: i64,
        backup_dir: Option<&Path>,
    ) -> Result<(), StorageError> {
        let version: u32 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > LATEST_SCHEMA_VERSION {
            return Err(StorageError::MigrationMismatch);
        }
        for migration in MIGRATIONS
            .iter()
            .filter(|migration| migration.version <= version)
        {
            let saved: Option<String> = self
                .connection
                .query_row(
                    "SELECT checksum FROM schema_migrations WHERE id=?1",
                    [migration.version],
                    |row| row.get(0),
                )
                .optional()?;
            if saved.as_deref() != Some(checksum(migration.sql).as_str()) {
                return Err(StorageError::MigrationMismatch);
            }
        }
        if version == LATEST_SCHEMA_VERSION {
            return Ok(());
        }
        if version > 0
            && let Some(directory) = backup_dir
        {
            backup(&self.connection, directory, version, now_ms)?;
        }
        for migration in MIGRATIONS
            .iter()
            .filter(|migration| migration.version > version)
        {
            apply(&mut self.connection, migration, now_ms)?;
        }
        Ok(())
    }
}

/// 用 `VACUUM INTO` 生成一致快照，运行中的 WAL 内容也包含在内。
fn backup(
    connection: &Connection,
    directory: &Path,
    version: u32,
    now_ms: i64,
) -> Result<(), StorageError> {
    std::fs::create_dir_all(directory).map_err(|_| StorageError::BackupFailed)?;
    let target = directory.join(format!("desktrace-v{version}-{now_ms}.sqlite3"));
    connection
        .execute("VACUUM INTO ?1", [target.to_string_lossy()])
        .map_err(|_| StorageError::BackupFailed)?;
    Ok(())
}

fn apply(
    connection: &mut Connection,
    migration: &Migration,
    now_ms: i64,
) -> Result<(), StorageError> {
    if migration.rebuilds_tables {
        connection.pragma_update(None, "foreign_keys", false)?;
    }
    let result = (|| -> Result<(), StorageError> {
        let transaction = connection.transaction()?;
        transaction.execute_batch(migration.sql)?;
        if migration.rebuilds_tables {
            let violations: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check",
                [],
                |row| row.get(0),
            )?;
            if violations > 0 {
                return Err(StorageError::MigrationMismatch);
            }
        }
        transaction.execute(
            "INSERT INTO schema_migrations (id,name,checksum,applied_at,created_at,updated_at) VALUES (?1,?2,?3,?4,?4,?4)",
            params![migration.version, migration.name, checksum(migration.sql), now_ms],
        )?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        transaction.commit()?;
        Ok(())
    })();
    if migration.rebuilds_tables {
        connection.pragma_update(None, "foreign_keys", true)?;
    }
    result
}
