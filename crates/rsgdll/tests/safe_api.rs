use rsgdll::lua::{LuaFunction, LuaResult, LuaTable, RegistryReference, StackFrame, UserDataType};
use rsgdll::module::{Function, IntoLuaReturn, ReturnError, ReturnWriter};

struct Counter;
pub struct CustomReturn(pub bool);

impl IntoLuaReturn for CustomReturn {
    fn into_lua_return(self, writer: &mut ReturnWriter<'_>) -> Result<(), ReturnError> {
        writer.push(self.0)
    }
}

#[allow(dead_code)]
fn public_callback_descriptors_are_nameable(_: Function) {}

#[allow(dead_code)]
fn ordinary_lua_operations_are_safe<'guard, 'lua>(
    frame: &mut StackFrame<'guard, 'lua>,
    table: &LuaTable<'lua>,
    function: &LuaFunction<'lua>,
    reference: &RegistryReference<'lua>,
    userdata: &UserDataType<'lua, Counter>,
) -> LuaResult<()> {
    frame.push(true)?;
    let _ = frame.new_table()?;
    let _ = frame.create_reference(1)?;
    let _ = frame.function(1)?;
    let _ = frame.userdata_type::<Counter>("Counter")?;
    reference.push(frame)?;
    table.push(frame)?;
    let _: bool = table.get(frame, "key")?;
    table.set(frame, "key", true)?;
    function.push(frame)?;
    let _: (bool,) = function.call(frame, ())?;
    userdata.push(frame, Counter)?;
    userdata.push_metatable(frame)?;
    Ok(())
}

#[cfg(feature = "serde")]
#[allow(dead_code)]
fn serde_operations_are_safe(frame: &mut StackFrame<'_, '_>) -> LuaResult<()> {
    rsgdll::lua::serde::to_lua(frame, &true)?;
    let _: bool = rsgdll::lua::serde::from_lua(frame, -1)?;
    Ok(())
}
