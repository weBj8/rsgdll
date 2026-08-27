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
    pub fn push(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        self.reference.push(frame)
    }

    /// Reads one raw table entry with checked Rust conversion.
    ///
    pub fn get<K, V>(&self, frame: &mut StackFrame<'_, 'lua>, key: K) -> LuaResult<V>
    where
        K: IntoLua,
        V: FromLua,
    {
        let entry_top = frame.top();
        let result = (|| {
            self.push(frame)?;
            frame.push(key)?;
            frame.raw_get(-2)?;
            let value = frame.get(-1);
            frame.pop(2)?;
            value
        })();
        frame.rollback_on_error(entry_top, result)
    }

    /// Writes one raw table entry.
    ///
    pub fn set<K, V>(&self, frame: &mut StackFrame<'_, 'lua>, key: K, value: V) -> LuaResult<()>
    where
        K: IntoLua,
        V: IntoLua,
    {
        let entry_top = frame.top();
        let result = (|| {
            self.push(frame)?;
            frame.push(key)?;
            frame.push(value)?;
            frame.raw_set(-3)?;
            frame.pop(1)
        })();
        frame.rollback_on_error(entry_top, result)
    }
}
