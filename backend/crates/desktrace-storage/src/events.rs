use desktrace_core::{ObjectType, UsageObservation, normalize_local_path};
use rusqlite::{OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::{Storage, StorageError};

/// 当前阶段登记对象所需的最小有效观察上下文。
pub struct TrackedObjectDraft {
    /// 核心使用的 UUID。
    pub id: Uuid,
    /// 对象类型。
    pub object_type: ObjectType,
    /// 已解析的稳定身份。
    pub identity_key: String,
    /// 展示名称。
    pub display_name: String,
    /// 当前观察代次。
    pub generation: u32,
    /// 当前观察起点。
    pub observation_started_at_ms: i64,
}

/// 一次事件事务的幂等结果。
#[derive(Debug)]
pub struct RecordOutcome {
    /// 是否首次插入该分钟聚合行。
    pub inserted: bool,
    /// 更新后的最近直接观察时间。
    pub last_used_at_ms: i64,
}

struct ObservationContext {
    object_type: String,
    current_path: Option<String>,
    generation: u32,
    started_at_ms: Option<i64>,
    capability: String,
    availability: String,
}

impl Storage {
    /// 登记已经完成身份解析的可观察对象，不做模糊身份合并。
    pub fn register_object(&mut self, draft: &TrackedObjectDraft) -> Result<(), StorageError> {
        if draft.generation == 0
            || draft.observation_started_at_ms < 0
            || draft.identity_key.is_empty()
            || draft.display_name.is_empty()
        {
            return Err(StorageError::InvalidObservation);
        }
        self.connection.execute(
            "INSERT INTO tracked_objects (id,object_type,identity_key,display_name,observation_capability,first_seen_at,observation_started_at,observation_generation,created_at,updated_at) VALUES (?1,?2,?3,?4,'supported',?5,?5,?6,?5,?5)",
            params![draft.id.to_string(),draft.object_type.as_str(),draft.identity_key,draft.display_name,draft.observation_started_at_ms,draft.generation],
        )?;
        Ok(())
    }

    /// 在单个事务中合并分钟事件并维护最近时间，重试不会增加第二行。
    pub fn record_usage(
        &mut self,
        observation: &UsageObservation,
        now_ms: i64,
    ) -> Result<RecordOutcome, StorageError> {
        let dedup_key = observation.dedup_key()?;
        let transaction = self.connection.transaction()?;
        validate_context(&transaction, observation, now_ms)?;
        let existed: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_events WHERE dedup_key=?1)",
            [&dedup_key],
            |row| row.get(0),
        )?;
        upsert_usage(&transaction, observation, &dedup_key, now_ms)?;
        transaction.execute(
            "UPDATE tracked_objects SET last_used_at=MAX(COALESCE(last_used_at,?1),?1), updated_at=MAX(updated_at,?2) WHERE id=?3",
            params![observation.observed_at_ms,now_ms,observation.object_id.to_string()],
        )?;
        let last_used_at_ms = transaction.query_row(
            "SELECT last_used_at FROM tracked_objects WHERE id=?1",
            [observation.object_id.to_string()],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(RecordOutcome {
            inserted: !existed,
            last_used_at_ms,
        })
    }
}

fn validate_context(
    transaction: &Transaction<'_>,
    observation: &UsageObservation,
    now_ms: i64,
) -> Result<(), StorageError> {
    let context: Option<ObservationContext> = transaction.query_row(
        "SELECT object_type,observation_generation,observation_started_at,observation_capability,availability,current_path FROM tracked_objects WHERE id=?1",
        [observation.object_id.to_string()],
        |row| Ok(ObservationContext {
            object_type: row.get(0)?, generation: row.get(1)?, started_at_ms: row.get(2)?,
            capability: row.get(3)?, availability: row.get(4)?, current_path: row.get(5)?,
        }),
    ).optional()?;
    let context = context.ok_or(StorageError::ObjectNotObservable)?;
    let start = context
        .started_at_ms
        .ok_or(StorageError::ObjectNotObservable)?;
    if context.capability != "supported" || context.availability != "available" {
        return Err(StorageError::ObjectNotObservable);
    }
    if context.object_type != observation.kind.object_type().as_str()
        || context.generation != observation.generation
        || observation.observed_at_ms < start
        || observation.observed_at_ms > now_ms
    {
        return Err(StorageError::InvalidObservation);
    }
    // 目录还要检查子树排除，避免排除父目录后子目录事件仍被写入。
    let folder_key = (context.object_type != ObjectType::Application.as_str())
        .then(|| {
            context
                .current_path
                .as_deref()
                .and_then(|path| normalize_local_path(path).ok())
        })
        .flatten()
        .map(|path| path.key);
    if crate::objects::object_excluded(
        transaction,
        &observation.object_id.to_string(),
        folder_key.as_deref(),
    )? {
        return Err(StorageError::ObjectExcluded);
    }
    Ok(())
}

fn upsert_usage(
    transaction: &Transaction<'_>,
    observation: &UsageObservation,
    key: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO usage_events (id,object_id,event_kind,source,observed_at,observation_generation,dedup_key,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8) ON CONFLICT(dedup_key) DO UPDATE SET observed_at=MAX(usage_events.observed_at,excluded.observed_at), updated_at=MAX(usage_events.updated_at,excluded.updated_at)",
        params![Uuid::new_v4().to_string(),observation.object_id.to_string(),observation.kind.as_str(),observation.kind.source(),observation.observed_at_ms,observation.generation,key,now_ms],
    )?;
    Ok(())
}
