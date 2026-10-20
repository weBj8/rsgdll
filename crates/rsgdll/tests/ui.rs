//! Compile-time checks (spec §13.2 Lints/Exports/lua!). Needs a realm feature:
//! `cargo test -p rsgdll --features server --test ui`.

#[cfg(feature = "server")]
#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
