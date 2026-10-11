//! 后台任务（清单刷新、CSV 导出）的状态登记与进度推送。
//! 界面重开后仍可用任务 ID 查询；事件只是提示，权威状态通过 `get_job_status` 读取。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use uuid::Uuid;

use super::notifier::Notifier;

/// 已结束任务最多保留的条数。
const FINISHED_JOBS_KEPT: usize = 20;

/// 任务类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobKind {
    /// 应用清单刷新。
    CatalogRefresh,
    /// CSV 导出。
    Export,
}

/// 任务状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    fn finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// 任务快照，有总量才返回总量，不伪造百分比。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobSnapshot {
    pub(crate) job_id: String,
    pub(crate) kind: JobKind,
    pub(crate) status: JobStatus,
    pub(crate) stage: String,
    pub(crate) processed: u64,
    pub(crate) total: Option<u64>,
    pub(crate) message: Option<String>,
    pub(crate) result_path: Option<String>,
}

struct Entry {
    snapshot: JobSnapshot,
    cancel: Arc<AtomicBool>,
    order: u64,
}

/// 任务登记表。
pub(crate) struct JobRegistry {
    entries: Mutex<HashMap<String, Entry>>,
    notifier: Arc<Notifier>,
    counter: std::sync::atomic::AtomicU64,
}

/// 任务执行方持有的句柄。
#[derive(Clone)]
pub(crate) struct JobHandle {
    pub(crate) id: String,
    cancel: Arc<AtomicBool>,
    registry: Arc<JobRegistry>,
}

impl JobRegistry {
    pub(crate) fn new(notifier: Arc<Notifier>) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
            notifier,
            counter: Default::default(),
        })
    }

    /// 登记新任务。清单刷新同一时间只运行一个，已有任务时返回 `Err(已有任务 ID)`。
    pub(crate) fn start(self: &Arc<Self>, kind: JobKind, stage: &str) -> Result<JobHandle, String> {
        let mut entries = self.entries.lock().map_err(|_| String::new())?;
        if kind == JobKind::CatalogRefresh
            && let Some(running) = entries
                .values()
                .find(|entry| entry.snapshot.kind == kind && !entry.snapshot.status.finished())
        {
            return Err(running.snapshot.job_id.clone());
        }
        let id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let snapshot = JobSnapshot {
            job_id: id.clone(),
            kind,
            status: JobStatus::Pending,
            stage: stage.into(),
            processed: 0,
            total: None,
            message: None,
            result_path: None,
        };
        let order = self.counter.fetch_add(1, Ordering::Relaxed);
        entries.insert(
            id.clone(),
            Entry {
                snapshot: snapshot.clone(),
                cancel: cancel.clone(),
                order,
            },
        );
        prune(&mut entries);
        drop(entries);
        self.notifier.emit("job-progress", snapshot);
        Ok(JobHandle {
            id,
            cancel,
            registry: self.clone(),
        })
    }

    pub(crate) fn get(&self, id: &str) -> Option<JobSnapshot> {
        self.entries
            .lock()
            .ok()?
            .get(id)
            .map(|entry| entry.snapshot.clone())
    }

    /// 请求取消。已结束的任务原样返回。
    pub(crate) fn cancel(&self, id: &str) -> Option<JobSnapshot> {
        let entries = self.entries.lock().ok()?;
        let entry = entries.get(id)?;
        if !entry.snapshot.status.finished() {
            entry.cancel.store(true, Ordering::Relaxed);
        }
        Some(entry.snapshot.clone())
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut JobSnapshot)) {
        let snapshot = {
            let Ok(mut entries) = self.entries.lock() else {
                return;
            };
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            if entry.snapshot.status.finished() {
                return;
            }
            change(&mut entry.snapshot);
            entry.snapshot.clone()
        };
        self.notifier.emit("job-progress", snapshot);
    }
}

impl JobHandle {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub(crate) fn progress(&self, stage: &str, processed: u64, total: Option<u64>) {
        self.registry.update(&self.id, |snapshot| {
            snapshot.status = JobStatus::Running;
            snapshot.stage = stage.into();
            snapshot.processed = processed;
            snapshot.total = total;
        });
    }

    pub(crate) fn complete(&self, message: String, result_path: Option<String>) {
        self.registry.update(&self.id, |snapshot| {
            snapshot.status = JobStatus::Completed;
            snapshot.stage = "完成".into();
            snapshot.message = Some(message);
            snapshot.result_path = result_path;
        });
    }

    pub(crate) fn fail(&self, message: String) {
        self.registry.update(&self.id, |snapshot| {
            snapshot.status = JobStatus::Failed;
            snapshot.message = Some(message);
        });
    }

    pub(crate) fn cancelled(&self) {
        self.registry.update(&self.id, |snapshot| {
            snapshot.status = JobStatus::Cancelled;
            snapshot.stage = "已取消".into();
        });
    }
}

fn prune(entries: &mut HashMap<String, Entry>) {
    let mut finished: Vec<(u64, String)> = entries
        .iter()
        .filter(|(_, entry)| entry.snapshot.status.finished())
        .map(|(id, entry)| (entry.order, id.clone()))
        .collect();
    if finished.len() <= FINISHED_JOBS_KEPT {
        return;
    }
    finished.sort_unstable();
    for (_, id) in finished.iter().take(finished.len() - FINISHED_JOBS_KEPT) {
        entries.remove(id);
    }
}
