//! Executor-neutral adapters for background completion.

use std::future::Future;
use std::sync::mpsc::SendError;

use rsgdll_runtime::CompletionSender;

/// Awaits background work and forwards only its owned output to the main thread.
pub async fn complete<T, F>(sender: CompletionSender<T>, future: F) -> Result<(), SendError<T>>
where
    T: Send,
    F: Future<Output = T> + Send,
{
    sender.send(future.await)
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::num::NonZeroUsize;
    use std::task::{Context, Poll, Waker};

    use rsgdll_runtime::{MainThread, completion_queue};

    use super::complete;

    #[test]
    fn ready_future_forwards_owned_output_to_completion_queue() {
        // Given: one ready Send future and an empty completion queue.
        let (sender, mut queue) = completion_queue(NonZeroUsize::MIN);
        let mut completion = Box::pin(complete(sender, future::ready(42_u64)));
        let mut context = Context::from_waker(Waker::noop());

        // When: any executor polls the adapter to completion.
        assert!(matches!(
            completion.as_mut().poll(&mut context),
            Poll::Ready(Ok(()))
        ));

        // Then: only the owned output reaches main-thread completion.
        // SAFETY: test runs generated-callback-equivalent code on this thread.
        let mut main_thread = unsafe { MainThread::__from_callback() };
        let mut value = None;
        queue.drain(&mut main_thread, |_, completed| value = Some(completed));
        assert_eq!(value, Some(42));
    }
}
