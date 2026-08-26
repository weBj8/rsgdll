use rsgdll_platform::__private::RawLuaBase;

use crate::{
    FromLua, IntoLua, Lua, LuaCFunction, LuaError, LuaFunction, LuaResult, LuaTable, LuaType,
    RegistryReference, UserDataType,
};

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

    /// Creates an empty registry-owned table.
    ///
    /// # Safety
    ///
    /// Caller must ensure table allocation, stack growth, and registry
    /// insertion cannot raise a Lua error or longjmp.
    pub unsafe fn new_table(&mut self) -> LuaResult<LuaTable<'lua>> {
        // SAFETY: caller accepts table allocation's no-longjmp obligation.
        unsafe { self.create_table() };
        // SAFETY: the just-created table is live at stack top and caller
        // accepts registry insertion's no-longjmp obligation.
        let reference = unsafe { self.create_reference(-1)? };
        self.pop(1)?;
        Ok(LuaTable::new(reference))
    }

    /// Creates a registry reference to one existing stack value.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth and registry insertion cannot raise a
    /// Lua error or longjmp.
    pub unsafe fn create_reference(&mut self, index: i32) -> LuaResult<RegistryReference<'lua>> {
        if self.value_type(index) == LuaType::NONE {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::NIL,
                actual: LuaType::NONE,
            });
        }
        // SAFETY: caller guarantees stack growth cannot longjmp.
        unsafe { self.push_value(index) };
        // SAFETY: copied stack value is live; caller guarantees registry
        // insertion cannot longjmp.
        let id = unsafe { RawLuaBase::reference_create(self.lua.raw().as_ptr()) };
        Ok(RegistryReference::new(self.lua.raw(), id))
    }

    /// Captures one function in the Lua registry.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth and registry insertion cannot raise a
    /// Lua error or longjmp.
    pub unsafe fn function(&mut self, index: i32) -> LuaResult<LuaFunction<'lua>> {
        let actual = self.value_type(index);
        if actual != LuaType::FUNCTION {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::FUNCTION,
                actual,
            });
        }
        // SAFETY: type was checked and caller accepts registry operations.
        let reference = unsafe { self.create_reference(index)? };
        Ok(LuaFunction::new(reference))
    }

    /// Registers or retrieves one named Rust userdata type.
    ///
    /// # Safety
    ///
    /// Caller must ensure metatable creation cannot raise a Lua error or
    /// longjmp.
    pub unsafe fn userdata_type<T: 'static>(
        &mut self,
        name: &str,
    ) -> LuaResult<UserDataType<'lua, T>> {
        // SAFETY: caller accepts metatable creation's no-longjmp obligation.
        unsafe { UserDataType::register(self, name) }
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

    /// Advances Lua table iteration using the key at stack top.
    ///
    /// Returns `true` after replacing the key with the next key/value pair, or
    /// `false` after consuming the final key.
    ///
    /// # Safety
    ///
    /// Caller must ensure the foreign operation cannot raise a Lua error or
    /// longjmp.
    pub unsafe fn next(&mut self, table_index: i32) -> LuaResult<bool> {
        self.expect_table(table_index)?;
        self.require_frame_values(1)?;
        // SAFETY: table/key presence was checked and caller accepts foreign
        // iteration's no-longjmp obligation.
        Ok(unsafe { RawLuaBase::next(self.lua.raw().as_ptr(), table_index) } != 0)
    }

    pub(crate) fn absolute_index(&self, index: i32) -> LuaResult<i32> {
        if index > 0 {
            return Ok(index);
        }
        if index == 0 || index <= -10_000 {
            return Err(LuaError::CountOverflow);
        }
        self.top()
            .checked_add(index)
            .and_then(|offset| offset.checked_add(1))
            .ok_or(LuaError::CountOverflow)
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

    /// Validates and preserves callback return values before consuming the guard.
    pub fn commit(mut self, expected_returns: usize) -> LuaResult<i32> {
        let expected = i32::try_from(expected_returns).map_err(|_| LuaError::CountOverflow)?;
        let current = self.top();
        let actual = current
            .checked_sub(self.baseline)
            .ok_or(LuaError::StackUnderflow {
                baseline: self.baseline,
                requested_top: current,
            })?;
        if actual != expected {
            return Err(LuaError::ReturnCountMismatch { expected, actual });
        }
        self.finished = true;
        Ok(actual)
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

    pub(crate) const fn raw(&self) -> std::ptr::NonNull<RawLuaBase> {
        self.lua.raw()
    }

    #[doc(hidden)]
    pub const fn lua(&self) -> &Lua<'lua> {
        self.lua
    }
}

impl Drop for StackFrame<'_, '_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.restore();
        }
    }
}
