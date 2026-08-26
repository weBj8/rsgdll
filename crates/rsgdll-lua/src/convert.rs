use std::ffi::c_uint;

use rsgdll_platform::__private::RawLuaBase;

use crate::{Lua, LuaError, LuaResult, LuaType};

/// Converts one checked Lua stack value into an owned Rust value.
pub trait FromLua: Sized {
    /// Reads `index` without using Lua's throwing `Check*` APIs.
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self>;
}

/// Pushes one Rust value onto a Lua stack.
pub trait IntoLua {
    /// Pushes this value.
    ///
    /// # Safety
    ///
    /// The caller must ensure the pinned foreign push operation cannot raise a
    /// Lua error or longjmp, including allocation and stack-growth failures.
    unsafe fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()>;
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
        expect_type(lua, index, LuaType::STRING)?;
        let mut length: c_uint = 0;
        // SAFETY: exact string validation prevents number-to-string coercion;
        // `length` is writable and the live Lua value retains returned bytes.
        let bytes = unsafe { RawLuaBase::get_string(lua.raw().as_ptr(), index, &raw mut length) };
        let bytes = std::ptr::NonNull::new(bytes.cast_mut()).ok_or(LuaError::NullStringPointer)?;
        // SAFETY: [UB categories 3, 8, 10] Pinned `GetString` returned a
        // readable string allocation with exactly `length` bytes. We copy it
        // before any stack mutation can release the Lua value.
        let bytes = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), length as usize) };
        let value = std::str::from_utf8(bytes).map_err(|_| LuaError::InvalidUtf8)?;
        Ok(value.to_owned())
    }
}

impl FromLua for () {
    fn from_lua(lua: &Lua<'_>, index: i32) -> LuaResult<Self> {
        expect_type(lua, index, LuaType::NIL)
    }
}

impl IntoLua for bool {
    unsafe fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        // SAFETY: caller guarantees this foreign push cannot longjmp.
        unsafe { RawLuaBase::push_bool(lua.raw().as_ptr(), self) };
        Ok(())
    }
}

impl IntoLua for f64 {
    unsafe fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        // SAFETY: caller guarantees this foreign push cannot longjmp.
        unsafe { RawLuaBase::push_number(lua.raw().as_ptr(), self) };
        Ok(())
    }
}

impl IntoLua for () {
    unsafe fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        // SAFETY: caller guarantees this foreign push cannot longjmp.
        unsafe { RawLuaBase::push_nil(lua.raw().as_ptr()) };
        Ok(())
    }
}

impl IntoLua for &str {
    unsafe fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        let length = c_uint::try_from(self.len()).map_err(|_| LuaError::StringTooLong)?;
        let empty = [0_u8];
        let bytes = if self.is_empty() {
            empty.as_ptr()
        } else {
            self.as_ptr()
        };
        // SAFETY: pointer is readable for `length` bytes; empty strings use a
        // NUL byte because upstream interprets zero length via `strlen`.
        // Caller guarantees allocation or stack growth cannot longjmp.
        unsafe { RawLuaBase::push_string(lua.raw().as_ptr(), bytes.cast(), length) };
        Ok(())
    }
}

impl IntoLua for String {
    unsafe fn into_lua(self, lua: &mut Lua<'_>) -> LuaResult<()> {
        // SAFETY: caller's longjmp exclusion applies while `self` is borrowed;
        // `PushString` copies the bytes before returning.
        unsafe { self.as_str().into_lua(lua) }
    }
}

fn expect_type(lua: &Lua<'_>, index: i32, expected: LuaType) -> LuaResult<()> {
    let actual = lua.value_type(index);
    if actual == expected {
        Ok(())
    } else {
        Err(LuaError::TypeMismatch { expected, actual })
    }
}
