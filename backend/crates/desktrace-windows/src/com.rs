//! COM 单线程套间的初始化与专用线程。系统对话框、外壳定位和资源管理器接口都要求 STA。

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

use crate::WindowsError;

/// 当前线程的 COM 初始化凭据，离开作用域时反初始化。只能在初始化它的线程上使用。
pub(crate) struct ComApartment {
    // 阻止跨线程移动：COM 初始化与反初始化必须在同一线程。
    _not_send: std::marker::PhantomData<*const ()>,
}

impl ComApartment {
    /// 把当前线程初始化为 STA。
    pub(crate) fn single_threaded() -> Result<Self, WindowsError> {
        // SAFETY：只初始化当前线程，成功后由 Drop 在同一线程配对反初始化。
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()? };
        Ok(Self {
            _not_send: std::marker::PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY：与构造时的 CoInitializeEx 在同一线程配对调用。
        unsafe { CoUninitialize() };
    }
}

/// 在新的 STA 线程上执行一次性任务并等待结果，调用方线程的套间类型不受影响。
pub(crate) fn run_in_sta<T, F>(task: F) -> Result<T, WindowsError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, WindowsError> + Send + 'static,
{
    std::thread::Builder::new()
        .name("desktrace-sta".into())
        .spawn(move || {
            let _apartment = ComApartment::single_threaded()?;
            task()
        })
        .map_err(|_| WindowsError::ThreadFailed)?
        .join()
        .map_err(|_| WindowsError::ThreadFailed)?
}
