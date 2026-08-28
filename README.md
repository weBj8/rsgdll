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

## Panic strategy

With `panic = "unwind"`, rsgdll catches callback panics, returns normally
through Rust, and lets the C++ firewall raise a Lua error. With
`panic = "abort"`, Rust terminates the process immediately; no `PanicReport`
or Lua error can be produced, and Lua `pcall` cannot catch the panic.

Potentially throwing Lua mutations run in a C++ executor closure that is
prepared before Rust is entered and invoked through `ILuaBase::PCall`.
Failures therefore return to Rust as `LuaError`; no Lua `longjmp` removes a
Rust frame.

The supported runtime is Garry's Mod's pinned default `ILuaBase`
implementation; replacement implementations may not throw C++ exceptions
through framework calls. Dynamic binary-module unload/reload is unsupported:
keep the module loaded until Lua-state/process teardown. See
[`docs/abi-reference.md`](docs/abi-reference.md) for the exact contract.

## Lua callback

```rust
use rsgdll::prelude::*;

#[rsgdll::function]
fn call_greeting(frame: &mut StackFrame<'_, '_>) -> Result<String, LuaError> {
    let callback = frame.function(1)?;
    let (greeting,) = callback.call(frame, ("Ada",))?;
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
    let kind = frame.userdata_type::<Counter>("example.Counter")?;
    counter_add
        .install_method(frame, &kind, "add")
        .map_err(|error| UserdataError(error.to_string()))?;
    kind.push(frame, Counter(frame.get(1)?))?;
    Ok(LuaStackValues::new(1))
}

#[rsgdll::function]
fn counter_add(frame: &mut StackFrame<'_, '_>) -> Result<f64, LuaError> {
    let kind = frame.userdata_type::<Counter>("example.Counter")?;
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

## Debug feature

```toml
[dependencies]
rsgdll = { version = "0.1", features = ["debug"] }
```

`StackFrame::install_debug_hook` installs a callback-scoped checked hook.
`DebugContext` walks frames and exposes locals/upvalues through ordinary
checked stack values; no raw `lua_State` or `lua_Debug` pointer is public.
Keep the returned `DebugHookGuard` and call `restore_with_frame` to restore the
previous hook, mask, and count. See
[`examples/debug-hook`](examples/debug-hook/src/lib.rs).

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
mkdir -p garrysmod/lua/bin
cp target/release/librsgdll_example.so \
  garrysmod/lua/bin/gmsv_example_linux64.dll
```

For a client module, change the `gmsv` prefix to `gmcl`.
Filename support does not imply runtime support; see
[`docs/targets.md`](docs/targets.md).
