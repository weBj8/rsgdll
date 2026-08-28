//! Checked, main-thread-bound Lua abstractions.

#![deny(unsafe_op_in_unsafe_fn)]

mod convert;
#[cfg(feature = "debug")]
mod debug;
mod error;
mod function;
mod lua;
mod multi;
mod protected;
mod reference;
#[cfg(feature = "serde")]
pub mod serde;
mod stack;
mod string;
mod table;
mod userdata;

pub use convert::{FromLua, IntoLua};
#[cfg(feature = "debug")]
pub use debug::{
    DebugContext, DebugEvent, DebugFrame, DebugFrameInfo, DebugHook, DebugHookGuard, DebugLocal,
    DebugMask, DebugUpvalue,
};
pub use error::{LuaError, LuaResult};
pub use function::LuaFunction;
pub use lua::Lua;
pub use multi::{FromLuaMulti, IntoLuaMulti};
pub use reference::RegistryReference;
pub use rsgdll_platform::__private::{LuaCFunction, LuaType};
pub use stack::{Stack, StackFrame};
pub use string::LuaBytes;
pub use table::LuaTable;
pub use userdata::UserDataType;

#[doc(hidden)]
pub mod __private {
    use rsgdll_platform::__private::{LuaCFunction, RawLuaState};

    use crate::{Lua, LuaResult, StackFrame, UserDataType};

    /// Constructs a Lua handle at the callback boundary.
    ///
    /// # Safety
    ///
    /// `state` must be null or point to a live pinned-layout Garry's Mod
    /// `lua_State` for the returned handle's lifetime, with exclusive
    /// main-thread access.
    pub unsafe fn from_raw<'lua>(state: *mut RawLuaState) -> LuaResult<Lua<'lua>> {
        // SAFETY: callers uphold the callback-state lifetime contract above.
        unsafe { Lua::from_raw(state) }
    }

    /// Pushes framework-generated C closure plumbing.
    ///
    /// # Safety
    ///
    /// `function` must obey the Lua C callback ABI and may not unwind.
    pub unsafe fn push_c_closure(
        frame: &mut StackFrame<'_, '_>,
        function: LuaCFunction,
        upvalue_count: usize,
    ) -> LuaResult<()> {
        frame.push_c_closure(function, upvalue_count)
    }

    /// Returns the registered foreign userdata type tag.
    #[must_use]
    pub const fn userdata_type_id<T: 'static>(userdata: &UserDataType<'_, T>) -> u8 {
        userdata.type_id()
    }

    /// Runs generated userdata finalization after provenance validation.
    pub fn finalize_userdata<T: 'static>(
        frame: &mut StackFrame<'_, '_>,
        index: i32,
        lua_type: u8,
    ) -> LuaResult<()> {
        UserDataType::<T>::finalize_registered(frame, index, lua_type)
    }
}
