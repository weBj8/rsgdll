//! Safe GMod C++ interfaces (spec §5.2): `LuaInterface`, `LuaShared`, `create_interface`.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::CString;
use std::marker::PhantomData;
use std::rc::Rc;

use rsgdll_sys::gmod::{ILuaBase, ILuaInterface, ILuaShared, LUASHARED_INTERFACE, luabase};
use rsgdll_sys::lua::lua_State;

use crate::error::{Error, Result};
use crate::lua::Lua;
use crate::module::ModuleInfo;
use crate::protected::Op;
use crate::realm::Realm;

fn set_ext<T: 'static>(lua: &Lua, key: &'static str, v: T) {
    lua.runtime().ext.borrow_mut().insert(key, Rc::new(v));
}

/// Reads `luabase`, runs the vtable sanity check and records realm (spec §8.1 step 1).
pub fn attach(lua: &Lua, info: &ModuleInfo) -> Result<()> {
    let realm = match info.gmod {
        false => info.realms.first().copied(),
        true => {
            // SAFETY: main-thread GMod state (verified by open).
            let base = unsafe { luabase(lua.raw()) };
            let iface = base.cast::<ILuaInterface>();
            // SAFETY: GMod's luabase is a CLuaInterface; these slots are non-raising queries.
            let (top, s, c, m) = unsafe {
                let vt = (*iface).vt();
                ((vt.top)(iface.cast()), (vt.is_server)(iface.cast()), (vt.is_client)(iface.cast()), (vt.is_menu)(iface.cast()))
            };
            let exactly_one = [s, c, m].iter().filter(|b| **b).count() == 1;
            if base.is_null() || !exactly_one || top != lua.top() {
                return Err(Error::msg("vtable layout mismatch (GMod update?)"));
            }
            set_ext(lua, "luabase", Some(base as usize));
            Some(match (s, c) {
                (true, _) => Realm::Server,
                (_, true) => Realm::Client,
                _ => Realm::Menu,
            })
        }
    };
    set_ext(lua, "realm", realm);
    match realm {
        Some(r) if !info.realms.contains(&r) && !(r == Realm::Menu && info.realms.contains(&Realm::Client)) => {
            Err(Error::msg(format!("module built for {:?} cannot load in the {} state", info.realms, r.as_str())))
        }
        _ => Ok(()),
    }
}

/// Safe view of this state's `ILuaInterface`. Only available in GMod.
pub struct LuaInterface<'l> {
    lua: &'l Lua,
    ptr: *mut ILuaInterface,
}

fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::msg("string contains NUL"))
}

#[repr(C)]
struct RunString {
    iface: *mut ILuaInterface,
    name: *const c_char,
    path: *const c_char,
    code: *const c_char,
    ok: bool,
}

/// Protected step: `RunStringEx(..., run, printErrors=false, dontPushErrors=false, noReturns=true)`.
unsafe extern "C-unwind" fn run_string_step(_l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(r) = data.cast::<RunString>().as_mut() {
            let vt = (*r.iface).vt();
            r.ok = (vt.run_string_ex)(r.iface.cast(), r.name, r.path, r.code, true, false, false, true);
        }
    }
}

#[repr(C)]
struct Pooled {
    iface: *mut ILuaInterface,
    index: c_int,
}

unsafe extern "C-unwind" fn pooled_step(_l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(p) = data.cast::<Pooled>().as_ref() {
            ((*p.iface).vt().push_pooled_string)(p.iface.cast(), p.index);
        }
    }
}

#[repr(C)]
struct Named {
    iface: *mut ILuaInterface,
    name: *const c_char,
}

unsafe extern "C-unwind" fn require_step(_l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(p) = data.cast::<Named>().as_ref() {
            ((*p.iface).vt().require)(p.iface.cast(), p.name);
        }
    }
}

#[repr(C)]
struct FindRun {
    iface: *mut ILuaInterface,
    name: *const c_char,
    ok: bool,
}

unsafe extern "C-unwind" fn find_run_step(_l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(p) = data.cast::<FindRun>().as_mut() {
            p.ok = ((*p.iface).vt().find_and_run_script)(p.iface.cast(), p.name, true, true, c"".as_ptr(), true);
        }
    }
}

impl<'l> LuaInterface<'l> {
    pub fn is_server(&self) -> bool {
        // SAFETY: non-raising query on a live interface.
        unsafe { ((*self.ptr).vt().is_server)(self.ptr.cast()) }
    }
    pub fn is_client(&self) -> bool {
        unsafe { ((*self.ptr).vt().is_client)(self.ptr.cast()) }
    }
    pub fn is_menu(&self) -> bool {
        unsafe { ((*self.ptr).vt().is_menu)(self.ptr.cast()) }
    }
    pub fn realm(&self) -> Option<Realm> {
        self.lua.realm()
    }

    /// Current path id (`"GAME"`, `"LUA"`, `"lsv"`, ...).
    pub fn path_id(&self) -> String {
        // SAFETY: returns a pointer into CLuaInterface::m_sPathID (NUL-terminated).
        unsafe {
            let p = ((*self.ptr).vt().get_path_id)(self.ptr.cast());
            if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    }

    /// Runs Lua code like `RunString`; a compile/runtime error is `Err`.
    pub fn run_string(&self, name: &str, path: &str, code: &str) -> Result<()> {
        let (n, p, c) = (cstr(name)?, cstr(path)?, cstr(code)?);
        let mut r = RunString { iface: self.ptr, name: n.as_ptr(), path: p.as_ptr(), code: c.as_ptr(), ok: false };
        self.main_op(&[Op::Raw(run_string_step, crate::state::light_mut(&mut r))])?;
        match r.ok {
            true => Ok(()),
            false => Err(Error::lua(format!("RunString '{name}' failed"), None)),
        }
    }

    pub fn find_and_run_script(&self, file: &str) -> Result<bool> {
        let f = cstr(file)?;
        let mut r = FindRun { iface: self.ptr, name: f.as_ptr(), ok: false };
        self.main_op(&[Op::Raw(find_run_step, crate::state::light_mut(&mut r))]).map(|()| r.ok)
    }

    /// `require(name)` through GMod.
    pub fn require(&self, name: &str) -> Result<()> {
        let n = cstr(name)?;
        let mut r = Named { iface: self.ptr, name: n.as_ptr() };
        self.main_op(&[Op::Raw(require_step, crate::state::light_mut(&mut r))])
    }

    /// Console message (`Msg`); variadic slot called with `"%s"` + one string.
    pub fn msg(&self, text: &str) {
        let t = CString::new(text.replace('\0', "")).unwrap_or_default();
        // SAFETY: non-raising variadic; never a user format string.
        unsafe { ((*self.ptr).vt().msg)(self.ptr.cast(), c"%s".as_ptr(), t.as_ptr()) }
    }

    /// `ErrorNoHalt` (prints without raising).
    pub fn error_no_halt(&self, text: &str) {
        let t = CString::new(text.replace('\0', "")).unwrap_or_default();
        unsafe { ((*self.ptr).vt().error_no_halt)(self.ptr.cast(), c"%s".as_ptr(), t.as_ptr()) }
    }

    /// Pushes a pooled string (`util.NetworkStringToID`-style index) onto the stack.
    /// From a coroutine the value is pushed on the main stack and moved (spec §6.1a).
    pub fn push_pooled_string(&self, index: i32) -> Result<()> {
        let mut p = Pooled { iface: self.ptr, index };
        let (main, cur) = (self.lua.main_thread(), self.lua.raw());
        let step = Op::Raw(pooled_step, crate::state::light_mut(&mut p));
        let ops: Vec<Op> = match main == cur {
            true => vec![step],
            false => vec![Op::CheckStackOf(main, 1), step, Op::XMove(main, cur, 1)],
        };
        self.lua.run(&[], &ops).map(|_| ())
    }

    pub fn raw(&self) -> *mut ILuaInterface {
        self.ptr
    }

    /// Runs main-stack ops with the stack restored afterwards.
    fn main_op(&self, ops: &[Op]) -> Result<()> {
        let g = self.lua.guard();
        let r = self.lua.run(&[], ops).map(|_| ());
        drop(g);
        r
    }
}

impl Lua {
    /// `ILuaInterface` of this state (only in GMod).
    pub fn interface(&self) -> Result<LuaInterface<'_>> {
        self.base().map(|b| LuaInterface { lua: self, ptr: b.cast() })
    }

    /// Raw `ILuaBase*` of the main thread (opt-in raw access, spec §1).
    pub fn base(&self) -> Result<*mut ILuaBase> {
        crate::conv::gmod::luabase(self).ok_or_else(|| Error::msg("ILuaBase is only available inside GMod"))
    }
}

/// `ILuaShared` from `lua_shared`'s `CreateInterface("LUASHARED003")`.
pub struct LuaShared {
    ptr: *mut ILuaShared,
}

// SAFETY: a process-global engine singleton; we only call thread-agnostic getters on it.
unsafe impl Sync for LuaShared {}
unsafe impl Send for LuaShared {}

pub fn lua_shared() -> Result<&'static LuaShared> {
    static S: std::sync::OnceLock<Option<LuaShared>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let loaded = rsgdll_sys::lua_shared().ok()?;
        // SAFETY: the returned pointer is an ILuaShared per garrysmod_common.
        unsafe { rsgdll_sys::loader::create_interface(&loaded.lib, LUASHARED_INTERFACE) }.map(|p| LuaShared { ptr: p.cast() })
    })
    .as_ref()
    .ok_or_else(|| Error::msg("CreateInterface(LUASHARED003) failed"))
}

/// Cross-realm handle: re-fetches the interface on every use (spec §5.2). `!Send`.
pub struct RealmInterface {
    shared: &'static LuaShared,
    realm: Realm,
    seen: *mut ILuaInterface,
    _not_send: PhantomData<*mut ()>,
}

impl LuaShared {
    fn get(&self, realm: Realm) -> *mut ILuaInterface {
        // SAFETY: getter on the singleton.
        unsafe { ((*self.ptr).vt().get_lua_interface)(self.ptr.cast(), realm.id()).cast() }
    }

    pub fn lua_interface(&'static self, realm: Realm) -> Option<RealmInterface> {
        let p = self.get(realm);
        (!p.is_null()).then_some(RealmInterface { shared: self, realm, seen: p, _not_send: PhantomData })
    }

    /// `GetStackTraces()` (all realms, formatted by GMod).
    pub fn stack_traces(&self) -> String {
        // SAFETY: returns a C string owned by lua_shared.
        unsafe {
            let p = ((*self.ptr).vt().get_stack_traces)(self.ptr.cast());
            if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    }

    pub fn raw(&self) -> *mut ILuaShared {
        self.ptr
    }
}

impl RealmInterface {
    /// Current pointer, or `StateClosed` if the realm's state changed or is gone.
    pub fn ptr(&self) -> Result<*mut ILuaInterface> {
        let now = self.shared.get(self.realm);
        (!now.is_null() && now == self.seen).then_some(now).ok_or_else(Error::state_closed)
    }

    pub fn is_server(&self) -> Result<bool> {
        // SAFETY: validated pointer, non-raising getter.
        self.ptr().map(|p| unsafe { ((*p).vt().is_server)(p.cast()) })
    }

    pub fn msg(&self, text: &str) -> Result<()> {
        let t = CString::new(text.replace('\0', "")).unwrap_or_default();
        self.ptr().map(|p| unsafe { ((*p).vt().msg)(p.cast(), c"%s".as_ptr(), t.as_ptr()) })
    }
}

/// Engine interface lookup (`"engine"`, `"VEngineServer023"`). The caller asserts the layout.
///
/// # Safety
/// `T` must match the interface's real vtable layout.
pub unsafe fn create_interface<T>(module: &str, name: &str) -> Option<&'static T> {
    let lib = rsgdll_sys::loader::find_module(module)?;
    // SAFETY: by caller contract.
    let p = unsafe { rsgdll_sys::loader::create_interface(&lib, name) }?;
    std::mem::forget(lib); // engine modules stay loaded for the process lifetime
    // SAFETY: engine singletons live as long as the process.
    Some(unsafe { &*p.cast::<T>() })
}
