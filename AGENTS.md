# AGENTS.md — rsgdll Repository Instructions

## Project

`rsgdll` is a Rust framework for developing Garry's Mod binary Lua modules.

It replaces the role of `gmod-rs`. Do not introduce a dependency on `gmod-rs`.

The intended developer experience is:

```toml
[dependencies]
rsgdll = "..."
```

Normal users depend only on the public `rsgdll` facade crate.

Internal `rsgdll-*` crates are implementation components and must not become required direct dependencies for normal consumers.

For the architectural overview and crate responsibilities, read:

```text
docs/architecture.md
```

before making architectural changes.

---

## Primary design goals

Prefer, in order:

1. Soundness across Rust, C/C++, Lua, and Source ABI boundaries.
2. Small and auditable unsafe surfaces.
3. Stable, ergonomic public Rust APIs.
4. Explicit ownership and lifetime semantics.
5. Isolation between framework subsystems.
6. Useful diagnostics for native crashes and Lua-visible failures.
7. Minimal C++ code.
8. Minimal dependencies and compile-time cost where practical.

Do not sacrifice soundness for API convenience.

---

## Repository architecture

The repository is a Cargo workspace.

The intended crate families are:

```text
rsgdll
    Public facade.

rsgdll-abi
    Raw handwritten Garry's Mod / Lua ABI definitions.

rsgdll-platform
    OS and architecture-specific behavior.

rsgdll-lua
    Safe or checked Lua abstractions.

rsgdll-module
    Module lifecycle, callback dispatch and registration.

rsgdll-runtime
    Main-thread/runtime services.

rsgdll-macros
    Developer-facing proc macros.

rsgdll-bridge
    Minimal C++ longjmp/error firewall.

rsgdll-async
    Optional asynchronous integration.

rsgdll-engine-sys
    Optional raw Source engine ABI.

rsgdll-engine
    Optional higher-level Source engine API.

rsgdll-sigscan
    Optional signature scanning.

rsgdll-detour
    Optional function detouring.
```

Do not create an empty crate merely because it appears in this architecture.

Introduce a crate when its boundary has an actual implementation need.

---

## Dependency direction

Dependencies should flow downward.

The normal core direction is:

```text
rsgdll
   |
   +--> rsgdll-module
   +--> rsgdll-lua
   +--> rsgdll-runtime
             |
             v
      rsgdll-platform
             |
             v
        rsgdll-abi
```

Do not introduce upward dependencies from low-level crates.

In particular:

```text
rsgdll-abi
```

must not depend on:

```text
rsgdll-lua
rsgdll-module
rsgdll-runtime
rsgdll
```

Raw Source engine ABI belongs in `rsgdll-engine-sys`, not `rsgdll-abi`.

Signature scanning and detouring do not belong in the core Lua abstraction.

---

## Public API boundary

Only `rsgdll` is the normal application-facing crate.

Public examples and documentation should use:

```rust
use rsgdll::prelude::*;
```

where appropriate.

Proc-macro generated code should resolve framework internals through:

```text
rsgdll::__private
```

rather than requiring application developers to add internal crates.

Do not broadly re-export raw ABI internals.

Low-level escape hatches belong behind:

```text
rsgdll::raw
```

and the `raw` Cargo feature.

Treat `rsgdll::__private` as macro/framework plumbing, not a user-facing API.

---

## Cargo features

The public facade uses:

```toml
[features]
default = []
```

Core module/Lua support is always available and is not represented as a default feature.

Optional public capabilities are expected to include:

```text
engine
sigscan
detour
hook
async
serde
backtrace
raw
full
```

Rules:

```text
detour -> sigscan
hook   -> sigscan + detour
```

`full` must not implicitly enable `raw`.

Do not create Cargo features for operating systems or CPU architectures.

Use Rust target configuration:

```rust
#[cfg(target_os = "...")]
#[cfg(target_arch = "...")]
```

for target-specific behavior.

---

## Unsafe code rules

Unsafe code must be concentrated at real ABI/FFI boundaries.

Expected unsafe-heavy areas include:

```text
rsgdll-abi
rsgdll-engine-sys
rsgdll-bridge integration
explicit low-level hooking code
```

Higher-level crates should minimize unsafe code.

Where practical use:

```rust
#![deny(unsafe_op_in_unsafe_fn)]
```

Every non-trivial unsafe block must have a nearby `SAFETY:` explanation stating the invariant that makes it valid.

Do not use `unsafe` merely to bypass Rust ownership or thread-safety restrictions.

Do not manually implement `Send` or `Sync` for Lua-related types unless the safety argument is explicit and independently defensible.

---

## Lua thread-safety

Safe Lua state/context objects are main-thread-bound.

Conceptually:

```text
Lua<'lua>
    !Send
    !Sync
```

Do not make the primary public API depend on a globally retrievable Lua state.

Pass Lua/main-thread capabilities explicitly through call boundaries.

Lua references must not be transferred to worker threads unless a future type specifically represents a thread-safe operation without transferring Lua state ownership.

---

## Critical longjmp invariant

The following invariant is non-negotiable:

> A Lua error/longjmp must never remove a Rust stack frame.

Garry's Mod/Lua functions capable of raising Lua errors must not be called from arbitrary safe Rust frames in an unprotected way.

Examples that require special care include operations equivalent to:

```text
ThrowError
ArgError
CheckType
CheckString
CheckNumber
unprotected Call
lua_error
```

Do not use throwing `Check*` functions as the normal Rust conversion mechanism.

Instead:

```text
inspect Lua value
    ->
validate in Rust
    ->
return Result::Err
```

Calls into arbitrary Lua code should use a protected call mechanism where possible.

---

## Rust-to-Lua error model

Normal framework/application code uses ordinary Rust:

```rust
Result<T, E>
```

Errors are not represented by normal Lua return tuples such as:

```lua
local value, err = module.call()
```

The desired Lua behavior is:

```lua
local value = module.call()
```

and callers that intentionally handle failure use normal Lua mechanisms:

```lua
local ok, result = pcall(module.call)
```

Application error types should work naturally with:

```text
std::error::Error
Display
Error::source()
thiserror
```

Do not force users to convert application errors into a framework-specific Lua error type merely to expose a function.

---

## C++ error firewall

The project intentionally contains a very small C++ bridge.

Its primary purpose is to ensure this sequence:

```text
Lua
 ->
C++ trampoline
 ->
Rust dispatcher
 ->
Rust Result<T, E>
 ->
all Rust frames return normally
 ->
C++ receives POD result
 ->
C++ raises Lua error
```

The C++ layer must remain small.

Do not move normal Lua abstractions, conversions, module logic, runtime logic, engine APIs, or user code into C++.

Prefer:

```text
C-style POD
function pointers
fixed-size buffers
simple integer status codes
```

inside this boundary.

Avoid STL containers, C++ exceptions, and unnecessary dynamic ownership in the bridge.

---

## Error reporting

Rust errors should preserve useful Rust formatting.

A normal error report should contain, when available:

```text
exported module/function context
Display message
Error::source() chain
optional Rust backtrace
```

`thiserror` must work naturally through its normal `Display` and `source` implementations.

Do not use debug formatting as the default user-facing error representation when `Display` is available.

Backtrace capture is optional and belongs behind the `backtrace` capability where appropriate.

Do not pretend a normal `Result<T, E>` automatically contains the original Rust stack trace.

---

## Panic boundary

Rust panics must not unwind into Garry's Mod or C++.

Where panic unwinding is available, callback dispatch should catch the panic at the Rust FFI boundary, convert it to a diagnostic report, completely return from Rust, and allow the C++ boundary to raise the corresponding Lua-visible failure.

A panic and an ordinary `Result::Err` are different failure categories and should remain distinguishable in diagnostics.

Do not claim recoverable panic handling when compiling with an aborting panic strategy.

---

## FFI data ownership

Do not expose Rust-owned allocations across an FFI boundary unless ownership and destruction are explicit.

In particular, error reporting must not rely on C++ returning normally after it raises a Lua error.

Preferred error flow:

```text
C++ provides writable error buffer
 ->
Rust formats error
 ->
Rust copies bytes into buffer
 ->
Rust destroys temporary allocations
 ->
Rust returns
 ->
C++ raises Lua error using copied message
```

Any FFI structure must use stable C-compatible representation where required.

Use explicit-width integer types for externally shared status/length fields.

---

## ABI implementation

`rsgdll-abi` is an ABI description, not a convenience SDK.

Do not guess ABI details.

When modifying:

```text
lua_State layouts
ILuaBase layouts
virtual function ordering
calling conventions
platform-specific offsets
```

identify the authoritative upstream/reference definition being followed.

Document ABI source versions or commits where appropriate.

Do not claim runtime support for a platform merely because the code compiles for it.

Distinguish:

```text
build-supported
ABI-verified
E2E-verified
```

targets.

---

## Macros

Proc macros should remove repetitive glue, not hide framework architecture.

Generated code must route through stable framework/internal interfaces rather than emitting raw ABI/vtable manipulation.

Prefer normal typed Rust developer APIs.

Avoid dependence on unstable Rust language features unless explicitly approved.

Macro diagnostics should point developers toward their source code whenever practical.

---

## File organization

Keep files focused on one responsibility.

Avoid generic dumping-ground modules such as:

```text
utils.rs
helpers.rs
common.rs
misc.rs
```

unless the contained functionality genuinely forms a coherent abstraction.

Prefer naming files after the concept they implement.

Do not split code merely to reduce line count.

Split when components have distinct ownership, safety, dependency, or behavioral boundaries.

---

## Optional subsystems

Optional engine, hooking, async, serde, and diagnostic capabilities must not silently enter the core dependency graph.

A default `rsgdll` consumer should not compile Source-engine, detour, async-runtime, or other optional implementation dependencies it does not use.

Avoid speculative abstraction for hypothetical future executors, engines, or targets.

Implement abstractions when there is a concrete supported use case.

---

## End-to-end testing

Real Garry's Mod behavior is validated using GLuaTest.

The E2E consumer module must use `rsgdll` as an external developer would:

```toml
[dependencies]
rsgdll = { path = "../../crates/rsgdll" }
```

Do not make E2E consumer code depend directly on internal crates to simplify testing.

The E2E environment should exercise an actual GMod binary module loaded through the normal module loader.

Native test failures should distinguish where practical:

```text
TEST_FAILURE
MODULE_LOAD_FAILURE
SERVER_CRASH
TIMEOUT
```

For Linux native crashes, preserve useful diagnostics where the environment permits:

```text
console/server logs
debug.log
core dump
gdb backtrace
exact tested binary
build/runtime metadata
```

Treat core dumps and backtraces as diagnostic artifacts.

---

## Compatibility and dependencies

Do not add `gmod-rs`.

Avoid adding dependencies for functionality available clearly and safely in the standard library or existing workspace dependencies.

Before adding a non-trivial dependency, check:

```text
what capability it provides
whether it affects default users
whether it belongs behind a feature
whether its license is compatible
whether the dependency is maintained
```

Keep low-level ABI crates particularly dependency-light.

---

## Documentation

`docs/architecture.md` is the detailed source of truth for framework architecture.

Keep this `AGENTS.md` concise enough to remain useful as persistent agent context.

When a design becomes too detailed for this file, document it under `docs/` and link to it here instead of continuously expanding `AGENTS.md`.

When changing an established architectural invariant, update the corresponding documentation in the same change.

---

## Working on implementation Parts

When a task refers to a numbered implementation Part:

1. Read `docs/architecture.md`.
2. Inspect the current repository state.
3. Implement only the requested Part.
4. Do not pre-implement later Parts.
5. Preserve established public interfaces unless the current task explicitly changes them.
6. Run the checks relevant to the files and functionality changed.
7. Fix failures or warnings caused by the change.
8. Report architectural deviations or unresolved platform limitations explicitly.
9. Stop at the Part boundary when instructed.

Do not hide incomplete or unsupported behavior behind silent fallbacks.

Prefer an explicit unsupported/error state over behavior based on guessed ABI assumptions.
