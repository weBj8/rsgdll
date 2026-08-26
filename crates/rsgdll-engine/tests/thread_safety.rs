use rsgdll_engine::{Engine, EngineServer};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(Engine<'static>: Send, Sync);
assert_not_impl_any!(EngineServer<'static>: Send, Sync);
