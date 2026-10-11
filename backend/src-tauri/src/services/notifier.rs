//! 向界面推送刷新提示。事件只是提示，权威数据仍通过命令读取。

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::clock::now_ms;

/// `objects-updated` 最多每秒推送一次。
const OBJECTS_UPDATED_INTERVAL_MS: i64 = 1_000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectsUpdated {
    object_types: Vec<&'static str>,
    data_version: u64,
}

/// 刷新提示的节流器。
pub(crate) struct Notifier {
    app: AppHandle,
    last_emit_ms: AtomicI64,
    version: AtomicU64,
    pending: Mutex<Vec<&'static str>>,
}

impl Notifier {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self {
            app,
            last_emit_ms: AtomicI64::new(0),
            version: AtomicU64::new(0),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// 标记某类对象有变化；距上次推送不足 1 秒时合并到下一次。
    pub(crate) fn objects_changed(&self, object_type: &'static str) {
        if let Ok(mut pending) = self.pending.lock()
            && !pending.contains(&object_type)
        {
            pending.push(object_type);
        }
        self.flush(false);
    }

    /// 推送积压的变化。`force` 用于定时器补发。
    pub(crate) fn flush(&self, force: bool) {
        let now = now_ms();
        let last = self.last_emit_ms.load(Ordering::Relaxed);
        if !force && now - last < OBJECTS_UPDATED_INTERVAL_MS {
            return;
        }
        let object_types = match self.pending.lock() {
            Ok(mut pending) if !pending.is_empty() => std::mem::take(&mut *pending),
            _ => return,
        };
        self.last_emit_ms.store(now, Ordering::Relaxed);
        let data_version = self.version.fetch_add(1, Ordering::Relaxed) + 1;
        // 窗口未创建或已关闭时推送失败不影响记录。
        let _ = self.app.emit(
            "objects-updated",
            ObjectsUpdated {
                object_types,
                data_version,
            },
        );
    }

    /// 推送任意事件。
    pub(crate) fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) {
        let _ = self.app.emit(event, payload);
    }
}
