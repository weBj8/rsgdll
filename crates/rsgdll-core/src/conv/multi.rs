//! Multiple values (spec §6.2): tuples, `Variadic<T>`, export argument lists.

use core::ffi::c_int;

use super::{FromArg, FromLua, IntoLua, Stage};
use crate::error::ConvError;
use crate::lua::Lua;

/// Remaining values of a call (`...`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Variadic<T>(pub Vec<T>);

impl<T> std::ops::Deref for Variadic<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

/// Values starting at stack index `first` (`count` of them) -> Rust.
pub trait FromLuaMulti: Sized {
    fn from_lua_multi(lua: &Lua, first: c_int, count: c_int) -> Result<Self, ConvError>;
}

/// Rust -> any number of staged values; returns how many.
pub trait IntoLuaMulti {
    fn into_lua_multi(self, s: &mut Stage<'_>) -> Result<c_int, ConvError>;
}

/// Export arguments: each element knows its 1-based position for `bad argument #N`.
pub trait FromArgs<'a>: Sized {
    /// `Err((arg_number, error))`.
    fn from_args(lua: &'a Lua, first: c_int, count: c_int) -> Result<Self, (c_int, ConvError)>;
}

/// Single-value position helper: missing args read as `none` (Lua semantics).
fn at(first: c_int, count: c_int, i: c_int) -> c_int {
    match i < count {
        true => first + i,
        // Beyond the frame: an index above top reads as LUA_TNONE without raising.
        false => first + count.max(0) + 1_000_000,
    }
}

impl<T: FromLua> FromLuaMulti for T {
    fn from_lua_multi(lua: &Lua, first: c_int, count: c_int) -> Result<Self, ConvError> {
        T::from_lua(lua, valid(lua, at(first, count, 0)))
    }
}

/// Indexes past the top must stay acceptable (`lua_type` returns NONE up to stack size);
/// map out-of-frame indices to the slot just above top, which is always NONE.
fn valid(lua: &Lua, idx: c_int) -> c_int {
    if idx > lua.top() { lua.top() + 1 } else { idx }
}

impl<T: FromLua> FromLuaMulti for Variadic<T> {
    fn from_lua_multi(lua: &Lua, first: c_int, count: c_int) -> Result<Self, ConvError> {
        (0..count.max(0)).map(|i| T::from_lua(lua, first + i).map_err(|e| e.at(format!("#{}", i + 1)))).collect::<Result<_, _>>().map(Variadic)
    }
}

impl<T: IntoLua> IntoLuaMulti for T {
    fn into_lua_multi(self, s: &mut Stage<'_>) -> Result<c_int, ConvError> {
        self.into_lua(s).map(|()| 1)
    }
}

impl<T: IntoLua> IntoLuaMulti for Variadic<T> {
    fn into_lua_multi(self, s: &mut Stage<'_>) -> Result<c_int, ConvError> {
        let n = self.0.len();
        self.0.into_iter().try_for_each(|v| v.into_lua(s))?;
        Ok(c_int::try_from(n).unwrap_or(c_int::MAX))
    }
}

macro_rules! tuples {
    ($( ($($t:ident $i:tt),*) )*) => {$(
        impl<$($t: FromLua),*> FromLuaMulti for ($($t,)*) {
            #[allow(unused_variables)]
            fn from_lua_multi(lua: &Lua, first: c_int, count: c_int) -> Result<Self, ConvError> {
                Ok(($( $t::from_lua(lua, valid(lua, at(first, count, $i))).map_err(|e| e.at(format!("#{}", $i + 1)))?, )*))
            }
        }
        impl<$($t: IntoLua),*> IntoLuaMulti for ($($t,)*) {
            #[allow(unused_variables)]
            fn into_lua_multi(self, s: &mut Stage<'_>) -> Result<c_int, ConvError> {
                $( self.$i.into_lua(s)?; )*
                Ok(0 $( + { let _ = $i; 1 } )*)
            }
        }
        impl<'a, $($t: FromArg<'a>),*> FromArgs<'a> for ($($t,)*) {
            #[allow(unused_variables)]
            fn from_args(lua: &'a Lua, first: c_int, count: c_int) -> Result<Self, (c_int, ConvError)> {
                Ok(($( $t::from_arg(lua, valid(lua, at(first, count, $i))).map_err(|e| ($i + 1, e))?, )*))
            }
        }
    )*};
}

tuples! {
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

/// Converts the trailing `Variadic` of an export starting at 0-based position `from`.
pub fn variadic_from<T: FromLua>(lua: &Lua, first: c_int, count: c_int, from: c_int) -> Result<Variadic<T>, (c_int, ConvError)> {
    (from..count.max(from))
        .map(|i| T::from_lua(lua, first + i).map_err(|e| (i + 1, e)))
        .collect::<Result<_, _>>()
        .map(Variadic)
}

/// One export argument at 0-based position `i` (macro helper).
pub fn arg<'a, T: FromArg<'a>>(lua: &'a Lua, first: c_int, count: c_int, i: c_int) -> Result<T, (c_int, ConvError)> {
    T::from_arg(lua, valid(lua, at(first, count, i))).map_err(|e| (i + 1, e))
}

/// Two multi-values pushed back to back (`(event, ...args)`).
pub struct Chain<A, B>(pub A, pub B);

impl<A: IntoLuaMulti, B: IntoLuaMulti> IntoLuaMulti for Chain<A, B> {
    fn into_lua_multi(self, s: &mut Stage<'_>) -> Result<c_int, ConvError> {
        Ok(self.0.into_lua_multi(s)? + self.1.into_lua_multi(s)?)
    }
}
