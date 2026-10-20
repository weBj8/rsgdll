//! Per-DLL worker pool (spec §10.2, §8.1): lazily grown, capped at 2x CPUs.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::error::{Error, Result};

type Job = Box<dyn FnOnce() + Send>;

struct Pool {
    queue: Mutex<VecDeque<Job>>,
    ready: Condvar,
    threads: AtomicUsize,
    idle: AtomicUsize,
    cap: usize,
}

fn pool() -> &'static Pool {
    static P: OnceLock<Pool> = OnceLock::new();
    P.get_or_init(|| Pool {
        queue: Mutex::new(VecDeque::new()),
        ready: Condvar::new(),
        threads: AtomicUsize::new(0),
        idle: AtomicUsize::new(0),
        cap: std::thread::available_parallelism().map_or(4, |n| n.get() * 2),
    })
}

fn worker_loop(p: &'static Pool) {
    loop {
        let job = p.queue.lock().ok().and_then(|q| {
            p.idle.fetch_add(1, Ordering::AcqRel);
            let mut q = p.ready.wait_while(q, |q| q.is_empty()).ok()?;
            p.idle.fetch_sub(1, Ordering::AcqRel);
            q.pop_front()
        });
        match job {
            Some(j) => j(),
            None => return,
        }
    }
}

fn submit(job: Job) {
    let p = pool();
    if let Ok(mut q) = p.queue.lock() {
        q.push_back(job);
    }
    let grow = p.idle.load(Ordering::Acquire) == 0 && p.threads.load(Ordering::Acquire) < p.cap;
    if grow {
        let n = p.threads.fetch_add(1, Ordering::AcqRel);
        let spawned = std::thread::Builder::new().name(format!("rsgdll-worker-{n}")).spawn(move || worker_loop(pool()));
        if spawned.is_err() {
            p.threads.fetch_sub(1, Ordering::AcqRel);
        }
    }
    p.ready.notify_one();
}

/// Runs blocking/CPU work off the main thread; the future resolves on a later pump.
/// A panic in `f` becomes an `Err`.
pub fn spawn_worker<T, F>(f: F) -> impl std::future::Future<Output = Result<T>> + use<T, F>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let (send, recv) = super::handle::oneshot::<Result<T>>();
    let counter: Option<Arc<AtomicUsize>> = super::executor::CURRENT.with(|c| c.borrow().clone());
    if let Some(c) = &counter {
        c.fetch_add(1, Ordering::AcqRel);
    }
    submit(Box::new(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(Error::msg("worker task panicked")));
        send(r);
        if let Some(c) = counter {
            c.fetch_sub(1, Ordering::AcqRel);
        }
    }));
    recv
}
