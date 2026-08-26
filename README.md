# rsgdll

Rust framework for Garry's Mod binary Lua modules.

Application crates depend on the facade only:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
rsgdll = "0.1"
```

Internal `rsgdll-*` crates are published implementation components when Cargo
requires them. Do not add them to application manifests.

## Basic module

```rust
use rsgdll::prelude::*;

#[rsgdll::module]
fn module(module: &mut ModuleBuilder) {
    module.function("hello", hello);
}

#[rsgdll::function]
fn hello(name: String) -> String {
    format!("Hello {name}")
}
```

## `Result<T, E>` and `thiserror`

Add `thiserror = "2"` for the application error derive:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
enum LookupError {
    #[error("user {0} does not exist")]
    Missing(u64),
}

#[rsgdll::function]
fn user_name(id: u64) -> Result<String, LookupError> {
    (id != 0)
        .then(|| format!("user-{id}"))
        .ok_or(LookupError::Missing(id))
}
```

Errors retain normal Rust `Display`/`source` behavior and become Lua errors
after Rust returns. Lua callers can use `pcall`.

## Lua callback

```rust
use rsgdll::prelude::*;

#[rsgdll::function]
fn call_greeting(frame: &mut StackFrame<'_, '_>) -> Result<String, LuaError> {
    // SAFETY: capture occurs inside the generated callback firewall; call()
    // enters Lua through PCall.
    let callback = unsafe { frame.function(1)? };
    let (greeting,) = unsafe { callback.call(frame, ("Ada",))? };
    Ok(greeting)
}
```

## Userdata

```rust
use rsgdll::prelude::*;

struct Counter(f64);

#[derive(Debug)]
struct UserdataError(String);

impl std::fmt::Display for UserdataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UserdataError {}

impl From<LuaError> for UserdataError {
    fn from(error: LuaError) -> Self {
        Self(error.to_string())
    }
}

#[rsgdll::function]
fn new_counter(frame: &mut StackFrame<'_, '_>) -> Result<LuaStackValues, UserdataError> {
    // SAFETY: generated callback firewall contains all Lua mutations.
    unsafe {
        let kind = frame.userdata_type::<Counter>("example.Counter")?;
        counter_add
            .install_method(frame, &kind, "add")
            .map_err(|error| UserdataError(error.to_string()))?;
        install_userdata_gc(frame, &kind)
            .map_err(|error| UserdataError(error.to_string()))?;
        kind.push(frame, Counter(frame.get(1)?))?;
    }
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn counter_add(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    // SAFETY: retrieves the metatable registered by new_counter.
    let kind = unsafe { frame.userdata_type::<Counter>("example.Counter")? };
    let amount = frame.get::<f64>(2)?;
    let mut counter = kind.borrow_mut(frame, 1)?;
    counter.0 += amount;
    Ok(counter.0)
}
```

## Async feature

```toml
[dependencies]
rsgdll = { version = "0.1", features = ["async"] }
```

```rust
use rsgdll::prelude::*;

async fn run_background(sender: CompletionSender<u64>) {
    let _ = rsgdll::async_runtime::complete(sender, async { 42 }).await;
}
```

`rsgdll` does not choose an executor. Background tasks return owned `Send`
values through a `completion_queue`; drain the queue from a callback with
`MainThread`.

## Engine feature

```toml
[dependencies]
rsgdll = { version = "0.1", features = ["engine"] }
```

```rust
use rsgdll::prelude::*;

#[rsgdll::function]
fn is_dedicated(main_thread: &mut MainThread) -> Result<bool, rsgdll::engine::EngineError> {
    let engine = rsgdll::engine::Engine::attach(main_thread)?;
    Ok(engine.server()?.is_dedicated_server())
}
```

## Stage a module

```text
cargo xtask stage server example x86_64-unknown-linux-gnu \
  target/release/librsgdll_example.so garrysmod/lua/bin
```

This stages `gmsv_example_linux64.dll`. Use `client` for `gmcl_*`.
Filename support does not imply runtime support; see
[`docs/targets.md`](docs/targets.md).
