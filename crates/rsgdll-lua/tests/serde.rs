#![cfg(feature = "serde")]

#[allow(dead_code)]
mod support;

use rsgdll_lua::{Lua, LuaError, serde as lua_serde};
use serde::{Deserialize, Serialize};
use support::{Fixture, LuaTestExt};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Config {
    name: String,
    enabled: bool,
    scores: Vec<u64>,
}

#[test]
fn serde_rejects_integer_that_lua_cannot_represent_exactly() {
    // Given: an integer one above Lua's exact f64 range.
    let value = 9_007_199_254_740_993_u64;
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();

    // When: serde serializes it to a Lua number.
    let result = lua_serde::to_lua(&mut frame, &value);

    // Then: conversion fails instead of silently rounding the integer.
    assert_eq!(result, Err(LuaError::IntegerOutOfRange));
}

#[test]
fn serde_rejects_large_sparse_numeric_keys() {
    // Given: a Lua table with a numeric key far beyond a reasonable sequence.
    let mut fixture = Fixture::new(vec![], vec![]);
    // SAFETY: fixture owns a live state and matching fake vtable.
    let mut lua = unsafe { Lua::from_raw(fixture.state()) }.expect("valid fixture");
    let mut stack = lua.stack();
    let mut frame = stack.frame();
    frame.create_table().expect("table");
    frame.push(1_000_000_000.0).expect("key");
    frame.push(true).expect("value");
    frame.raw_set(-3).expect("table assignment");

    // When: the sparse table is deserialized.
    let result = lua_serde::from_lua::<Vec<bool>>(&mut frame, -1);

    // Then: malformed input returns an error instead of allocating for the key.
    assert!(result.is_err());
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
    lua_serde::to_lua(&mut frame, &expected).expect("serialize to Lua");
    let actual: Config = lua_serde::from_lua(&mut frame, -1).expect("deserialize from Lua");

    // Then: serde-visible data is unchanged.
    assert_eq!(actual, expected);
}
