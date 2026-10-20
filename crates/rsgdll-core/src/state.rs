//! Per-state runtime registry (spec §6.1a): keyed by `lua_topointer(L, LUA_REGISTRYINDEX)`,
//! with a per-DLL monotonic generation counter.

use core::ffi::{c_int, c_void};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use rsgdll_sys::lua::{LUA_REGISTRYINDEX, lua_State};

use crate::protected::{Boot, MAX_UNREFS};

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Generation ids of states that are currently open (checked by `Drop` of refs off-map).
static OPEN: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

pub fn is_open(generation: u64) -> bool {
    OPEN.lock().map(|v| v.contains(&generation)).unwrap_or(false)
}

/// Runtime data of one Lua state for this DLL.
pub struct Runtime {
    pub main: *mut lua_State,
    pub generation: u64,
    pub module: &'static str,
    pub boot: Boot,
    pub unrefs: RefCell<Vec<c_int>>,
    /// Hook (event, id) pairs and timer names created by the framework (removed at close).
    pub hooks: RefCell<Vec<(String, String)>>,
    pub timers: RefCell<Vec<String>>,
    /// Typed per-state extension slots (executor, chunk cache, color metatable, ...).
    pub ext: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
}

impl Runtime {
    /// Drains up to `MAX_UNREFS` deferred unrefs into a `Copy` array (spec §6.2).
    pub fn take_unrefs(&self) -> ([c_int; MAX_UNREFS], usize) {
        let mut q = self.unrefs.borrow_mut();
        let n = q.len().min(MAX_UNREFS);
        let mut out = [0; MAX_UNREFS];
        q.drain(..n).zip(out.iter_mut()).for_each(|(r, o)| *o = r);
        (out, n)
    }

    /// Gets or creates a typed extension value.
    pub fn ext<T: 'static>(&self, key: &'static str, init: impl FnOnce() -> T) -> Rc<T> {
        let found = self.ext.borrow().get(key).cloned();
        let any = found.unwrap_or_else(|| {
            let v: Rc<dyn Any> = Rc::new(init());
            self.ext.borrow_mut().insert(key, v.clone());
            v
        });
        any.downcast::<T>().unwrap_or_else(|_| panic!("rsgdll: ext slot `{key}` type mismatch"))
    }
}

thread_local! {
    static STATES: RefCell<HashMap<usize, Rc<Runtime>>> = RefCell::new(HashMap::new());
}

/// Key of the state owning `l` (shared by all its coroutines).
pub fn key(l: *mut lua_State) -> usize {
    // SAFETY: lua_topointer on the registry pseudo-index never raises or allocates.
    unsafe { (rsgdll_sys::api().lua_topointer)(l, LUA_REGISTRYINDEX) as usize }
}

pub fn get(l: *mut lua_State) -> Option<Rc<Runtime>> {
    STATES.with(|s| s.borrow().get(&key(l)).cloned())
}

pub fn by_generation(generation: u64) -> Option<Rc<Runtime>> {
    STATES.with(|s| s.borrow().values().find(|r| r.generation == generation).cloned())
}

pub fn insert(main: *mut lua_State, module: &'static str, boot: Boot) -> Rc<Runtime> {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let rt = Rc::new(Runtime {
        main,
        generation,
        module,
        boot,
        unrefs: RefCell::new(Vec::new()),
        hooks: RefCell::new(Vec::new()),
        timers: RefCell::new(Vec::new()),
        ext: RefCell::new(HashMap::new()),
    });
    if let Ok(mut v) = OPEN.lock() {
        v.push(generation);
    }
    STATES.with(|s| s.borrow_mut().insert(key(main), rt.clone()));
    rt
}

/// Marks the generation stale and removes the entry.
pub fn remove(l: *mut lua_State) -> Option<Rc<Runtime>> {
    let rt = STATES.with(|s| s.borrow_mut().remove(&key(l)));
    if let (Some(rt), Ok(mut v)) = (&rt, OPEN.lock()) {
        v.retain(|g| *g != rt.generation);
    }
    rt
}

/// Queues a ref id for unref, if `generation` is still open on this thread (spec §6.2).
pub fn defer_unref(generation: u64, id: c_int) {
    if !is_open(generation) {
        return;
    }
    // `try_with`: may run during thread-local destruction.
    let _ = STATES.try_with(|s| {
        if let Ok(map) = s.try_borrow() {
            map.values()
                .find(|r| r.generation == generation)
                .and_then(|r| r.unrefs.try_borrow_mut().ok())
                .map(|mut q| q.push(id));
        }
    });
}

/// Light-userdata helper.
pub fn light<T>(p: &T) -> *mut c_void {
    (p as *const T).cast_mut().cast()
}
