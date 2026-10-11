//! 分模块采集区间的切换、心跳与异常退出恢复，对应详细设计第 7 节。

use desktrace_core::{
    CollectorModule, HEARTBEAT_TOLERANCE_MS, IntervalReason, IntervalSpan, IntervalState,
    format_utc,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;
use uuid::Uuid;

use crate::{Storage, StorageError};

/// 展示用的一段缺口区间。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GapRecord {
    /// 所属模块。
    pub module: CollectorModule,
    /// `gap` 或 `unknown`。
    pub state: IntervalState,
    /// 原因枚举值。
    pub reason: &'static str,
    /// 中文说明。
    pub reason_label: &'static str,
    /// 开始时刻（UTC 毫秒）。
    #[serde(skip)]
    pub started_at_ms: i64,
    /// 结束时刻，进行中为空。
    #[serde(skip)]
    pub ended_at_ms: Option<i64>,
    /// 开始时刻，UTC RFC 3339。
    pub started_at: String,
    /// 结束时刻，进行中为空。
    pub ended_at: Option<String>,
}

struct OpenInterval {
    id: String,
    reason: String,
    verified_until: i64,
}

impl Storage {
    /// 切换模块状态：同原因只推进心跳；不同原因在一个事务中关闭旧区间并开启新区间。
    pub fn transition_interval(
        &mut self,
        module: CollectorModule,
        reason: IntervalReason,
        at_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        if let Some(open) = open_interval(&transaction, module)? {
            if open.reason == reason.as_str() && within_tolerance(&open, at_ms) {
                advance(&transaction, &open, at_ms)?;
                transaction.commit()?;
                return Ok(());
            }
            close(&transaction, &open, at_ms)?;
        }
        insert(&transaction, module, reason, at_ms, at_ms, None)?;
        transaction.commit()?;
        Ok(())
    }

    /// 推进开放区间的已验证时刻，不会倒退。距上次心跳超过容差（例如进程被系统挂起）时，
    /// 关闭旧区间并以相同原因重开，中间留下未验证空档，而不是把整段时间算作已验证。
    pub fn heartbeat(&mut self, module: CollectorModule, at_ms: i64) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        let Some(open) = open_interval(&transaction, module)? else {
            return Ok(());
        };
        if at_ms <= open.verified_until {
            return Ok(());
        }
        if within_tolerance(&open, at_ms) {
            advance(&transaction, &open, at_ms)?;
        } else if let Some(reason) = IntervalReason::parse(&open.reason) {
            close(&transaction, &open, at_ms)?;
            insert(&transaction, module, reason, at_ms, at_ms, None)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// 启动时处理上次运行遗留的开放区间：正常退出的区间关闭到当前时刻；
    /// 其他区间只认到最后心跳，之后补一段 `unexpected_exit`，不补造使用历史。
    pub fn recover_open_intervals(&mut self, now_ms: i64) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        for module in CollectorModule::ALL {
            let Some(open) = open_interval(&transaction, module)? else {
                continue;
            };
            if open.reason == IntervalReason::CollectorStopped.as_str() {
                transaction.execute(
                    "UPDATE collector_intervals SET ended_at=MAX(?1,verified_until), updated_at=?1 WHERE id=?2",
                    params![now_ms, open.id],
                )?;
                continue;
            }
            transaction.execute(
                "UPDATE collector_intervals SET ended_at=verified_until, updated_at=?1 WHERE id=?2",
                params![now_ms, open.id],
            )?;
            if now_ms > open.verified_until {
                insert(
                    &transaction,
                    module,
                    IntervalReason::UnexpectedExit,
                    open.verified_until,
                    open.verified_until,
                    Some(now_ms),
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// 读取模块的全部区间，用于覆盖计算。
    pub fn interval_spans(
        &self,
        module: CollectorModule,
    ) -> Result<Vec<IntervalSpan>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT interval_state,started_at,verified_until,ended_at FROM collector_intervals WHERE module=?1 ORDER BY started_at",
        )?;
        let rows = statement.query_map([module.as_str()], |row| {
            let state: String = row.get(0)?;
            Ok(IntervalSpan {
                state: IntervalState::parse(&state),
                started_at_ms: row.get(1)?,
                verified_until_ms: row.get(2)?,
                ended_at_ms: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 读取与时间段相交的缺口与未知区间，新的在前。`module` 为空时读取全部模块。
    pub fn gaps(
        &self,
        module: Option<CollectorModule>,
        from_ms: i64,
        to_ms: i64,
        limit: u32,
    ) -> Result<Vec<GapRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT module,reason,started_at,ended_at FROM collector_intervals WHERE interval_state IN ('gap','unknown') AND (?1 IS NULL OR module=?1) AND started_at<?3 AND (ended_at IS NULL OR ended_at>?2) ORDER BY started_at DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![module.map(CollectorModule::as_str), from_ms, to_ms, limit],
            |row| {
                let module: String = row.get(0)?;
                let reason: String = row.get(1)?;
                Ok((
                    module,
                    reason,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )?;
        let mut gaps = Vec::new();
        for row in rows {
            let (module, reason, started_at_ms, ended_at_ms) = row?;
            let Some(reason) = IntervalReason::parse(&reason) else {
                continue;
            };
            gaps.push(GapRecord {
                module: CollectorModule::parse(&module).unwrap_or(CollectorModule::Applications),
                state: reason.state(),
                reason: reason.as_str(),
                reason_label: reason.label(),
                started_at_ms,
                ended_at_ms,
                started_at: format_utc(started_at_ms),
                ended_at: ended_at_ms.map(format_utc),
            });
        }
        Ok(gaps)
    }

    /// 模块当前开放区间的原因；没有开放区间时为空。
    pub fn open_interval_reason(
        &self,
        module: CollectorModule,
    ) -> Result<Option<IntervalReason>, StorageError> {
        let reason: Option<String> = self
            .connection
            .query_row(
                "SELECT reason FROM collector_intervals WHERE module=?1 AND ended_at IS NULL",
                [module.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(reason.as_deref().and_then(IntervalReason::parse))
    }

    /// 最早一次开始记录的时刻，作为概览中的观察起点。
    pub fn first_collection_start(&self) -> Result<Option<i64>, StorageError> {
        Ok(self.connection.query_row(
            "SELECT MIN(started_at) FROM collector_intervals WHERE reason='collection_started'",
            [],
            |row| row.get(0),
        )?)
    }
}

fn open_interval(
    transaction: &Transaction<'_>,
    module: CollectorModule,
) -> Result<Option<OpenInterval>, StorageError> {
    Ok(transaction
        .query_row(
            "SELECT id,reason,verified_until FROM collector_intervals WHERE module=?1 AND ended_at IS NULL",
            [module.as_str()],
            |row| Ok(OpenInterval { id: row.get(0)?, reason: row.get(1)?, verified_until: row.get(2)? }),
        )
        .optional()?)
}

fn within_tolerance(open: &OpenInterval, at_ms: i64) -> bool {
    at_ms - open.verified_until <= HEARTBEAT_TOLERANCE_MS
}

fn advance(
    transaction: &Transaction<'_>,
    open: &OpenInterval,
    at_ms: i64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE collector_intervals SET verified_until=MAX(verified_until,?1), updated_at=?1 WHERE id=?2",
        params![at_ms, open.id],
    )?;
    Ok(())
}

/// 关闭开放区间。只有最后心跳仍在容差内时才把已验证时刻推进到切换点，
/// 否则保留空档，让覆盖计算把它当作未验证。
fn close(
    transaction: &Transaction<'_>,
    open: &OpenInterval,
    at_ms: i64,
) -> Result<(), StorageError> {
    let ended = at_ms.max(open.verified_until);
    let verified = if ended - open.verified_until <= HEARTBEAT_TOLERANCE_MS {
        ended
    } else {
        open.verified_until
    };
    transaction.execute(
        "UPDATE collector_intervals SET verified_until=?1, ended_at=?2, updated_at=?2 WHERE id=?3",
        params![verified, ended, open.id],
    )?;
    Ok(())
}

fn insert(
    transaction: &Transaction<'_>,
    module: CollectorModule,
    reason: IntervalReason,
    started_at_ms: i64,
    verified_until_ms: i64,
    ended_at_ms: Option<i64>,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO collector_intervals (id,module,interval_state,reason,started_at,verified_until,ended_at,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
        params![
            Uuid::new_v4().to_string(),
            module.as_str(),
            reason.state().as_str(),
            reason.as_str(),
            started_at_ms,
            verified_until_ms,
            ended_at_ms,
            ended_at_ms.unwrap_or(started_at_ms)
        ],
    )?;
    Ok(())
}
