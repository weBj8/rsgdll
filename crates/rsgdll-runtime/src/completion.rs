use std::num::NonZeroUsize;
use std::sync::mpsc::{Receiver, SendError, SyncSender, TryRecvError, sync_channel};

use crate::MainThread;

/// Sendable producer for values completed by background work.
#[derive(Debug, Clone)]
pub struct CompletionSender<T>(SyncSender<T>);

impl<T> CompletionSender<T> {
    /// Queues one owned value for main-thread completion.
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        self.0.send(value)
    }
}

/// Main-thread side of a bounded background completion channel.
#[derive(Debug)]
pub struct CompletionQueue<T>(Receiver<T>);

impl<T> CompletionQueue<T> {
    /// Runs every currently queued completion on the GMod main thread.
    pub fn drain(
        &mut self,
        main_thread: &mut MainThread,
        mut complete: impl FnMut(&mut MainThread, T),
    ) -> usize {
        let mut count = 0;
        loop {
            match self.0.try_recv() {
                Ok(value) => {
                    complete(main_thread, value);
                    count += 1;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return count,
            }
        }
    }
}

/// Creates a bounded channel carrying only owned `Send` values from workers.
#[must_use]
pub fn completion_queue<T>(capacity: NonZeroUsize) -> (CompletionSender<T>, CompletionQueue<T>) {
    let (sender, receiver) = sync_channel(capacity.get());
    (CompletionSender(sender), CompletionQueue(receiver))
}

#[cfg(test)]
mod tests {
    use std::thread;

    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::*;

    assert_not_impl_any!(MainThread: Send, Sync);
    assert_impl_all!(CompletionSender<u64>: Clone, Send, Sync);

    #[test]
    fn worker_values_complete_only_during_main_thread_drain() {
        let (sender, mut queue) = completion_queue(NonZeroUsize::MIN);
        let worker = thread::spawn(move || sender.send(42_u64));
        worker.join().expect("worker exits").expect("queue is open");
        let mut completed = Vec::new();
        let mut main_thread = MainThread::new();

        assert_eq!(
            queue.drain(&mut main_thread, |_, value| completed.push(value)),
            1
        );
        assert_eq!(completed, [42]);
        assert_eq!(queue.drain(&mut main_thread, |_, _| {}), 0);
    }
}
