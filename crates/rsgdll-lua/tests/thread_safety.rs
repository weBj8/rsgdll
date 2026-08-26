use rsgdll_lua::{Lua, LuaFunction, LuaTable, RegistryReference, UserDataType};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(Lua<'static>: Send, Sync);
assert_not_impl_any!(RegistryReference<'static>: Send, Sync);
assert_not_impl_any!(LuaTable<'static>: Send, Sync);
assert_not_impl_any!(LuaFunction<'static>: Send, Sync);
assert_not_impl_any!(UserDataType<'static, ()>: Send, Sync);
