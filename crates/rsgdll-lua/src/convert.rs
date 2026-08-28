use std::ffi::c_uint;

use rsgdll_platform::__private::RawLuaBase;

use crate::{Lua, LuaBytes, LuaError, LuaResult, LuaType, protected};

pub(crate) const MAX_EXACT_LUA_INTEGER: f64 = (1_u64 << 53) as f64;

/// Converts one checked Lua stack value into an owned Rust value.
pub trait FromLua: Sized {
    /// Reads `index` without using Lua's throwing `Check*` APIs.
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self>;
}

/// Pushes one Rust value onto a Lua stack.
pub trait IntoLua {
    /// Pushes this value.
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()>;
}

impl FromLua for bool {
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
        expect_type(lua, index, LuaType::BOOL)?;
        // SAFETY: exact type validation above prevents coercion; pinned
        // `GetBool` only reads the existing boolean.
        Ok(unsafe { RawLuaBase::get_bool(lua.raw().as_ptr(), index) })
    }
}

impl FromLua for f64 {
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
        expect_type(lua, index, LuaType::NUMBER)?;
        // SAFETY: exact type validation above prevents conversion; pinned
        // `GetNumber` only reads the existing number.
        Ok(unsafe { RawLuaBase::get_number(lua.raw().as_ptr(), index) })
    }
}

impl FromLua for String {
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
        let bytes = string_bytes(lua, index)?;
        let value = std::str::from_utf8(&bytes).map_err(|_| LuaError::InvalidUtf8)?;
        Ok(value.to_owned())
    }
}

impl FromLua for LuaBytes {
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
        string_bytes(lua, index).map(Self::from)
    }
}

impl FromLua for () {
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
        expect_type(lua, index, LuaType::NIL)
    }
}

impl IntoLua for bool {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        protected::push_bool(lua.context(), self)
    }
}

impl IntoLua for f64 {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        protected::push_number(lua.context(), self)
    }
}

macro_rules! impl_integer_conversions {
    ($($integer:ty),+ $(,)?) => {
        $(
            impl FromLua for $integer {
                fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
                    let value = f64::from_lua(lua, index)?;
                    if value.fract() == 0.0
                        && (-MAX_EXACT_LUA_INTEGER..=MAX_EXACT_LUA_INTEGER).contains(&value)
                        && (<$integer>::MIN as f64..=<$integer>::MAX as f64).contains(&value)
                    {
                        Ok(value as Self)
                    } else {
                        Err(LuaError::IntegerOutOfRange)
                    }
                }
            }

            impl IntoLua for $integer {
                fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
                    let value = self as f64;
                    if (-MAX_EXACT_LUA_INTEGER..=MAX_EXACT_LUA_INTEGER).contains(&value)
                        && value as Self == self
                    {
                        value.into_lua(lua)
                    } else {
                        Err(LuaError::IntegerOutOfRange)
                    }
                }
            }
        )+
    };
}

impl_integer_conversions!(u8, u16, u32, u64, i8, i16, i32, i64);

impl IntoLua for () {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        protected::push_nil(lua.context())
    }
}

impl IntoLua for &str {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        self.as_bytes().into_lua(lua)
    }
}

impl IntoLua for String {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        self.as_str().into_lua(lua)
    }
}

impl IntoLua for &[u8] {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        let length = c_uint::try_from(self.len()).map_err(|_| LuaError::StringTooLong)?;
        let empty = [0_u8];
        let bytes = if self.is_empty() {
            empty.as_ptr()
        } else {
            self.as_ptr()
        };
        protected::push_string(lua.context(), bytes, length)
    }
}

impl IntoLua for Vec<u8> {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        self.as_slice().into_lua(lua)
    }
}

impl IntoLua for LuaBytes {
    fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        self.as_bytes().into_lua(lua)
    }
}

fn string_bytes(lua: &Lua<'_>, index: i32) -> LuaResult<Vec<u8>> {
    expect_type(lua, index, LuaType::STRING)?;
    let mut length: c_uint = 0;
    // SAFETY: exact string validation prevents number-to-string coercion;
    // `length` is writable and the live Lua value retains returned bytes.
    let bytes = unsafe { RawLuaBase::get_string(lua.raw().as_ptr(), index, &raw mut length) };
    let bytes = std::ptr::NonNull::new(bytes.cast_mut()).ok_or(LuaError::NullStringPointer)?;
    // SAFETY: [UB categories 3, 8, 10] Pinned `GetString` returned a readable
    // allocation with exactly `length` bytes. Copy occurs before stack mutation.
    let bytes = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), length as usize) };
    Ok(bytes.to_vec())
}

fn expect_type(lua: &Lua<'_>, index: i32, expected: LuaType) -> LuaResult<()> {
    let actual = lua.value_type(index);
    if actual == expected {
        Ok(())
    } else {
        Err(LuaError::TypeMismatch { expected, actual })
    }
}
