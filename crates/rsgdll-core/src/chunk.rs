//! Runtime of `lua!` (spec §8.3): per-site chunk cache keyed by a `static` address.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;

use rsgdll_sys::lua::LUA_MULTRET;

use crate::conv::{FromLuaMulti, IntoLuaMulti, Stage};
use crate::error::{Error, Result};
use crate::lua::Lua;
use crate::protected::Op;
use crate::refs::LuaRef;

type Cache = RefCell<HashMap<usize, LuaRef>>;

/// Compiled chunk for `site` in this state, compiling it on first use.
fn chunk(
    lua: &Lua,
    site: &'static u8,
    src: &str,
    name: &'static CStr,
) -> Result<std::rc::Rc<Cache>> {
    let cache = lua
        .runtime()
        .ext::<Cache>("chunks", || RefCell::new(HashMap::new()));
    let key = site as *const u8 as usize;
    let cached = cache.borrow().contains_key(&key);
    if !cached {
        let g = lua.guard();
        lua.run(
            &[],
            &[
                Op::CheckStack(4),
                Op::Load(src.as_ptr(), src.len(), name.as_ptr()),
            ],
        )?;
        let r = LuaRef::new(lua, -1)?;
        drop(g);
        cache.borrow_mut().insert(key, r);
    }
    Ok(cache)
}

/// Runs a `lua!` chunk with captures as `...`; Lua errors carry the Lua traceback and
/// the Rust call site (`at file:line:col`).
pub fn run<R: FromLuaMulti, A: IntoLuaMulti>(
    lua: &Lua,
    site: &'static u8,
    src: &str,
    name: &'static CStr,
    args: A,
) -> Result<R> {
    let at = || format!("lua! at {}", name.to_string_lossy().trim_start_matches('='));
    let cache = chunk(lua, site, src, name).map_err(|e| e.context(at()))?;
    let g = lua.guard();
    let mut s = Stage::new(lua);
    {
        let map = cache.borrow();
        let f = map
            .get(&(site as *const u8 as usize))
            .ok_or_else(|| Error::msg("lua!: chunk cache miss"))?;
        s.reference_id(f)?;
    }
    let n = args.into_lua_multi(&mut s)?;
    s.op(Op::Call(n, LUA_MULTRET), -n - 1);
    let r = s
        .finish()
        .and_then(|(first, count)| R::from_lua_multi(lua, first, count).map_err(Error::from));
    drop(g);
    r.map_err(|e| e.context(at()))
}

/// Capture argument: borrowed Rust value converted for one call.
pub trait Capture {
    fn stage(self, s: &mut Stage<'_>) -> std::result::Result<(), crate::ConvError>;
}

impl<T: Clone + crate::conv::IntoLua> Capture for &T {
    fn stage(self, s: &mut Stage<'_>) -> std::result::Result<(), crate::ConvError> {
        self.clone().into_lua(s)
    }
}

macro_rules! ref_captures {
    ($($t:ty),*) => {$(
        impl Capture for &$t {
            fn stage(self, s: &mut Stage<'_>) -> std::result::Result<(), crate::ConvError> {
                crate::conv::IntoLua::into_lua(self, s)
            }
        }
    )*};
}
ref_captures!(
    crate::conv::Player,
    crate::conv::Entity,
    crate::conv::LuaTable,
    crate::conv::LuaFunction,
    crate::conv::LuaAny
);

/// Captures packed by the macro.
pub struct Captures<T>(pub T);

macro_rules! capture_tuples {
    ($( ($($t:ident $i:tt),*) )*) => {$(
        impl<$($t: Capture),*> IntoLuaMulti for Captures<($($t,)*)> {
            #[allow(unused_variables)]
            fn into_lua_multi(self, s: &mut Stage<'_>) -> std::result::Result<core::ffi::c_int, crate::ConvError> {
                $( (self.0).$i.stage(s)?; )*
                Ok(0 $( + { let _ = $i; 1 } )*)
            }
        }
    )*};
}
capture_tuples! {
    ()
    (A 0)
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11)
}

/// Handle types accepted by `lua!` (`&Lua`, `&LocalLua`, spec §8.3).
pub trait AsLua {
    fn as_lua(&self) -> Result<&Lua>;
}

impl AsLua for Lua {
    fn as_lua(&self) -> Result<&Lua> {
        Ok(self)
    }
}

impl<T: AsLua + ?Sized> AsLua for &T {
    fn as_lua(&self) -> Result<&Lua> {
        (**self).as_lua()
    }
}
