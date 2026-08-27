//! Main-thread runtime services.

mod completion;
mod main_thread;

pub use completion::{CompletionQueue, CompletionSender, completion_queue};
pub use main_thread::MainThread;

#[doc(hidden)]
pub mod __private {
    use super::MainThread;

    /// Mints the main-thread capability in generated callback glue.
    ///
    /// # Safety
    ///
    /// Caller must be generated framework glue executing inside a GMod
    /// main-thread callback.
    #[must_use]
    pub const unsafe fn main_thread_from_callback() -> MainThread {
        MainThread::new()
    }
}
