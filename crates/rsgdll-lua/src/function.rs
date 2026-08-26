use rsgdll_platform::__private::RawLuaBase;

use crate::{
    FromLua, FromLuaMulti, IntoLuaMulti, LuaBytes, LuaError, LuaResult, LuaType, RegistryReference,
    StackFrame,
};

/// Registry-owned Lua function tied to one originating state.
pub struct LuaFunction<'lua> {
    pub(crate) reference: RegistryReference<'lua>,
}

impl<'lua> LuaFunction<'lua> {
    pub(crate) const fn new(reference: RegistryReference<'lua>) -> Self {
        Self { reference }
    }

    /// Pushes this function onto its originating Lua stack.
    ///
    /// # Safety
    ///
    /// Caller must ensure stack growth cannot raise a Lua error or longjmp.
    pub unsafe fn push(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        // SAFETY: caller accepts the documented stack-growth obligation.
        unsafe { self.reference.push(frame) }
    }

    /// Calls this function through Lua's protected-call boundary.
    ///
    /// Lua errors are copied into [`LuaError::Call`] and returned normally.
    ///
    /// # Safety
    ///
    /// Caller must ensure registry and argument pushes cannot raise a Lua error
    /// or longjmp before protected execution begins.
    pub unsafe fn call<A, R>(&self, frame: &mut StackFrame<'_, 'lua>, arguments: A) -> LuaResult<R>
    where
        A: IntoLuaMulti,
        R: FromLuaMulti,
    {
        let argument_count =
            i32::try_from(arguments.count()).map_err(|_| LuaError::CountOverflow)?;
        let result_count = i32::try_from(R::COUNT).map_err(|_| LuaError::CountOverflow)?;
        // SAFETY: caller accepts registry and argument push obligations.
        unsafe {
            self.push(frame)?;
            arguments.push(frame)?;
        }
        // SAFETY: function and arguments are live at stack top. `PCall`
        // catches Lua errors internally and returns an integer status.
        let status =
            unsafe { RawLuaBase::pcall(frame.raw().as_ptr(), argument_count, result_count, 0) };
        if status != 0 {
            let message = if frame.value_type(-1) == LuaType::STRING {
                LuaBytes::from_lua(frame.lua(), -1)?
            } else {
                LuaBytes::from(
                    format!("Lua error value has type {}", frame.value_type(-1).0).into_bytes(),
                )
            };
            frame.pop(1)?;
            return Err(LuaError::Call { status, message });
        }
        let first = frame
            .top()
            .checked_sub(result_count)
            .and_then(|index| index.checked_add(1))
            .ok_or(LuaError::CountOverflow)?;
        let output = R::read(frame, first);
        frame.pop(R::COUNT)?;
        output
    }
}
