#![cfg(feature = "serde")]

#[allow(dead_code)]
mod support;

use rsgdll_lua::{Lua, serde as lua_serde};
use serde::{Deserialize, Serialize};
use support::Fixture;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Config {
    name: String,
    enabled: bool,
    scores: Vec<u64>,
}

#[test]
fn serde_struct_round_trips_through_a_lua_table() {
    // Given: one ordinary serde data structure.
    let expected = Config {
        name: "Ada".to_owned(),
        enabled: true,
        scores: vec![2, 7],
    };
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: it is serialized to Lua and deserialized back.
    // SAFETY: fake table/string operations cannot allocate, raise, or longjmp.
    unsafe { lua_serde::to_lua(&mut frame, &expected).expect("serialize to Lua") };
    // SAFETY: fake table iteration cannot raise or longjmp.
    let actual: Config =
        unsafe { lua_serde::from_lua(&mut frame, -1).expect("deserialize from Lua") };

    // Then: serde-visible data is unchanged.
    assert_eq!(actual, expected);
}
