//! Userdata of Rust types (spec §6.2). Checked by metatable identity (`lua_rawequal`
//! against a cached ref), never `luaL_checkudata`. `__gc` drops under `catch_unwind`.

use core::ffi::{c_int, c_void};
use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use rsgdll_sys::lua::*;

use super::func::{bad_argument, push_raw_closure, returns};
use super::{FromArgs, FromLua, IntoLua, IntoLuaMulti, Stage, lua_err, mismatch};
use crate::error::{ConvError, Result};
use crate::lua::Lua;
use crate::protected::{Op, Raw};
use crate::trampoline::{self, Outcome};

/// A Rust type exposed to Lua as full userdata.
pub trait UserData: 'static + Sized {
    /// Metatable name (`rsgdll.<module>.<NAME>`).
    const NAME: &'static str;
    /// Registers methods/metamethods.
    fn methods(_m: &mut UserDataMethods<Self>) {}
}

type Method = Box<dyn Fn(&Lua) -> Outcome>;

/// Method table builder.
pub struct UserDataMethods<T> {
    entries: Vec<(String, Method)>,
    _t: std::marker::PhantomData<T>,
}

/// Userdata memory: the value (taken on `__gc`).
type Slot<T> = RefCell<Option<T>>;

fn self_slot<T: UserData>(lua: &Lua, idx: c_int) -> Result<&Slot<T>, ConvError> {
    let meta = metatable::<T>(lua).map_err(lua_err)?;
    let g = lua.guard();
    let [found, eq] = lua.read_ops::<2>(
        &[idx],
        &[
            Op::GetMetatable(1, 0),
            Op::PushRef(meta),
            Op::RawEqual(-1, -2, 1),
        ],
    )?;
    drop(g);
    let ok = lua.read(idx).tag == LUA_TUSERDATA && found.num != 0.0 && eq.num != 0.0;
    match ok {
        // SAFETY: metatable identity proves this userdata was created by `push_userdata::<T>`.
        true => Ok(unsafe { &*(lua.read(idx).ptr as *const Slot<T>) }),
        false => Err(mismatch(lua, idx, T::NAME)),
    }
}

impl<T: UserData> UserDataMethods<T> {
    fn add(
        &mut self,
        name: &str,
        path: String,
        f: impl Fn(&Lua, &Slot<T>, c_int) -> Outcome + 'static,
    ) {
        let m: Method = Box::new(move |lua| match self_slot::<T>(lua, 1) {
            Ok(slot) => f(lua, slot, lua.top()),
            Err(e) => bad_argument(lua, &path, 1, &e),
        });
        self.entries.push((name.to_owned(), m));
    }

    /// `obj:name(args)` with `&T`.
    pub fn method<A, R, F>(&mut self, name: &str, f: F)
    where
        A: for<'a> FromArgs<'a>,
        R: IntoLuaMulti,
        F: Fn(&Lua, &T, A) -> crate::Result<R> + 'static,
    {
        let path = format!("{}:{name}", T::NAME);
        let p2 = path.clone();
        self.add(name, path, move |lua, slot, top| {
            match A::from_args(lua, 2, top - 1) {
                Err((n, e)) => bad_argument(lua, &p2, n + 1, &e),
                Ok(args) => {
                    let r = match slot.try_borrow() {
                        Ok(b) => match b.as_ref() {
                            Some(v) => f(lua, v, args),
                            None => Err(crate::anyhow!("{} already collected", T::NAME)),
                        },
                        Err(_) => Err(crate::anyhow!("{} is mutably borrowed", T::NAME)),
                    };
                    finish(lua, top, &p2, r)
                }
            }
        });
    }

    /// `obj:name(args)` with `&mut T` (re-entrant use errors instead of aliasing).
    pub fn method_mut<A, R, F>(&mut self, name: &str, f: F)
    where
        A: for<'a> FromArgs<'a>,
        R: IntoLuaMulti,
        F: Fn(&Lua, &mut T, A) -> crate::Result<R> + 'static,
    {
        let path = format!("{}:{name}", T::NAME);
        let p2 = path.clone();
        self.add(name, path, move |lua, slot, top| {
            match A::from_args(lua, 2, top - 1) {
                Err((n, e)) => bad_argument(lua, &p2, n + 1, &e),
                Ok(args) => {
                    let r = match slot.try_borrow_mut() {
                        Ok(mut b) => match b.as_mut() {
                            Some(v) => f(lua, v, args),
                            None => Err(crate::anyhow!("{} already collected", T::NAME)),
                        },
                        Err(_) => Err(crate::anyhow!("{} is already borrowed", T::NAME)),
                    };
                    finish(lua, top, &p2, r)
                }
            }
        });
    }

    /// Metamethod (`__tostring`, `__eq`, `__len`, ...).
    pub fn meta<R, F>(&mut self, name: &str, f: F)
    where
        R: IntoLuaMulti,
        F: Fn(&Lua, &T) -> crate::Result<R> + 'static,
    {
        self.method(name, move |lua, v, ()| f(lua, v));
    }
}

fn finish<R: IntoLuaMulti>(lua: &Lua, top: c_int, path: &str, r: crate::Result<R>) -> Outcome {
    match r {
        Ok(v) => returns(lua, top, path, v),
        Err(e) => Outcome::Err(trampoline::format_error(
            lua.module_name(),
            path,
            &e.lua_message(),
        )),
    }
}

unsafe extern "C-unwind" fn gc<T: UserData>(l: *mut lua_State) -> c_int {
    unsafe {
        let p = (rsgdll_sys::api().lua_touserdata)(l, 1) as *const Slot<T>;
        if let Some(slot) = p.as_ref() {
            let r = catch_unwind(AssertUnwindSafe(|| {
                // Take then drop; the slot itself has no destructor to run.
                let v = slot.try_borrow_mut().ok().and_then(|mut b| b.take());
                drop(v);
            }));
            if r.is_err() {
                rsgdll_sys::tier0::msg("rsgdll: panic in userdata drop during __gc (ignored)\n");
            }
        }
    }
    0
}

thread_local! {
    static METAS: RefCell<HashMap<(u64, TypeId), c_int>> = RefCell::new(HashMap::new());
}

/// Per-state metatable for `T` (created on first use).
pub fn metatable<T: UserData>(lua: &Lua) -> Result<c_int> {
    let key = (lua.generation(), TypeId::of::<T>());
    match METAS.with(|m| m.borrow().get(&key).copied()) {
        Some(r) => Ok(r),
        None => {
            let mut methods = UserDataMethods::<T> {
                entries: Vec::new(),
                _t: std::marker::PhantomData,
            };
            T::methods(&mut methods);
            let g = lua.guard();
            let full = format!("rsgdll.{}.{}", lua.module_name(), T::NAME);
            let mut out = [Raw::default()];
            lua.protect(
                &[],
                &[
                    Op::CheckStack(8),
                    Op::NewTable(0, 4),
                    Op::PushCClosure(gc::<T>, 0),
                    Op::SetField(-2, c"__gc".as_ptr()),
                    Op::PushStr(full.as_ptr(), full.len()),
                    Op::SetField(-2, c"__name".as_ptr()),
                    Op::NewTable(0, 8),
                ],
                &mut out,
            )?;
            // stack: meta, index
            let (meta, index) = (lua.top() - 1, lua.top());
            methods
                .entries
                .into_iter()
                .try_for_each(|(name, m)| -> Result<()> {
                    push_raw_closure(lua, m)?;
                    let target = if name.starts_with("__") { meta } else { index };
                    let f = lua.top();
                    let r = lua
                        .run(
                            &[target, f],
                            &[
                                Op::PushStr(name.as_ptr(), name.len()),
                                Op::PushValue(2),
                                Op::RawSet(1),
                                Op::Pop(2),
                            ],
                        )
                        .map(|_| ());
                    // SAFETY: pop the closure copy left in the caller frame.
                    unsafe { (lua.api().lua_settop)(lua.raw(), -2) };
                    r
                })?;
            lua.protect(
                &[meta, index],
                &[Op::SetField(1, c"__index".as_ptr()), Op::Ref(0)],
                &mut out,
            )?;
            drop(g);
            let id = out[0].num as c_int;
            METAS.with(|m| m.borrow_mut().insert(key, id));
            Ok(id)
        }
    }
}

#[repr(C)]
struct Init {
    size: usize,
    ptr: *mut c_void,
}

unsafe extern "C-unwind" fn alloc(l: *mut lua_State, data: *mut c_void) {
    unsafe {
        if let Some(init) = data.cast::<Init>().as_mut() {
            init.ptr = (rsgdll_sys::api().lua_newuserdata)(l, init.size);
        }
    }
}

/// Moves `v` into a new userdata with `T`'s metatable (pushed on the stack).
pub fn push_userdata<T: UserData>(lua: &Lua, v: T) -> Result<()> {
    let meta = metatable::<T>(lua)?;
    let mut init = Init {
        size: core::mem::size_of::<Slot<T>>(),
        ptr: core::ptr::null_mut(),
    };
    lua.run(
        &[],
        &[
            Op::CheckStack(4),
            Op::Raw(alloc, crate::state::light_mut(&mut init).cast()),
        ],
    )?;
    // SAFETY: fresh userdata block of the right size; Lua aligns to max_align_t (16 on x64).
    unsafe { init.ptr.cast::<Slot<T>>().write(RefCell::new(Some(v))) };
    lua.run(
        &[-1],
        &[Op::PushRef(meta), Op::SetMetatable(1), Op::Remove(1)],
    )
    .map(|_| ())
}

/// Owned value moved into Lua as userdata (`IntoLua`).
pub struct LuaUd<T>(pub T);

impl<T: UserData> IntoLua for LuaUd<T> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        // Allocation must happen now so the value is not lost; the slot is re-pushed by ref.
        let g = s.lua.guard();
        push_userdata(s.lua, self.0).map_err(lua_err)?;
        let r = crate::refs::LuaRef::new(s.lua, -1).map_err(lua_err)?;
        drop(g);
        s.reference(r).map(|_| ())
    }
}

/// Clone out of userdata (requires `T: Clone`).
impl<T: UserData + Clone> FromLua for LuaUd<T> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        let slot = self_slot::<T>(lua, idx)?;
        slot.try_borrow()
            .ok()
            .and_then(|b| b.clone())
            .map(LuaUd)
            .ok_or_else(|| ConvError::msg(format!("{} unavailable", T::NAME)))
    }
}

/// Borrow of the userdata's value for the duration of an export call.
pub fn borrow<'l, T: UserData>(
    lua: &'l Lua,
    idx: c_int,
) -> Result<std::cell::Ref<'l, Option<T>>, ConvError> {
    self_slot::<T>(lua, idx)?
        .try_borrow()
        .map_err(|_| ConvError::msg(format!("{} is mutably borrowed", T::NAME)))
}

/// Autoref probe used by `#[derive(LuaUserData)]` / `#[rsgdll::methods]`.
#[doc(hidden)]
pub struct MethodsProbe<T>(pub std::marker::PhantomData<T>);

#[doc(hidden)]
pub trait NoMethods<T> {
    fn __rsgdll_methods(&self, _m: &mut UserDataMethods<T>) {}
}
impl<T> NoMethods<T> for &MethodsProbe<T> {}

#[doc(hidden)]
pub trait HasMethods<T> {
    fn __rsgdll_methods(&self, m: &mut UserDataMethods<T>);
}
