use crate::{
    FromLua, FromLuaMulti, IntoLuaMulti, LuaBytes, LuaError, LuaResult, LuaType, RegistryReference,
    StackFrame, protected,
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
    pub fn push(&self, frame: &mut StackFrame<'_, 'lua>) -> LuaResult<()> {
        self.reference.push(frame)
    }

    /// Calls this function through Lua's protected-call boundary.
    ///
    /// Lua errors are copied into [`LuaError::Call`] and returned normally.
    ///
    pub fn call<A, R>(&self, frame: &mut StackFrame<'_, 'lua>, arguments: A) -> LuaResult<R>
    where
        A: IntoLuaMulti,
        R: FromLuaMulti,
    {
        let entry_top = frame.top();
        let argument_count =
            i32::try_from(arguments.count()).map_err(|_| LuaError::CountOverflow)?;
        let result_count = i32::try_from(R::COUNT).map_err(|_| LuaError::CountOverflow)?;
        let result = (|| {
            self.push(frame)?;
            arguments.push(frame)?;
            let actual_arguments = frame
                .top()
                .checked_sub(entry_top)
                .and_then(|count| count.checked_sub(1))
                .ok_or(LuaError::CountOverflow)?;
            if actual_arguments != argument_count {
                return Err(LuaError::ArgumentCountMismatch {
                    expected: argument_count,
                    actual: actual_arguments,
                });
            }
            let status = protected::pcall(frame.context(), argument_count, result_count)?;
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
        })();
        frame.rollback_on_error(entry_top, result)
    }
}
