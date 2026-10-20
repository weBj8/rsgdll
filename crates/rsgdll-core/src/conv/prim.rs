//! Primitive conversions (spec §6.2): nil, bool, numbers (checked to 2^53), strings.

use core::ffi::c_int;
use std::borrow::Cow;

use rsgdll_sys::lua::*;

use super::{FromArg, FromLua, IntoLua, Stage, mismatch};
use crate::error::ConvError;
use crate::lua::Lua;
use crate::protected::Op;

/// Largest integer exactly representable in an `f64`.
pub const MAX_SAFE: i128 = 1 << 53;

/// Explicit Lua `nil`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nil;

impl FromLua for Nil {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        match lua.read(idx).tag {
            LUA_TNIL | LUA_TNONE => Ok(Nil),
            _ => Err(mismatch(lua, idx, "nil")),
        }
    }
}

impl IntoLua for Nil {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.nil();
        Ok(())
    }
}

impl FromLua for bool {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        let r = lua.read(idx);
        match r.tag {
            LUA_TBOOLEAN => Ok(r.num != 0.0),
            _ => Err(mismatch(lua, idx, "boolean")),
        }
    }
}

impl IntoLua for bool {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.bool(self);
        Ok(())
    }
}

/// Number with `luaL_checknumber`-style string coercion (`"5"` -> 5).
fn number(lua: &Lua, idx: c_int) -> Result<f64, ConvError> {
    let r = lua.read(idx);
    match r.tag {
        LUA_TNUMBER => Ok(r.num),
        LUA_TSTRING => lua
            .raw_bytes(idx)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|s| s.trim().parse::<f64>().ok())
            .ok_or_else(|| mismatch(lua, idx, "number")),
        _ => Err(mismatch(lua, idx, "number")),
    }
}

impl FromLua for f64 {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        number(lua, idx)
    }
}

impl IntoLua for f64 {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.number(self);
        Ok(())
    }
}

impl FromLua for f32 {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        number(lua, idx).map(|n| n as f32) // lossy by design (spec §6.2)
    }
}

impl IntoLua for f32 {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.number(f64::from(self));
        Ok(())
    }
}

/// Lua number -> integer: must be integral and in range.
pub fn to_int(n: f64, min: i128, max: i128, name: &str) -> Result<i128, ConvError> {
    let ok = n.is_finite() && n.fract() == 0.0 && (n as i128) >= min && (n as i128) <= max;
    ok.then_some(n as i128)
        .ok_or_else(|| ConvError::new(format!("{name} (integer in range)"), format!("number {n}")))
}

/// Integer -> Lua number: errors when `|v| > 2^53` (never silently lossy).
pub fn from_int(v: i128) -> Result<f64, ConvError> {
    (-MAX_SAFE..=MAX_SAFE)
        .contains(&v)
        .then_some(v as f64)
        .ok_or_else(|| {
            ConvError::msg(format!(
                "integer {v} exceeds 2^53 and cannot be a Lua number; use #[lua(as_string)]"
            ))
        })
}

macro_rules! ints {
    ($($t:ty),*) => {$(
        impl FromLua for $t {
            fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
                number(lua, idx)
                    .and_then(|n| to_int(n, <$t>::MIN as i128, <$t>::MAX as i128, stringify!($t)))
                    .map(|v| v as $t)
            }
        }
        impl IntoLua for $t {
            fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
                from_int(self as i128).map(|n| { s.number(n); })
            }
        }
    )*};
}
ints!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl FromLua for i128 {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        number(lua, idx).and_then(|n| to_int(n, -MAX_SAFE, MAX_SAFE, "i128"))
    }
}

impl IntoLua for i128 {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        from_int(self).map(|n| {
            s.number(n);
        })
    }
}

/// String bytes; numbers are coerced like `luaL_checkstring` (allocates, so protected).
/// The slot is converted in place, like Lua does.
pub fn string_bytes(lua: &Lua, idx: c_int) -> Result<&[u8], ConvError> {
    let idx = lua.abs(idx);
    match lua.read(idx).tag {
        LUA_TSTRING => lua
            .raw_bytes(idx)
            .ok_or_else(|| mismatch(lua, idx, "string")),
        LUA_TNUMBER => {
            // Coerce on a copy and write it back so the slot (and its bytes) stay alive.
            let g = lua.guard();
            lua.read_ops::<0>(&[idx], &[Op::ToStr(1, 0)]).map(|_| ())?;
            // SAFETY: frame result (the string) is on top; replace never raises.
            unsafe { (lua.api().lua_replace)(lua.raw(), idx) };
            drop(g);
            lua.raw_bytes(idx)
                .ok_or_else(|| mismatch(lua, idx, "string"))
        }
        _ => Err(mismatch(lua, idx, "string")),
    }
}

fn utf8(b: &[u8]) -> Result<&str, ConvError> {
    std::str::from_utf8(b).map_err(|_| ConvError::msg("expected UTF-8 string"))
}

impl FromLua for String {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        string_bytes(lua, idx).and_then(utf8).map(str::to_owned)
    }
}

impl<'a> FromArg<'a> for &'a str {
    fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError> {
        string_bytes(lua, idx).and_then(utf8)
    }
}

impl<'a> FromArg<'a> for &'a [u8] {
    fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError> {
        string_bytes(lua, idx)
    }
}

impl<'a> FromArg<'a> for Cow<'a, str> {
    fn from_arg(lua: &'a Lua, idx: c_int) -> Result<Self, ConvError> {
        <&str>::from_arg(lua, idx).map(Cow::Borrowed)
    }
}

impl IntoLua for String {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.bytes(self.into_bytes());
        Ok(())
    }
}

impl IntoLua for &str {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.bytes(self.as_bytes());
        Ok(())
    }
}

impl IntoLua for Cow<'_, str> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.bytes(self.as_bytes());
        Ok(())
    }
}

impl IntoLua for &String {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        self.as_str().into_lua(s)
    }
}

/// Binary-safe string (net messages, files).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LuaBytes(pub Vec<u8>);

impl FromLua for LuaBytes {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        string_bytes(lua, idx).map(|b| LuaBytes(b.to_vec()))
    }
}

impl IntoLua for LuaBytes {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.bytes(self.0);
        Ok(())
    }
}

impl IntoLua for &[u8] {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        s.bytes(self);
        Ok(())
    }
}

/// 64-bit integer carried as a decimal string (SteamID64 etc., `#[lua(as_string)]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AsString<T>(pub T);

impl<T: std::str::FromStr> FromLua for AsString<T> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        let s = String::from_lua(lua, idx)?;
        s.trim()
            .parse()
            .map(AsString)
            .map_err(|_| ConvError::new("integer string", format!("\"{s}\"")))
    }
}

impl<T: std::fmt::Display> IntoLua for AsString<T> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        self.0.to_string().into_lua(s)
    }
}

/// A SteamID64 (`"76561198000000000"`), string-backed in Lua.
pub type SteamId64 = AsString<u64>;

impl<T: FromLua> FromLua for Option<T> {
    fn from_lua(lua: &Lua, idx: c_int) -> Result<Self, ConvError> {
        match lua.read(idx).tag {
            LUA_TNIL | LUA_TNONE => Ok(None),
            _ => T::from_lua(lua, idx).map(Some),
        }
    }
}

impl<T: IntoLua> IntoLua for Option<T> {
    fn into_lua(self, s: &mut Stage<'_>) -> Result<(), ConvError> {
        match self {
            Some(v) => v.into_lua(s),
            None => {
                s.nil();
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_checks() {
        assert_eq!(to_int(3.0, 0, 255, "u8"), Ok(3));
        assert!(to_int(3.5, 0, 255, "u8").is_err());
        assert!(to_int(256.0, 0, 255, "u8").is_err());
        assert!(from_int(MAX_SAFE).is_ok());
        assert!(from_int(MAX_SAFE + 1).is_err());
    }
}
