use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

static RUNTIME_HANDLE: OnceLock<Handle> = OnceLock::new();

/// Register the tokio runtime used for background tasks spawned outside an
/// active runtime context (e.g. player callbacks on plain threads).
pub fn set_runtime_handle(handle: Handle) {
    if RUNTIME_HANDLE.set(handle).is_err() {
        tracing::warn!("runtime handle already set; ignoring subsequent registration");
    }
}

pub(crate) fn spawn(fut: impl Future<Output = ()> + Send + 'static) -> JoinHandle<()> {
    if let Ok(handle) = Handle::try_current() {
        return handle.spawn(fut);
    }
    match RUNTIME_HANDLE.get() {
        Some(handle) => handle.spawn(fut),
        None => panic!(
            "no tokio runtime available: call syncplay_core::set_runtime_handle during startup"
        ),
    }
}
