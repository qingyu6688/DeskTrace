//! 受限 IPC 命令。返回统一的 `code/message/data`；所有参数在 Rust 端再次校验，
//! 不提供任意路径、任意 SQL 或任意系统命令。

pub(crate) mod actions;
pub(crate) mod bootstrap;
pub(crate) mod collection;
pub(crate) mod data;
pub(crate) mod exclusions;
pub(crate) mod objects;
pub(crate) mod scopes;
pub(crate) mod settings;

use desktrace_storage::StorageError;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::services::storage::ServiceError;

/// 统一响应。错误码是应用错误码，不是 HTTP 状态。
#[derive(Serialize)]
pub(crate) struct ApiResponse<T> {
    code: u16,
    message: String,
    data: Option<T>,
}

/// Tauri 异步命令借用 `State` 时要求返回 `Result`；业务错误仍放在统一响应里，`Err` 不会出现。
pub(crate) type CommandResult<T> = Result<ApiResponse<T>, String>;

impl<T> ApiResponse<T> {
    pub(crate) fn is_success(&self) -> bool {
        self.code == 200
    }
}

pub(crate) fn ok<T>(message: &str, data: T) -> ApiResponse<T> {
    ApiResponse {
        code: 200,
        message: message.into(),
        data: Some(data),
    }
}

pub(crate) fn fail<T>(code: u16, message: impl Into<String>) -> ApiResponse<T> {
    ApiResponse {
        code,
        message: message.into(),
        data: None,
    }
}

/// 把服务错误转换为用户可读的响应，不暴露数据库原始信息。
pub(crate) fn service_failure<T>(error: ServiceError) -> ApiResponse<T> {
    match error {
        ServiceError::Unavailable(message) => fail(503, message),
        ServiceError::Stopped => fail(503, "本地数据库暂不可用，请稍后重试"),
        ServiceError::Storage(error) => match error {
            StorageError::NotFound => fail(404, error.to_string()),
            StorageError::Conflict | StorageError::ScopeExists => fail(409, error.to_string()),
            StorageError::NotApplicable => fail(422, error.to_string()),
            StorageError::ObjectExcluded
            | StorageError::InvalidObservation
            | StorageError::ObjectNotObservable => fail(409, "数据状态已变化，请刷新后重试"),
            StorageError::Rule(_) => fail(400, error.to_string()),
            StorageError::MigrationMismatch | StorageError::BackupFailed => {
                fail(503, error.to_string())
            }
            StorageError::Database(_) => fail(500, "保存失败，请查看本地诊断信息"),
        },
    }
}

pub(crate) fn respond<T>(result: Result<T, ServiceError>, message: &str) -> ApiResponse<T> {
    match result {
        Ok(data) => ok(message, data),
        Err(error) => service_failure(error),
    }
}

/// 在阻塞线程池中执行，避免数据库或系统对话框阻塞异步运行时。
pub(crate) async fn blocking<T, F>(task: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ApiResponse<T> + Send + 'static,
{
    Ok(tauri::async_runtime::spawn_blocking(task)
        .await
        .unwrap_or_else(|_| fail(500, "操作失败，请重试")))
}

/// 按严格结构解析参数，未知字段或类型不符返回 400。
pub(crate) fn parse<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| "参数格式不正确".to_string())
}
