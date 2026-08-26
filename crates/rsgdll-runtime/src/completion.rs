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
