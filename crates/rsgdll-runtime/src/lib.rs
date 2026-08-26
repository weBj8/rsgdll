//! Main-thread runtime services.

mod completion;
mod main_thread;

pub use completion::{CompletionQueue, CompletionSender, completion_queue};
pub use main_thread::MainThread;
