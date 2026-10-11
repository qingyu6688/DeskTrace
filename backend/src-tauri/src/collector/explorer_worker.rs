//! 资源管理器探测的专用 STA 线程。探测超时后不再派发新请求，直到线程返回；
//! 无法强制取消正在阻塞的 COM 调用，只能标记故障并退避。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::time::Duration;

use desktrace_windows::{ExplorerProbe, ExplorerView};

type Request = (isize, SyncSender<ExplorerView>);

/// 探测失败的原因。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProbeError {
    /// 上一次调用仍未返回或本次超时。
    Busy,
    /// 线程已退出。
    Stopped,
}

pub(crate) struct ExplorerWorker {
    requests: Sender<Request>,
    busy: Arc<AtomicBool>,
}

impl ExplorerWorker {
    /// 启动线程并创建探测器；COM 初始化或外壳窗口集合不可用时返回 `None`。
    pub(crate) fn start() -> Option<Self> {
        let (requests, receiver) = mpsc::channel::<Request>();
        let (ready, started) = mpsc::channel();
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = busy.clone();
        std::thread::Builder::new()
            .name("desktrace-explorer".into())
            .spawn(move || {
                let probe = match ExplorerProbe::new() {
                    Ok(probe) => probe,
                    Err(_) => {
                        let _ = ready.send(false);
                        return;
                    }
                };
                let _ = ready.send(true);
                for (window, reply) in receiver {
                    let view = probe.probe(window);
                    worker_busy.store(false, Ordering::Release);
                    // 调用方已超时放弃时结果直接丢弃。
                    let _ = reply.try_send(view);
                }
            })
            .ok()?;
        started.recv().ok().filter(|ok| *ok)?;
        Some(Self { requests, busy })
    }

    /// 探测一次，最多等待 `timeout`。
    pub(crate) fn probe(
        &self,
        window: isize,
        timeout: Duration,
    ) -> Result<ExplorerView, ProbeError> {
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(ProbeError::Busy);
        }
        let (reply, response) = mpsc::sync_channel(1);
        if self.requests.send((window, reply)).is_err() {
            self.busy.store(false, Ordering::Release);
            return Err(ProbeError::Stopped);
        }
        response.recv_timeout(timeout).map_err(|_| ProbeError::Busy)
    }
}
