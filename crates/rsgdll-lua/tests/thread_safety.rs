use rsgdll_lua::Lua;
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(Lua<'static>: Send, Sync);
