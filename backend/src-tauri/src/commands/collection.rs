use desktrace_core::{
    CollectionState, local_day, normalize_local_path, package_install_date,
    parse_registry_install_date,
};
use desktrace_storage::CatalogEntry;
use desktrace_windows::{CatalogScan, read_catalog};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::{CommandResult, fail, ok, service_failure};
use crate::collector::CollectorStatus;
use crate::services::clock::{now_ms, offset_minutes};
use crate::services::jobs::JobKind;
use crate::state::{AppState, apply_collection_state};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JobStarted {
    job_id: String,
}

/// 两个模块的采集状态、清单刷新情况与已知限制。
#[tauri::command]
pub(crate) async fn get_collector_status(
    state: State<'_, AppState>,
) -> CommandResult<CollectorStatus> {
    Ok(ok("查询成功", state.collector.status()))
}

/// 启用、暂停或恢复记录。重复请求幂等。
#[tauri::command]
pub(crate) async fn set_collection_state(
    app: AppHandle,
    state: String,
) -> CommandResult<CollectorStatus> {
    let next = match state.as_str() {
        "running" => CollectionState::Running,
        "paused" => CollectionState::Paused,
        _ => return Ok(fail(400, "采集状态只能是 running 或 paused")),
    };
    let handle = app.clone();
    let applied =
        tauri::async_runtime::spawn_blocking(move || apply_collection_state(&handle, next)).await;
    Ok(match applied {
        Ok(Ok(collection)) => {
            let mut status = app.state::<AppState>().collector.status();
            status.collection_state = collection;
            ok(
                if collection == CollectionState::Running {
                    "已开始记录"
                } else {
                    "已暂停记录"
                },
                status,
            )
        }
        Ok(Err(error)) => service_failure(error),
        Err(_) => fail(500, "操作失败，请重试"),
    })
}

/// 异步刷新应用清单；已有刷新任务时返回同一个任务 ID。
#[tauri::command]
pub(crate) async fn refresh_application_catalog(app: AppHandle) -> CommandResult<JobStarted> {
    Ok(ok(
        "已开始刷新应用清单",
        JobStarted {
            job_id: start_catalog_refresh(&app),
        },
    ))
}

/// 启动清单刷新任务并返回任务 ID。
pub(crate) fn start_catalog_refresh(app: &AppHandle) -> String {
    let Some(state) = app.try_state::<AppState>() else {
        return String::new();
    };
    let job = match state.jobs.start(JobKind::CatalogRefresh, "读取应用清单") {
        Ok(job) => job,
        Err(existing) => return existing,
    };
    let job_id = job.id.clone();
    state
        .collector
        .update_catalog(|catalog| catalog.running_job_id = Some(job_id.clone()));
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("desktrace-catalog".into())
        .spawn(move || {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            job.progress("读取注册表与应用包", 0, None);
            let scan = read_catalog(&job.cancel_flag());
            if scan.cancelled {
                job.cancelled();
                state
                    .collector
                    .update_catalog(|catalog| catalog.running_job_id = None);
                return;
            }
            let now = now_ms();
            let entries = catalog_entries(&scan, now);
            let total = entries.len() as u64;
            job.progress("保存清单", 0, Some(total));
            let complete = scan.complete;
            let result = state.storage.call(move |storage| {
                let running = storage.collection_state()? == CollectionState::Running;
                storage.sync_catalog(&entries, complete, running, now)
            });
            state.collector.update_catalog(|catalog| {
                catalog.running_job_id = None;
                if result.is_ok() {
                    catalog.last_refreshed_at = Some(desktrace_core::format_utc(now));
                    catalog.complete = Some(complete);
                }
            });
            match result {
                Ok(summary) => {
                    state.notifier.objects_changed("application");
                    let partial = if complete {
                        ""
                    } else {
                        "；部分来源读取失败，本次不判断卸载"
                    };
                    job.complete(
                        format!(
                            "新增 {} 个，刷新 {} 个，标记失联 {} 个{partial}",
                            summary.added, summary.refreshed, summary.removed
                        ),
                        None,
                    );
                }
                Err(_) => job.fail("清单保存失败，请稍后重试".into()),
            }
        });
    if spawned.is_err() {
        state
            .collector
            .update_catalog(|catalog| catalog.running_job_id = None);
    }
    job_id
}

/// 把系统清单转换为存储层记录：日期按严格规则解析，主程序路径必须能规范化。
fn catalog_entries(scan: &CatalogScan, now: i64) -> Vec<CatalogEntry> {
    let today = local_day(now, offset_minutes(now));
    let win32 = scan.win32.iter().map(|app| CatalogEntry::Win32 {
        uninstall_key: app.uninstall_key.clone(),
        display_name: app.display_name.clone(),
        publisher: app.publisher.clone(),
        version: app.version.clone(),
        install_date: parse_registry_install_date(
            app.install_date_raw.as_deref(),
            app.is_msi,
            today,
        ),
        entry_executable: app
            .entry_executable
            .as_deref()
            .and_then(|path| normalize_local_path(path).ok()),
    });
    let packages = scan.packages.iter().map(|app| CatalogEntry::Package {
        aumid: app.aumid.clone(),
        display_name: app.display_name.clone(),
        publisher: app.publisher.clone(),
        version: app.version.clone(),
        install_date: package_install_date(app.installed_at_ms, now),
    });
    win32.chain(packages).collect()
}
