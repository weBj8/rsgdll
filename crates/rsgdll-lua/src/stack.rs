use rsgdll_platform::__private::RawLuaBase;

use crate::{FromLua, IntoLua, Lua, LuaCFunction, LuaError, LuaResult, LuaType};

/// Checked access to one borrowed Lua stack.
pub struct Stack<'stack, 'lua> {
    lua: &'stack mut Lua<'lua>,
}

impl<'stack, 'lua> Stack<'stack, 'lua> {
    pub(crate) const fn new(lua: &'stack mut Lua<'lua>) -> Self {
        Self { lua }
    }

    /// Returns current absolute stack height.
    #[must_use]
    pub fn top(&self) -> i32 {
        self.lua.top()
    }

    /// Returns one stack value's type tag.
    #[must_use]
    pub fn value_type(&self, index: i32) -> LuaType {
        self.lua.value_type(index)
    }

    /// Converts one stack value with Rust-side type validation.
    pub fn get<T: FromLua>(&self, index: i32) -> LuaResult<T> {
        T::from_lua(self.lua, index)
    }

    /// Starts a guard that restores this stack height when dropped.
    pub fn frame(&mut self) -> StackFrame<'_, 'lua> {
        StackFrame::new(self.lua)
    }
}

/// Restores a Lua stack to its entry height on every normal Rust exit path.
pub struct StackFrame<'guard, 'lua> {
    lua: &'guard mut Lua<'lua>,
    baseline: i32,
    finished: bool,
}

impl<'guard, 'lua> StackFrame<'guard, 'lua> {
    pub(crate) fn new(lua: &'guard mut Lua<'lua>) -> Self {
        Self {
            baseline: lua.top(),
            lua,
            finished: false,
        }
    }

    /// Returns current absolute stack height.
    #[must_use]
    pub fn top(&self) -> i32 {
        self.lua.top()
    }

    /// Returns one stack value's type tag.
    #[must_use]
    pub fn value_type(&self, index: i32) -> LuaType {
        self.lua.value_type(index)
    }

    /// Converts one stack value with Rust-side type validation.
    pub fn get<T: FromLua>(&self, index: i32) -> LuaResult<T> {
        T::from_lua(self.lua, index)
    }

    /// Pushes one Rust value.
    ///
    /// # Safety
    ///
    /// Caller must uphold [`IntoLua::into_lua`]'s no-longjmp contract.
    pub unsafe fn push<T: IntoLua>(&mut self, value: T) -> LuaResult<()> {
        // SAFETY: caller accepted the conversion's documented foreign-call
        // obligations.
        unsafe { value.into_lua(self.lua) }
    }

    /// Pushes a copy of an existing stack value.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth cannot raise a Lua error or longjmp.
    pub unsafe fn push_value(&mut self, index: i32) {
        // SAFETY: caller guarantees the foreign push cannot longjmp; any index
        // is accepted by the pinned Lua stack API.
        unsafe { RawLuaBase::push(self.lua.raw().as_ptr(), index) };
    }

    /// Creates and pushes an empty table.
    ///
    /// # Safety
    ///
    /// Caller must ensure allocation and stack growth cannot raise a Lua error
    /// or longjmp.
    pub unsafe fn create_table(&mut self) {
        // SAFETY: caller guarantees the allocating foreign call cannot
        // longjmp across this Rust frame.
        unsafe { RawLuaBase::create_table(self.lua.raw().as_ptr()) };
    }

    /// Performs raw table lookup using the key at stack top.
    ///
    /// # Safety
    ///
    /// Caller must ensure the foreign operation cannot raise a Lua error or
    /// longjmp. This method validates table type and frame-local key ownership.
    pub unsafe fn raw_get(&mut self, table_index: i32) -> LuaResult<()> {
        self.expect_table(table_index)?;
        self.require_frame_values(1)?;
        // SAFETY: table type and key availability were checked in Rust; caller
        // guarantees remaining foreign failure paths cannot longjmp.
        unsafe { RawLuaBase::raw_get(self.lua.raw().as_ptr(), table_index) };
        Ok(())
    }

    /// Performs raw table assignment using key and value at stack top.
    ///
    /// # Safety
    ///
    /// Caller must ensure the foreign operation cannot raise a Lua error or
    /// longjmp. This method validates table type and frame-local operands.
    pub unsafe fn raw_set(&mut self, table_index: i32) -> LuaResult<()> {
        self.expect_table(table_index)?;
        self.require_frame_values(2)?;
        // SAFETY: table type and both operands were checked in Rust; caller
        // guarantees remaining foreign failure paths cannot longjmp.
        unsafe { RawLuaBase::raw_set(self.lua.raw().as_ptr(), table_index) };
        Ok(())
    }

    /// Pushes a C function with no closure upvalues.
    ///
    /// # Safety
    ///
    /// Caller must ensure closure allocation and stack growth cannot raise a
    /// Lua error or longjmp.
    pub unsafe fn push_c_function(&mut self, function: LuaCFunction) -> LuaResult<()> {
        // SAFETY: caller accepted the no-longjmp contract; zero consumes no
        // frame values.
        unsafe { self.push_c_closure(function, 0) }
    }

    /// Consumes frame-owned values and pushes a C closure.
    ///
    /// # Safety
    ///
    /// Caller must ensure closure allocation and stack growth cannot raise a
    /// Lua error or longjmp.
    pub unsafe fn push_c_closure(
        &mut self,
        function: LuaCFunction,
        upvalue_count: usize,
    ) -> LuaResult<()> {
        self.require_frame_values(upvalue_count)?;
        let upvalue_count = i32::try_from(upvalue_count).map_err(|_| LuaError::CountOverflow)?;
        // SAFETY: upvalues are frame-owned and count fits the ABI; caller
        // guarantees closure allocation cannot longjmp.
        unsafe {
            RawLuaBase::push_c_closure(self.lua.raw().as_ptr(), function, upvalue_count);
        }
        Ok(())
    }

    /// Pops frame-owned values without crossing the frame baseline.
    pub fn pop(&mut self, count: usize) -> LuaResult<()> {
        let count = i32::try_from(count).map_err(|_| LuaError::CountOverflow)?;
        let current = self.top();
        let requested_top = current.checked_sub(count).ok_or(LuaError::CountOverflow)?;
        if requested_top < self.baseline {
            return Err(LuaError::StackUnderflow {
                baseline: self.baseline,
                requested_top,
            });
        }
        if count != 0 {
            // SAFETY: `count` is nonnegative and bounded by values above the
            // frame baseline; pinned `Pop` cannot remove caller-owned values.
            unsafe { RawLuaBase::pop(self.lua.raw().as_ptr(), count) };
        }
        Ok(())
    }

    /// Checks and restores the frame before consuming the guard.
    pub fn finish(mut self) -> LuaResult<()> {
        let result = self.restore();
        self.finished = true;
        result
    }

    fn restore(&mut self) -> LuaResult<()> {
        let current = self.top();
        if current < self.baseline {
            return Err(LuaError::StackUnderflow {
                baseline: self.baseline,
                requested_top: current,
            });
        }
        let extra = current - self.baseline;
        if extra != 0 {
            // SAFETY: `extra` is exactly the number of values above the saved
            // baseline, so pinned `Pop` restores that baseline.
            unsafe { RawLuaBase::pop(self.lua.raw().as_ptr(), extra) };
        }
        Ok(())
    }

    fn expect_table(&self, index: i32) -> LuaResult<()> {
        let actual = self.value_type(index);
        if actual == LuaType::TABLE {
            Ok(())
        } else {
            Err(LuaError::TypeMismatch {
                expected: LuaType::TABLE,
                actual,
            })
        }
    }

    fn require_frame_values(&self, count: usize) -> LuaResult<()> {
        let count = i32::try_from(count).map_err(|_| LuaError::CountOverflow)?;
        let requested_top = self
            .top()
            .checked_sub(count)
            .ok_or(LuaError::CountOverflow)?;
        if requested_top < self.baseline {
            Err(LuaError::StackUnderflow {
                baseline: self.baseline,
                requested_top,
            })
        } else {
            Ok(())
        }
    }
}

impl Drop for StackFrame<'_, '_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.restore();
        }
    }
}
