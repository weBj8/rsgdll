//! Feature `tokio` (spec §10.2): one lazily started runtime per DLL, never shut down.

use std::sync::OnceLock;

pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("rsgdll-tokio")
            .build()
            .unwrap_or_else(|e| panic!("rsgdll: cannot start tokio runtime: {e}"))
    })
}

/// `rsgdll::tokio::spawn`.
pub fn spawn<F>(f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(f)
}
