use std::num::NonZeroUsize;
use std::thread;

use rsgdll_runtime::{CompletionSender, MainThread, completion_queue};
use static_assertions::{assert_impl_all, assert_not_impl_any};

assert_not_impl_any!(MainThread: Send, Sync);
assert_impl_all!(CompletionSender<u64>: Clone, Send, Sync);

#[test]
fn worker_values_complete_only_during_main_thread_drain() {
    // Given: a bounded completion queue and one Send-only worker value.
    let (sender, mut queue) = completion_queue(NonZeroUsize::MIN);
    let worker = thread::spawn(move || sender.send(42_u64));
    worker.join().expect("worker exits").expect("queue is open");
    let mut completed = Vec::new();
    // SAFETY: this test invokes the same constructor generated callbacks use
    // while running on the test's designated main thread.
    let mut main_thread = unsafe { MainThread::__from_callback() };

    // When: the designated main thread drains queued completions.
    let count = queue.drain(&mut main_thread, |_, value| completed.push(value));

    // Then: the value is observed once, on drain, and removed from the queue.
    assert_eq!(count, 1);
    assert_eq!(completed, [42]);
    assert_eq!(queue.drain(&mut main_thread, |_, _| {}), 0);
}
