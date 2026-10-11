//! 白名单设置的持久化。`value_json` 只写入固定类型的 JSON 字面量。

use desktrace_core::{CollectionState, RETENTION_RANGE, Settings, THRESHOLD_RANGE};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{Storage, StorageError};

const THRESHOLD_KEY: &str = "threshold_days";
const RETENTION_KEY: &str = "event_retention_days";
const AUTO_START_KEY: &str = "auto_start_enabled";
const COLLECTION_KEY: &str = "collection_state";

impl Storage {
    /// 读取完整设置；缺失或被外部改坏的值回落到默认值。
    pub fn settings(&self) -> Result<Settings, StorageError> {
        let defaults = Settings::default();
        let threshold_days = self
            .setting_value(THRESHOLD_KEY)?
            .and_then(|value| value.parse().ok())
            .filter(|value| THRESHOLD_RANGE.contains(value))
            .unwrap_or(defaults.threshold_days);
        let event_retention_days = self
            .setting_value(RETENTION_KEY)?
            .and_then(|value| value.parse().ok())
            .filter(|value| RETENTION_RANGE.contains(value))
            .unwrap_or(defaults.event_retention_days);
        let auto_start_enabled = self
            .setting_value(AUTO_START_KEY)?
            .map_or(defaults.auto_start_enabled, |value| value == "true");
        Ok(Settings {
            threshold_days,
            event_retention_days,
            auto_start_enabled,
        })
    }

    /// 保存已校验的完整设置。
    pub fn save_settings(&mut self, settings: &Settings, now_ms: i64) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        upsert(
            &transaction,
            THRESHOLD_KEY,
            &settings.threshold_days.to_string(),
            now_ms,
        )?;
        upsert(
            &transaction,
            RETENTION_KEY,
            &settings.event_retention_days.to_string(),
            now_ms,
        )?;
        upsert(
            &transaction,
            AUTO_START_KEY,
            if settings.auto_start_enabled {
                "true"
            } else {
                "false"
            },
            now_ms,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// 读取采集意图。
    pub fn collection_state(&self) -> Result<CollectionState, StorageError> {
        Ok(self
            .setting_value(COLLECTION_KEY)?
            .map_or(CollectionState::NotStarted, |value| {
                CollectionState::parse(value.trim_matches('"'))
            }))
    }

    /// 保存采集意图。
    pub fn save_collection_state(
        &mut self,
        state: CollectionState,
        now_ms: i64,
    ) -> Result<(), StorageError> {
        upsert(
            &self.connection,
            COLLECTION_KEY,
            &format!("\"{}\"", state.as_str()),
            now_ms,
        )
    }

    /// 读取内部一次性标记，例如“已自动加入系统桌面”。
    pub fn flag(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.setting_value(&format!("flag.{key}"))?.as_deref() == Some("true"))
    }

    /// 设置内部一次性标记。
    pub fn set_flag(&mut self, key: &str, now_ms: i64) -> Result<(), StorageError> {
        upsert(&self.connection, &format!("flag.{key}"), "true", now_ms)
    }

    fn setting_value(&self, key: &str) -> Result<Option<String>, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value_json FROM settings WHERE setting_key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?)
    }
}

fn upsert(
    connection: &rusqlite::Connection,
    key: &str,
    value_json: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO settings (id,setting_key,value_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?4) ON CONFLICT(setting_key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
        params![Uuid::new_v4().to_string(), key, value_json, now_ms],
    )?;
    Ok(())
}
