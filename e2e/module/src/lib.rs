use rsgdll::prelude::*;
use thiserror::Error;

#[rsgdll::module]
fn module(module: &mut ModuleBuilder) {
    module
        .function("plain", plain)
        .function("add", add)
        .function("primitives", primitives)
        .function("result_ok", result_ok)
        .function("result_err", result_err)
        .function("panic_now", panic_now);
}

#[rsgdll::function]
fn plain() -> &'static str {
    "plain Rust call"
}

#[rsgdll::function]
fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[rsgdll::function]
fn primitives() -> (&'static str, u64, bool) {
    ("converted", 7, true)
}

#[rsgdll::function]
fn result_ok() -> Result<&'static str, E2eError> {
    Ok("ok")
}

#[rsgdll::function]
fn result_err() -> Result<(), E2eError> {
    Err(E2eError { source: InnerError })
}

#[rsgdll::function]
fn panic_now() {
    panic!("intentional E2E panic");
}

#[derive(Debug, Error)]
#[error("outer E2E failure")]
struct E2eError {
    #[source]
    source: InnerError,
}

#[derive(Debug, Error)]
#[error("inner E2E cause")]
struct InnerError;
