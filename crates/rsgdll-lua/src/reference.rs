use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;

use rsgdll_platform::__private::{RawLuaBase, RawLuaState};

use crate::{LuaError, LuaResult, StackFrame, protected};

/// One registry-owned Lua value tied to its originating state lifetime.
///
/// The reference is neither `Send` nor `Sync`. Dropping it releases the
/// registry slot while the state lifetime proves that `ILuaBase` remains live.
pub struct RegistryReference<'lua> {
    state: NonNull<RawLuaState>,
    raw: NonNull<RawLuaBase>,
    id: i32,
    _state: PhantomData<&'lua mut RawLuaState>,
    _main_thread: PhantomData<Rc<()>>,
}

impl<'lua> RegistryReference<'lua> {
    pub(crate) const fn new(
        state: NonNull<RawLuaState>,
        raw: NonNull<RawLuaBase>,
        id: i32,
    ) -> Self {
        Self {
            state,
            raw,
            id,
            _state: PhantomData,
            _main_thread: PhantomData,
        }
    }

    /// Pushes the referenced value onto its originating Lua stack.
    pub fn push(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        if self.raw != frame.raw() || self.state != frame.state() {
            return Err(LuaError::WrongState);
        }
        protected::reference_push(protected::Context::new(self.state, self.raw), self.id)
    }
}

impl Drop for RegistryReference<'_> {
    fn drop(&mut self) {
        let _ = protected::reference_free(protected::Context::new(self.state, self.raw), self.id);
    }
}
