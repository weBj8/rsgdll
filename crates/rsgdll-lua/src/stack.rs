use rsgdll_platform::__private::RawLuaBase;

use crate::{
    FromLua, IntoLua, Lua, LuaCFunction, LuaError, LuaFunction, LuaResult, LuaTable, LuaType,
    RegistryReference, UserDataType, protected,
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
    pub fn push<T: IntoLua>(&mut self, value: T) -> LuaResult<()> {
        value.into_lua(self.lua)
    }

    /// Pushes a copy of an existing stack value.
    pub(crate) fn push_value(&mut self, index: i32) -> LuaResult<()> {
        protected::push(self.lua.context(), index)
    }

    /// Creates and pushes an empty table.
    pub fn create_table(&mut self) -> LuaResult<()> {
        protected::create_table(self.lua.context())
    }

    /// Creates an empty registry-owned table.
    pub fn new_table(&mut self) -> LuaResult<LuaTable<'lua>> {
        self.create_table()?;
        let reference = self.create_reference(-1)?;
        self.pop(1)?;
        Ok(LuaTable::new(reference))
    }

    /// Creates a registry reference to one existing stack value.
    pub fn create_reference(&mut self, index: i32) -> LuaResult<RegistryReference<'lua>> {
        if self.value_type(index) == LuaType::NONE {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::NIL,
                actual: LuaType::NONE,
            });
        }
        self.push_value(index)?;
        let id = protected::reference_create(self.lua.context())?;
        Ok(RegistryReference::new(self.lua.state(), self.lua.raw(), id))
    }

    /// Captures one function in the Lua registry.
    pub fn function(&mut self, index: i32) -> LuaResult<LuaFunction<'lua>> {
        let actual = self.value_type(index);
        if actual != LuaType::FUNCTION {
            return Err(LuaError::TypeMismatch {
                expected: LuaType::FUNCTION,
                actual,
            });
        }
        let reference = self.create_reference(index)?;
        Ok(LuaFunction::new(reference))
    }

    /// Registers or retrieves one named Rust userdata type.
    pub fn userdata_type<T: 'static>(&mut self, name: &str) -> LuaResult<UserDataType<'lua, T>> {
        UserDataType::register(self, name)
    }

    /// Performs raw table lookup using the key at stack top.
    ///
    /// This method validates table type and frame-local key ownership.
    pub fn raw_get(&mut self, table_index: i32) -> LuaResult<()> {
        self.expect_table(table_index)?;
        self.require_frame_values(1)?;
        protected::raw_get(self.lua.context(), table_index)
    }

    /// Performs raw table assignment using key and value at stack top.
    ///
    /// This method validates table type and frame-local operands.
    pub fn raw_set(&mut self, table_index: i32) -> LuaResult<()> {
        self.expect_table(table_index)?;
        self.require_frame_values(2)?;
        protected::raw_set(self.lua.context(), table_index)
    }

    /// Advances Lua table iteration using the key at stack top.
    ///
    /// Returns `true` after replacing the key with the next key/value pair, or
    /// `false` after consuming the final key.
    pub fn next(&mut self, table_index: i32) -> LuaResult<bool> {
        self.expect_table(table_index)?;
        self.require_frame_values(1)?;
        protected::next(self.lua.context(), table_index)
    }

    #[cfg(feature = "serde")]
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

    /// Consumes frame-owned values and pushes a C closure.
    pub(crate) fn push_c_closure(
        &mut self,
        function: LuaCFunction,
        upvalue_count: usize,
    ) -> LuaResult<()> {
        self.require_frame_values(upvalue_count)?;
        let upvalue_count = i32::try_from(upvalue_count).map_err(|_| LuaError::CountOverflow)?;
        protected::push_c_closure(self.lua.context(), function, upvalue_count)
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
            protected::pop(self.lua.context(), count)?;
        }
        Ok(())
    }

    pub(crate) fn rollback_on_error<T>(
        &mut self,
        entry_top: i32,
        result: LuaResult<T>,
    ) -> LuaResult<T> {
        if result.is_ok() {
            return result;
        }
        let current = self.top();
        let extra = current
            .checked_sub(entry_top)
            .ok_or(LuaError::StackUnderflow {
                baseline: entry_top,
                requested_top: current,
            })?;
        self.pop(extra as usize)?;
        result
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
            protected::pop(self.lua.context(), extra)?;
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

    pub(crate) const fn state(&self) -> std::ptr::NonNull<rsgdll_platform::__private::RawLuaState> {
        self.lua.state()
    }

    pub(crate) const fn context(&self) -> protected::Context {
        self.lua.context()
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
