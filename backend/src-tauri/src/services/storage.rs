//! 唯一的数据库工作线程。所有读写都以闭包形式串行送入，不在界面线程或系统回调里访问数据库。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use desktrace_storage::{Storage, StorageError};

use super::clock::now_ms;

/// 等待数据库响应的上限。导出等长任务在自己的线程里分批调用，单次调用不会超过这个时间。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

type Job = Box<dyn FnOnce(Result<&mut Storage, &str>) + Send>;

/// 数据库服务调用失败的原因。
#[derive(Debug)]
pub(crate) enum ServiceError {
    /// 数据库没能打开，附用户可读的说明。
    Unavailable(String),
    /// 线程已停止或响应超时。
    Stopped,
    /// 存储层错误。
    Storage(StorageError),
}

impl From<StorageError> for ServiceError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

/// 数据库服务句柄，可在线程间克隆。
#[derive(Clone)]
pub(crate) struct StorageService {
    sender: Sender<Job>,
    directory: PathBuf,
}

impl StorageService {
    /// 启动工作线程并打开数据库；打开失败时线程继续运行，所有调用返回 `Unavailable`。
    pub(crate) fn start(directory: PathBuf) -> Result<Self, std::io::Error> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let worker_directory = directory.clone();
        std::thread::Builder::new()
            .name("desktrace-storage".into())
            .spawn(move || {
                let mut storage = open_storage(&worker_directory);
                for job in receiver {
                    match &mut storage {
                        Ok(storage) => job(Ok(storage)),
                        Err(message) => job(Err(message.as_str())),
                    }
                }
            })?;
        Ok(Self { sender, directory })
    }

    /// 本地数据目录。
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// 在数据库线程上执行闭包并等待结果。
    pub(crate) fn call<T, F>(&self, task: F) -> Result<T, ServiceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Storage) -> Result<T, StorageError> + Send + 'static,
    {
        let (reply, response) = mpsc::channel();
        let job: Job = Box::new(move |storage| {
            let result = match storage {
                Ok(storage) => task(storage).map_err(ServiceError::Storage),
                Err(message) => Err(ServiceError::Unavailable(message.to_string())),
            };
            // 调用方超时后接收端已释放，结果丢弃即可。
            let _ = reply.send(result);
        });
        self.sender.send(job).map_err(|_| ServiceError::Stopped)?;
        response
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| ServiceError::Stopped)?
    }
}

fn open_storage(directory: &Path) -> Result<Storage, String> {
    std::fs::create_dir_all(directory)
        .map_err(|_| "无法创建本地数据目录，请检查权限".to_string())?;
    let now = now_ms();
    if now <= 0 {
        return Err("系统时间异常，无法初始化数据库".into());
    }
    Storage::open(&directory.join("desktrace.sqlite3"), now).map_err(|error| error.to_string())
}
