use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;

use rsgdll_platform::__private::{RawLuaBase, RawLuaState};

use crate::{LuaError, LuaResult, LuaType, Stack};

const GLOBAL_INDEX: i32 = -10_002;

/// Lifetime-bound access to one main-thread Lua state.
///
/// `Lua` is neither [`Send`] nor [`Sync`]. It must be passed explicitly from
/// the callback boundary that owns the underlying state.
pub struct Lua<'lua> {
    raw: NonNull<RawLuaBase>,
    _state: PhantomData<&'lua mut RawLuaState>,
    _main_thread: PhantomData<Rc<()>>,
}

impl<'lua> Lua<'lua> {
    /// Creates a checked handle for a callback's raw Lua state.
    ///
    /// # Safety
    ///
    /// `state` must be null or point to a live pinned-layout Garry's Mod
    /// `lua_State` for all of `'lua`. The caller must provide exclusive
    /// main-thread access for that lifetime and ensure no Lua longjmp or C++
    /// exception crosses this constructor.
    pub unsafe fn from_raw(state: *mut RawLuaState) -> LuaResult<Self> {
        let state = NonNull::new(state).ok_or(LuaError::NullState)?;
        // SAFETY: caller guarantees a live pinned-layout state for `'lua`.
        let lua_base = unsafe { RawLuaState::lua_base(state.as_ptr()) };
        let raw = NonNull::new(lua_base).ok_or(LuaError::NullLuaBase)?;
        Ok(Self {
            raw,
            _state: PhantomData,
            _main_thread: PhantomData,
        })
    }

    /// Returns current absolute stack height.
    #[must_use]
    pub fn top(&self) -> i32 {
        // SAFETY: constructor guarantees a live `ILuaBase`; pinned `Top` only
        // inspects stack state and does not raise Lua errors.
        unsafe { RawLuaBase::top(self.raw.as_ptr()) }
    }

    /// Returns a stack value's raw Lua/GMod type tag.
    #[must_use]
    pub fn value_type(&self, index: i32) -> LuaType {
        // SAFETY: constructor guarantees a live `ILuaBase`; pinned `GetType`
        // accepts any Lua stack index and does not invoke Lua error APIs.
        LuaType(unsafe { RawLuaBase::get_type(self.raw.as_ptr(), index) })
    }

    /// Borrows this state's checked stack abstraction.
    pub fn stack(&mut self) -> Stack<'_, 'lua> {
        Stack::new(self)
    }

    /// Returns the pinned pseudo-index for a one-based closure upvalue.
    pub const fn upvalue_index(position: u8) -> LuaResult<i32> {
        if position == 0 {
            Err(LuaError::InvalidUpvaluePosition)
        } else {
            Ok(GLOBAL_INDEX - position as i32)
        }
    }

    pub(crate) const fn raw(&self) -> NonNull<RawLuaBase> {
        self.raw
    }
}
