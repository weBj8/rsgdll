use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;

use rsgdll_platform::__private::{RawLuaBase, RawLuaState};

use crate::{LuaError, LuaResult, StackFrame};

/// One registry-owned Lua value tied to its originating state lifetime.
///
/// The reference is neither `Send` nor `Sync`. Dropping it releases the
/// registry slot while the state lifetime proves that `ILuaBase` remains live.
pub struct RegistryReference<'lua> {
    raw: NonNull<RawLuaBase>,
    id: i32,
    _state: PhantomData<&'lua mut RawLuaState>,
    _main_thread: PhantomData<Rc<()>>,
}

impl<'lua> RegistryReference<'lua> {
    pub(crate) const fn new(raw: NonNull<RawLuaBase>, id: i32) -> Self {
        Self {
            raw,
            id,
            _state: PhantomData,
            _main_thread: PhantomData,
        }
    }

    /// Pushes the referenced value onto its originating Lua stack.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth cannot raise a Lua error or longjmp.
    pub unsafe fn push(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        if self.raw != frame.raw() {
            return Err(LuaError::WrongState);
        }
        // SAFETY: state identity matches and caller guarantees stack growth
        // cannot longjmp across this Rust frame.
        unsafe { RawLuaBase::reference_push(self.raw.as_ptr(), self.id) };
        Ok(())
    }
}

impl Drop for RegistryReference<'_> {
    fn drop(&mut self) {
        // SAFETY: upstream `ReferenceFree` only removes a raw registry entry
        // and does not invoke metamethods or allocate. The state lifetime
        // guarantees that this `ILuaBase` remains live until after this drop.
        unsafe { RawLuaBase::reference_free(self.raw.as_ptr(), self.id) };
    }
}
