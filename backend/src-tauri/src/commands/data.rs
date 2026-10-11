use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use desktrace_core::{CollectionState, ObjectType, format_date, local_day};
use desktrace_storage::StorageError;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use super::objects::parse_query;
use super::scopes::owner_window;
use super::{CommandResult, blocking, fail, ok, respond, service_failure};
use crate::services::clock::{now_ms, offset_minutes};
use crate::services::csv::{self, BOM, header, row};
use crate::services::jobs::{JobHandle, JobKind, JobSnapshot};
use crate::services::storage::ServiceError;
use crate::services::tokens::TOKEN_TTL;
use crate::state::AppState;

/// 导出时每写多少行检查一次取消并推送进度。
const EXPORT_PROGRESS_ROWS: usize = 200;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportStarted {
    job_id: Option<String>,
    cancelled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClearPreparation {
    confirmation_token: String,
    affected_objects: i64,
    expires_in_seconds: u64,
}

#[derive(Serialize)]
pub(crate) struct Cleared {
    cleared: bool,
}

/// 弹出系统保存对话框，确认后在后台导出当前筛选的全部结果。用户取消时 `cancelled` 为真。
#[tauri::command]
pub(crate) async fn request_export(
    app: AppHandle,
    window: WebviewWindow,
    query: serde_json::Value,
    confirmed_fields: Vec<String>,
) -> CommandResult<ExportStarted> {
    let query = match parse_query(query) {
        Ok(query) => query,
        Err(message) => return Ok(fail(400, message)),
    };
    let Some(fields) = csv::confirmed_fields(&confirmed_fields) else {
        return Ok(fail(400, "导出字段不正确"));
    };
    let now = now_ms();
    let kind = match query.object_types.as_slice() {
        [ObjectType::Application] => "应用",
        [ObjectType::Folder] => "文件夹",
        [ObjectType::File] => "文件",
        _ => "文件与文件夹",
    };
    let default_name = format!(
        "DeskTrace-{kind}-{}.csv",
        format_date(local_day(now, offset_minutes(now))).replace('-', "")
    );
    let owner = owner_window(&window);
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        desktrace_windows::save_csv_dialog(owner, &default_name)
    })
    .await;
    let target = match chosen {
        Ok(Ok(Some(path))) => PathBuf::from(path),
        Ok(Ok(None)) => {
            return Ok(ok(
                "已取消导出",
                ExportStarted {
                    job_id: None,
                    cancelled: true,
                },
            ));
        }
        _ => return Ok(fail(500, "无法打开保存窗口")),
    };
    let state = app.state::<AppState>();
    let job = match state.jobs.start(JobKind::Export, "准备导出") {
        Ok(job) => job,
        Err(_) => return Ok(fail(409, "已有导出任务在进行")),
    };
    let job_id = job.id.clone();
    let worker_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("desktrace-export".into())
        .spawn(move || {
            let Some(state) = worker_app.try_state::<AppState>() else {
                return;
            };
            let items = match state
                .storage
                .call(move |storage| storage.matching_objects(&query, now))
            {
                Ok(items) => items,
                Err(_) => return job.fail("读取导出数据失败".into()),
            };
            match write_csv(&target, &items, &fields, &job) {
                Ok(true) => job.complete(
                    format!("已导出 {} 项", items.len()),
                    Some(target.to_string_lossy().into_owned()),
                ),
                Ok(false) => job.cancelled(),
                Err(_) => job.fail("写入文件失败，请检查保存位置".into()),
            }
        });
    if spawned.is_err() {
        return Ok(fail(500, "无法启动导出任务"));
    }
    Ok(ok(
        "已开始导出",
        ExportStarted {
            job_id: Some(job_id),
            cancelled: false,
        },
    ))
}

/// 先写同目录的临时文件，完成后再替换目标；取消或失败时删除临时文件，不留下半个文件。
/// 返回 `Ok(false)` 表示被取消。
fn write_csv(
    target: &std::path::Path,
    items: &[desktrace_storage::ObjectSummary],
    fields: &[&str],
    job: &JobHandle,
) -> std::io::Result<bool> {
    let temporary = target.with_extension("csv.partial");
    let outcome = (|| -> std::io::Result<bool> {
        let mut writer = BufWriter::new(File::create(&temporary)?);
        writer.write_all(BOM.as_bytes())?;
        writer.write_all(header(fields).as_bytes())?;
        let total = items.len() as u64;
        for (index, item) in items.iter().enumerate() {
            if index % EXPORT_PROGRESS_ROWS == 0 {
                if job.is_cancelled() {
                    return Ok(false);
                }
                job.progress("写入文件", index as u64, Some(total));
            }
            writer.write_all(row(item, fields, &offset_minutes).as_bytes())?;
        }
        writer.flush()?;
        Ok(true)
    })();
    match outcome {
        Ok(true) => {
            std::fs::rename(&temporary, target)?;
            Ok(true)
        }
        other => {
            let _ = std::fs::remove_file(&temporary);
            other
        }
    }
}

#[tauri::command]
pub(crate) async fn get_job_status(
    state: State<'_, AppState>,
    job_id: String,
) -> CommandResult<JobSnapshot> {
    Ok(match state.jobs.get(&job_id) {
        Some(snapshot) => ok("查询成功", snapshot),
        None => fail(404, "任务不存在"),
    })
}

/// 取消只读扫描或导出，未完成的临时文件会被删除。
#[tauri::command]
pub(crate) async fn cancel_job(
    state: State<'_, AppState>,
    job_id: String,
) -> CommandResult<JobSnapshot> {
    Ok(match state.jobs.cancel(&job_id) {
        Some(snapshot) => ok("已请求取消", snapshot),
        None => fail(404, "任务不存在"),
    })
}

/// 生成清除凭据，绑定当前数据代次，60 秒内有效。
#[tauri::command]
pub(crate) async fn prepare_clear_usage_data(app: AppHandle) -> CommandResult<ClearPreparation> {
    blocking(move || {
        let state = app.state::<AppState>();
        match state
            .storage
            .call(|storage| Ok((storage.data_revision()?, storage.objects_with_usage()?)))
        {
            Ok((revision, affected)) => ok(
                "请确认清除",
                ClearPreparation {
                    confirmation_token: state.tokens.issue_clear(revision),
                    affected_objects: affected,
                    expires_in_seconds: TOKEN_TTL.as_secs(),
                },
            ),
            Err(error) => service_failure(error),
        }
    })
    .await
}

/// 用确认凭据清除本工具的使用记录并重新开始观察。数据在确认后有变化时返回 409。
#[tauri::command]
pub(crate) async fn clear_usage_data(
    app: AppHandle,
    confirmation_token: String,
) -> CommandResult<Cleared> {
    let state = app.state::<AppState>();
    let Some(revision) = state.tokens.take_clear(&confirmation_token) else {
        return Ok(fail(409, "确认已过期，请重新确认"));
    };
    let storage = state.storage.clone();
    let now = now_ms();
    let result = blocking(move || {
        let result = storage.call(move |storage| {
            if storage.data_revision()? != revision {
                return Err(StorageError::Conflict);
            }
            let running = storage.collection_state()? == CollectionState::Running;
            storage.clear_usage_data(running, now)
        });
        match result {
            Err(ServiceError::Storage(StorageError::Conflict)) => {
                fail(409, "数据状态已变化，请重新确认")
            }
            other => respond(other.map(|_| Cleared { cleared: true }), "已清除使用记录"),
        }
    })
    .await?;
    if result.is_success() {
        state.collector.reset();
        state.notifier.objects_changed("application");
        state.notifier.objects_changed("folder");
    }
    Ok(result)
}
