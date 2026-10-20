//! Per-state main-thread executor (spec §10.1): `async-task` runnables polled from a
//! framework `Think` hook with a time budget; timers for `sleep`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use async_task::{Runnable, Task};

use super::handle::Shared;
use super::local::LocalLua;
use crate::error::Result;
use crate::lua::Lua;

pub const DEFAULT_BUDGET: Duration = Duration::from_millis(2);

pub struct Executor {
    queue: Arc<Mutex<VecDeque<Runnable>>>,
    tasks: RefCell<Vec<Task<()>>>,
    pub(crate) shared: Arc<Shared>,
    /// Worker jobs started by this state's tasks that have not finished yet.
    pub(crate) in_flight: Arc<AtomicUsize>,
    pub budget: Cell<Duration>,
}

thread_local! {
    static TIMERS: RefCell<Vec<(Instant, Waker)>> = const { RefCell::new(Vec::new()) };
    /// In-flight counter of the executor currently polling (for `spawn_worker`).
    pub(crate) static CURRENT: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
}

/// Executor of `lua`'s state (created on first use).
pub fn get(lua: &Lua) -> Rc<Executor> {
    lua.runtime().ext("executor", || Executor {
        queue: Arc::new(Mutex::new(VecDeque::new())),
        tasks: RefCell::new(Vec::new()),
        shared: Shared::new(lua.generation()),
        in_flight: Arc::new(AtomicUsize::new(0)),
        budget: Cell::new(DEFAULT_BUDGET),
    })
}

/// Spawns a `!Send` future on the main thread of `lua`'s state.
pub fn spawn_future(lua: &Lua, fut: impl Future<Output = ()> + 'static) {
    let ex = get(lua);
    let q = ex.queue.clone();
    let schedule = move |r: Runnable| {
        if let Ok(mut q) = q.lock() {
            q.push_back(r);
        }
    };
    let (runnable, task) = async_task::spawn_local(fut, schedule);
    runnable.schedule();
    ex.tasks.borrow_mut().push(task);
}

/// Logs an async error the GMod way (`ErrorNoHalt`, or tier0 outside GMod).
pub fn log_error(lua: &Lua, msg: &str) {
    let line = format!("{msg}\n");
    match lua.interface() {
        Ok(i) => i.error_no_halt(&line),
        Err(_) => rsgdll_sys::tier0::warning(&line),
    }
}

/// `spawn_local(lua, |lua| async move { ... })` (spec §10.2); errors are logged.
pub fn spawn_local<F, Fut>(lua: &Lua, f: F)
where
    F: FnOnce(LocalLua) -> Fut,
    Fut: Future<Output = Result<()>> + 'static,
{
    let local = LocalLua::new(lua);
    let log_with = LocalLua::new(lua);
    let fut = f(local);
    spawn_future(lua, async move {
        if let Err(e) = fut.await {
            if let Ok(l) = log_with.lua() {
                log_error(
                    l,
                    &format!("[{}] async task: {}", l.module_name(), e.lua_message()),
                );
            }
        }
    });
}

/// One pump step: handle jobs, expired timers, then runnables within the budget.
pub fn pump(lua: &Lua) {
    let ex = get(lua);
    let start = Instant::now();
    ex.shared.drain(lua);
    let now = Instant::now();
    let expired: Vec<Waker> = TIMERS.with(|t| {
        let mut t = t.borrow_mut();
        let (due, keep): (Vec<_>, Vec<_>) = t.drain(..).partition(|(d, _)| *d <= now);
        *t = keep;
        due.into_iter().map(|(_, w)| w).collect()
    });
    expired.into_iter().for_each(Waker::wake);
    CURRENT.with(|c| *c.borrow_mut() = Some(ex.in_flight.clone()));
    // The budget is checked between polls; a single long poll is not preempted.
    std::iter::from_fn(|| {
        (start.elapsed() < ex.budget.get())
            .then(|| ex.queue.lock().ok().and_then(|mut q| q.pop_front()))
            .flatten()
    })
    .for_each(|r| {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.run())).is_err() {
            log_error(lua, &format!("[{}] async task panicked", lua.module_name()));
        }
    });
    CURRENT.with(|c| *c.borrow_mut() = None);
    ex.tasks.borrow_mut().retain(|t| !t.is_finished());
}

/// Number of live (unfinished) tasks.
pub fn pending(lua: &Lua) -> usize {
    get(lua)
        .tasks
        .borrow()
        .iter()
        .filter(|t| !t.is_finished())
        .count()
}

/// Stops the executor: fails pending handle calls, cancels tasks, waits for workers.
pub fn stop(lua: &Lua, worker_timeout: Duration) {
    let ex = get(lua);
    ex.shared.close();
    let tasks: Vec<Task<()>> = ex.tasks.borrow_mut().drain(..).collect();
    drop(tasks); // dropping a Task cancels it
    if let Ok(mut q) = ex.queue.lock() {
        q.clear();
    }
    let deadline = Instant::now() + worker_timeout;
    while ex.in_flight.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    match ex.in_flight.load(Ordering::Acquire) {
        0 => {}
        n => rsgdll_sys::tier0::warning(&format!(
            "[{}] {n} worker task(s) still running at close; detached\n",
            lua.module_name()
        )),
    }
}

/// Timer-backed sleep (resolves on a later pump; never blocks the thread).
pub struct Sleep {
    deadline: Instant,
}

pub fn sleep(d: Duration) -> Sleep {
    Sleep {
        deadline: Instant::now() + d,
    }
}

impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match Instant::now() >= self.deadline {
            true => Poll::Ready(()),
            false => {
                TIMERS.with(|t| t.borrow_mut().push((self.deadline, cx.waker().clone())));
                Poll::Pending
            }
        }
    }
}
