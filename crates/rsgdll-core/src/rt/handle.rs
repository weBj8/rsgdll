//! `LuaHandle` (spec §10.1/10.2): `Send + Sync + Clone` access to a state from any thread.
//! Work is queued and run on the main thread by the executor pump.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::ThreadId;

use crate::error::{Error, Result};
use crate::lua::Lua;

/// `None` = the state closed; the job must report `StateClosed`.
type Job = Box<dyn FnOnce(Option<&Lua>) + Send>;

pub struct Shared {
    jobs: Mutex<VecDeque<Job>>,
    closed: AtomicBool,
    generation: u64,
    main: ThreadId,
}

impl Shared {
    pub(crate) fn new(generation: u64) -> Arc<Self> {
        Arc::new(Self {
            jobs: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
            generation,
            main: std::thread::current().id(),
        })
    }

    fn push(&self, job: Job) {
        // Checked under the lock so a job cannot slip in after `close` drained the queue.
        let rejected = match self.jobs.lock() {
            Ok(mut q) if !self.closed.load(Ordering::Acquire) => {
                q.push_back(job);
                None
            }
            _ => Some(job),
        };
        if let Some(job) = rejected {
            job(None);
        }
    }

    pub(crate) fn drain(&self, lua: &Lua) {
        let jobs: Vec<Job> = self
            .jobs
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        jobs.into_iter().for_each(|j| {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| j(Some(lua)))).is_err() {
                super::executor::log_error(
                    lua,
                    &format!("[{}] LuaHandle job panicked", lua.module_name()),
                );
            }
        });
    }

    pub(crate) fn close(&self) {
        let jobs: Vec<Job> = match self.jobs.lock() {
            Ok(mut q) => {
                self.closed.store(true, Ordering::Release);
                q.drain(..).collect()
            }
            Err(_) => Vec::new(),
        };
        jobs.into_iter().for_each(|j| j(None));
    }
}

/// Thread-safe handle to one Lua state.
#[derive(Clone)]
pub struct LuaHandle {
    shared: Arc<Shared>,
}

/// Minimal oneshot channel.
struct Slot<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

pub struct Receive<T> {
    slot: Arc<Mutex<Slot<T>>>,
}

pub(crate) fn oneshot<T>() -> (impl FnOnce(T) + Send, Receive<T>)
where
    T: Send + 'static,
{
    let slot = Arc::new(Mutex::new(Slot {
        value: None,
        waker: None,
    }));
    let tx = slot.clone();
    let send = move |v: T| {
        let w = tx.lock().ok().and_then(|mut s| {
            s.value = Some(v);
            s.waker.take()
        });
        if let Some(w) = w {
            w.wake();
        }
    };
    (send, Receive { slot })
}

impl<T> Future for Receive<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        match self.slot.lock() {
            Ok(mut s) => match s.value.take() {
                Some(v) => Poll::Ready(v),
                None => {
                    s.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            },
            Err(_) => Poll::Pending,
        }
    }
}

impl LuaHandle {
    pub fn new(lua: &Lua) -> Self {
        Self {
            shared: super::executor::get(lua).shared.clone(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }

    /// Runs `f` on the main thread and awaits its result (flattened).
    pub fn call<T, F>(&self, f: F) -> impl Future<Output = Result<T>> + Send + use<T, F>
    where
        T: Send + 'static,
        F: FnOnce(&Lua) -> Result<T> + Send + 'static,
    {
        let (send, recv) = oneshot::<Result<T>>();
        self.shared.push(Box::new(move |lua| {
            send(match lua {
                Some(l) => f(l),
                None => Err(Error::state_closed()),
            })
        }));
        recv
    }

    /// Fire and forget; errors are logged with `ErrorNoHalt`.
    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce(&Lua) -> Result<()> + Send + 'static,
    {
        self.shared.push(Box::new(move |lua| {
            if let Some(l) = lua {
                if let Err(e) = f(l) {
                    super::executor::log_error(
                        l,
                        &format!("[{}] spawn: {}", l.module_name(), e.lua_message()),
                    );
                }
            }
        }));
    }

    /// Blocks the calling (non-main) thread until `f` ran on the main thread.
    pub fn call_blocking<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Lua) -> Result<T> + Send + 'static,
    {
        if std::thread::current().id() == self.shared.main {
            return Err(Error::msg(
                "LuaHandle::call_blocking on the main thread would deadlock",
            ));
        }
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<T>>(1);
        self.shared.push(Box::new(move |lua| {
            let _ = tx.send(match lua {
                Some(l) => f(l),
                None => Err(Error::state_closed()),
            });
        }));
        rx.recv().unwrap_or_else(|_| Err(Error::state_closed()))
    }
}

impl Lua {
    /// Thread-safe handle to this state.
    pub fn handle(&self) -> LuaHandle {
        LuaHandle::new(self)
    }
}
