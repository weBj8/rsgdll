//! Protected-op overhead benchmark (spec §15 gate). Run:
//! `cargo test --release -p rsgdll-core --test bench -- --ignored --nocapture`

mod common;

use std::time::Instant;

use rsgdll_core::protected::Op;
use rsgdll_core::{IntoLua, Stage};

const N: u32 = 200_000;

fn per_op(label: &str, f: impl Fn()) -> f64 {
    let t = Instant::now();
    (0..N).for_each(|_| f());
    let ns = t.elapsed().as_nanos() as f64 / f64::from(N);
    println!("BENCH {label}: {ns:.0} ns/op");
    ns
}

#[test]
#[ignore]
fn protected_op_overhead() {
    let (l, lua) = common::state();
    let a = common::api();
    let raw = per_op("raw pushnumber+settop", || unsafe {
        (a.lua_pushnumber)(l, 1.0);
        (a.lua_settop)(l, 0);
    });
    let one = per_op("protected single push", || {
        lua.run(&[], &[Op::PushNumber(1.0)]).unwrap();
        unsafe { (a.lua_settop)(l, 0) };
    });
    let batch = per_op("protected batch of 8 pushes (whole batch)", || {
        let mut s = Stage::new(&lua);
        (0..8).for_each(|i| f64::from(i).into_lua(&mut s).unwrap());
        s.finish().unwrap();
        unsafe { (a.lua_settop)(l, 0) };
    }) / 8.0;
    println!(
        "BENCH overhead single={:.1}x batched={:.1}x",
        one / raw,
        batch / raw
    );
}
