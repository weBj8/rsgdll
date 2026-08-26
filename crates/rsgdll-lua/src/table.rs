use crate::{FromLua, IntoLua, LuaResult, RegistryReference, StackFrame};

/// Registry-owned Lua table tied to one originating state.
pub struct LuaTable<'lua> {
    reference: RegistryReference<'lua>,
}

impl<'lua> LuaTable<'lua> {
    pub(crate) const fn new(reference: RegistryReference<'lua>) -> Self {
        Self { reference }
    }

    /// Pushes this table onto its originating Lua stack.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth cannot raise a Lua error or longjmp.
    pub unsafe fn push(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        // SAFETY: caller accepts the documented stack-growth obligation.
        unsafe { self.reference.push(frame) }
    }

    /// Reads one raw table entry with checked Rust conversion.
    ///
    /// # Safety
    ///
    /// Caller must ensure registry pushes, key conversion, and table access
    /// cannot raise a Lua error or longjmp.
    pub unsafe fn get<K, V>(&self, frame: &mut StackFrame<'_, 'lua>, key: K) -> LuaResult<V>
    where
        K: IntoLua,
        V: FromLua,
    {
        // SAFETY: caller accepts every allocating/stack-mutating operation.
        unsafe {
            self.push(frame)?;
            frame.push(key)?;
            frame.raw_get(-2)?;
        }
        let value = frame.get(-1);
        frame.pop(2)?;
        value
    }

    /// Writes one raw table entry.
    ///
    /// # Safety
    ///
    /// Caller must ensure registry pushes, conversions, and table access cannot
    /// raise a Lua error or longjmp.
    pub unsafe fn set<K, V>(
        &self,
        frame: &mut StackFrame<'_, 'lua>,
        key: K,
        value: V,
    ) -> LuaResult<()>
    where
        K: IntoLua,
        V: IntoLua,
    {
        // SAFETY: caller accepts every allocating/stack-mutating operation.
        unsafe {
            self.push(frame)?;
            frame.push(key)?;
            frame.push(value)?;
            frame.raw_set(-3)?;
        }
        frame.pop(1)
    }
}
