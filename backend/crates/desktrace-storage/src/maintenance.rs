//! 保留期清理与清除本工具记录，对应详细设计第 8 节。

use desktrace_core::DAY_MS;
use rusqlite::params;

use crate::{Storage, StorageError};

impl Storage {
    /// 删除超过保留天数的原始事件。最近直接时间和观察起点由对象表维护，不受影响。
    pub fn purge_expired_events(
        &mut self,
        retention_days: u32,
        now_ms: i64,
    ) -> Result<usize, StorageError> {
        let cutoff = now_ms - i64::from(retention_days) * DAY_MS;
        Ok(self
            .connection
            .execute("DELETE FROM usage_events WHERE observed_at<?1", [cutoff])?)
    }

    /// 有使用记录的对象数，用于清除前的影响说明。
    pub fn objects_with_usage(&self) -> Result<i64, StorageError> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM tracked_objects WHERE last_used_at IS NOT NULL OR EXISTS(SELECT 1 FROM usage_events e WHERE e.object_id=tracked_objects.id)",
            [],
            |row| row.get(0),
        )?)
    }

    /// 当前数据代次：所有对象代次之和加事件数。清除凭据绑定该值，数据变化后凭据失效。
    pub fn data_revision(&self) -> Result<i64, StorageError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE((SELECT SUM(observation_generation) FROM tracked_objects),0) + (SELECT COUNT(*) FROM usage_events)",
            [],
            |row| row.get(0),
        )?)
    }

    /// 清除本工具记录：事件、最近时间与采集区间。观察范围、排除规则、清单身份与日期证据保留；
    /// 每个对象进入新代次，正在记录时从当前时刻重新开始观察。
    pub fn clear_usage_data(&mut self, running: bool, now_ms: i64) -> Result<usize, StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM usage_events", [])?;
        transaction.execute("DELETE FROM collector_intervals", [])?;
        let objects = transaction.execute(
            "UPDATE tracked_objects SET last_used_at=NULL, observation_generation=observation_generation+1, observation_started_at=CASE WHEN ?1 AND observation_capability='supported' AND availability='available' THEN ?2 ELSE NULL END, updated_at=?2",
            params![running, now_ms],
        )?;
        transaction.commit()?;
        Ok(objects)
    }
}
