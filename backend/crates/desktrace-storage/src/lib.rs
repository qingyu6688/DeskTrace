//! 本地迁移、对象与事件事务、查询；连接由调用方固定在数据库工作线程。

mod catalog;
mod events;
mod exclusions;
mod intervals;
mod maintenance;
mod migration;
mod objects;
mod queries;
mod scopes;
mod settings;

#[cfg(test)]
mod feature_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

use rusqlite::Connection;
use thiserror::Error;

pub use catalog::{CatalogEntry, CatalogSyncSummary, MISSING_SCANS_BEFORE_REMOVED};
pub use events::{RecordOutcome, TrackedObjectDraft};
pub use exclusions::ExclusionRule;
pub use intervals::GapRecord;
pub use migration::{DESKTOP_FILES_MIGRATION, INITIAL_MIGRATION, LATEST_SCHEMA_VERSION};
pub use objects::{
    AppIdentity, EvidenceRecord, ExclusionMode, ExclusionRef, FolderTarget, ObjectRecord,
    ObservableObject, PathKind, PathTarget,
};
pub use queries::{
    DETAIL_EVENT_LIMIT, EventView, EvidenceView, IdentifierView, ObjectDetail, ObjectQuery,
    ObjectSummary, ObservationTrack, Overview, PageResult, SortField, TypeCounts,
    UNVERIFIED_REASON,
};
pub use scopes::{FolderScope, ScopeItem};

/// 存储及业务校验错误，不向界面暴露原始路径和数据库异常。
#[derive(Debug, Error)]
pub enum StorageError {
    /// SQLite 执行失败。
    #[error("本地数据库操作失败")]
    Database(#[from] rusqlite::Error),
    /// 事件规则错误。
    #[error(transparent)]
    Rule(#[from] desktrace_core::RuleError),
    /// 无法读到合法的观察对象。
    #[error("对象不存在或尚未开始有效观察")]
    ObjectNotObservable,
    /// 输入不属于当前观察上下文。
    #[error("事件类型、时间或代次与当前观察状态不符")]
    InvalidObservation,
    /// 该对象被明确排除。
    #[error("对象已被排除，停止记录")]
    ObjectExcluded,
    /// 当前程序不能安全打开数据库结构。
    #[error("数据库结构版本或迁移校验和不兼容")]
    MigrationMismatch,
    /// 升级前无法建立数据库快照，迁移没有执行。
    #[error("升级前无法备份本地数据库，请检查磁盘空间与权限")]
    BackupFailed,
    /// 记录不存在。
    #[error("该记录已不存在，请刷新列表")]
    NotFound,
    /// 重复的规则或状态冲突。
    #[error("该规则已存在")]
    Conflict,
    /// 观察范围路径重复。
    #[error("该文件夹已在观察范围中")]
    ScopeExists,
    /// 参数合法但动作不适用于该对象。
    #[error("此对象不支持该操作")]
    NotApplicable,
}

/// 数据库中的可复核计数。
#[derive(Debug)]
pub struct StorageSummary {
    /// 已应用的结构版本。
    pub schema_version: u32,
    /// 已登记对象数。
    pub object_count: i64,
    /// 一分钟聚合事件数。
    pub event_count: i64,
}

/// 一个工作线程独占的 SQLite 连接。
pub struct Storage {
    pub(crate) connection: Connection,
}

impl Storage {
    /// 打开本地文件，校验并执行迁移；已有数据升级前在同目录 `backups` 下留快照。目录由调用方创建。
    pub fn open(path: &Path, now_ms: i64) -> Result<Self, StorageError> {
        let backups = path.parent().map(|directory| directory.join("backups"));
        Self::initialize(Connection::open(path)?, now_ms, backups.as_deref())
    }

    /// 创建不落盘的测试数据库。
    pub fn in_memory(now_ms: i64) -> Result<Self, StorageError> {
        Self::initialize(Connection::open_in_memory()?, now_ms, None)
    }

    pub(crate) fn initialize(
        connection: Connection,
        now_ms: i64,
        backup_dir: Option<&Path>,
    ) -> Result<Self, StorageError> {
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=FULL;",
        )?;
        let mut storage = Self { connection };
        storage.apply_migrations(now_ms, backup_dir)?;
        Ok(storage)
    }

    /// 查询真实计数，不以示例数据冒充使用记录。
    pub fn summary(&self) -> Result<StorageSummary, StorageError> {
        Ok(StorageSummary {
            schema_version: self
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))?,
            object_count: self.connection.query_row(
                "SELECT COUNT(*) FROM tracked_objects",
                [],
                |row| row.get(0),
            )?,
            event_count: self.connection.query_row(
                "SELECT COUNT(*) FROM usage_events",
                [],
                |row| row.get(0),
            )?,
        })
    }
}
