# Codex Implementation Prompt — `rsgdll`

You are implementing a new Rust framework named **`rsgdll`** for developing Garry's Mod binary Lua modules.

The project replaces the role of `gmod-rs`; do **not** depend on `gmod-rs`.

The public developer experience must require only:

```toml
[dependencies]
rsgdll = "..."
```

Developers must not need to directly depend on internal `rsgdll-*` crates.

This is a low-level FFI/ABI project. Prioritize:

- sound Rust ownership and lifetime rules;
- isolation of unsafe code;
- precise ABI boundaries;
- no Lua `longjmp` crossing a Rust stack frame;
- minimal C++ code;
- a safe and ergonomic public Rust API;
- actual testing inside Garry's Mod through GLuaTest;
- useful diagnostics when Garry's Mod crashes.

---

# Execution protocol

Do **not** implement the entire project in one pass.

Implement exactly **one Part** at a time.

For each Part:

1. Inspect the existing repository first.
2. Preserve good existing work if it already exists.
3. Implement only the scope of the current Part.
4. Keep files small and responsibility-focused.
5. Avoid speculative abstractions needed only by future Parts.
6. Run all verification relevant to that Part.
7. Fix warnings and failures introduced by the Part.
8. Commit the Part as one or more logically separated commits.
9. Report:

   - files created/changed;
   - important API decisions;
   - verification performed;
   - known limitations intentionally left for later Parts.

10. **STOP. Do not begin the next Part.**

Wait until I explicitly say:

```text
Continue with Part N
```

before continuing.

If the repository already contains part of a later design, do not silently rewrite it. Explain any conflict before changing an established public interface.

---

# Global architectural constraints

The workspace should converge toward:

```text
rsgdll/
├── Cargo.toml
├── crates/
│   ├── rsgdll/                 # only normal public dependency
│   ├── rsgdll-abi/             # raw handwritten GMod/Lua ABI
│   ├── rsgdll-platform/        # OS/architecture-specific behavior
│   ├── rsgdll-lua/             # safe Lua abstraction
│   ├── rsgdll-module/          # module lifecycle + dispatch
│   ├── rsgdll-runtime/         # runtime/main-thread services
│   ├── rsgdll-macros/          # proc macros
│   ├── rsgdll-bridge/          # bounded C++ longjmp boundary
│   │
│   ├── rsgdll-async/           # optional
│   ├── rsgdll-engine-sys/      # optional raw Source ABI
│   ├── rsgdll-engine/          # optional safe engine wrapper
│   ├── rsgdll-sigscan/         # optional
│   └── rsgdll-detour/          # optional
│
├── e2e/
│   ├── module/
│   ├── addon/
│   └── runner/
│
├── examples/
├── docs/
└── xtask/
```

Do not create empty crates simply to match this diagram. Create a crate when its implementation Part begins.

Dependencies must point primarily downward:

```text
                    rsgdll
                      │
          ┌───────────┼───────────┐
          ▼           ▼           ▼
       module        lua       runtime
          │           │           │
          └───────────┼───────────┘
                      ▼
                   platform
                      │
                      ▼
                     abi
```

Optional engine stack:

```text
rsgdll
   │
   ▼
rsgdll-engine
   │
   ▼
rsgdll-engine-sys
```

Optional hooking stack:

```text
rsgdll
   │
   ├── rsgdll-sigscan
   │
   └── rsgdll-detour
```

`rsgdll-abi` must never depend upward on safe framework crates.

## Part 1 crate boundaries

The Part 1 workspace contains only the public facade and the six internal
crates needed to establish its core dependency direction:

```text
rsgdll
├── rsgdll-lua ─────┐
├── rsgdll-module ──┼──> rsgdll-platform ──> rsgdll-abi
├── rsgdll-runtime ─┘
└── rsgdll-macros
```

- `rsgdll` is the only normal application-facing dependency. It owns the
  `lua`, `module`, `runtime`, `prelude`, and macro-plumbing namespaces.
- `rsgdll-abi` is the dependency-light raw ABI leaf and has no dependencies
  on safe framework crates.
- ABI offsets remain ordinary Rust constants. Module initialization passes one
  immutable, module-local layout pointer to the private C++ bridge; consumer
  binaries export only the two GMod entrypoints, never ABI data symbols.
- `rsgdll-platform` isolates target-specific behavior above the raw ABI.
- `rsgdll-lua`, `rsgdll-module`, and `rsgdll-runtime` are separate safe
  subsystem boundaries above the platform layer.
- `rsgdll-macros` contains developer-facing proc macros. Generated code must
  address framework internals through `rsgdll::__private`.

Optional engine, signature scanning, detouring, hooking, async, serde, and
diagnostic implementations are not Part 1 workspace crates. Their public
feature names are reserved on `rsgdll`; `rsgdll::raw` is likewise reserved
behind the `raw` feature and does not re-export `rsgdll-abi` in Part 1.

---

# Public Cargo feature model

The public `rsgdll` crate must use:

```toml
[features]
default = []
```

Core Lua/module functionality is always present and therefore is **not** represented by a default feature.

Design toward these public capabilities:

```text
debug
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
debug
    enables checked Lua debug hooks, frames, locals, and upvalues

engine
    enables Source engine integration

sigscan
    enables signature scanning only

detour
    depends on sigscan and enables detouring

hook
    convenience alias for sigscan + detour

async
    enables asynchronous/background runtime integration

serde
    enables serde-based Lua conversion helpers

backtrace
    enables enhanced Rust diagnostic/backtrace support

raw
    reserves the future low-level ABI escape-hatch namespace;
    version 0.1 exposes no raw ABI items

full
    enables normal optional capabilities
    but does NOT implicitly enable raw
```

## Checked Lua debugger boundary

The default-disabled `debug` feature stays inside existing crates:

```text
rsgdll facade
    -> rsgdll-lua checked callback-scoped views
    -> rsgdll-bridge protected hook callback context
    -> pinned LuaJIT debug C API
```

No debugger crate or raw-state escape hatch is added. `RawLuaDebug` and exact
event/mask constants live in `rsgdll-abi`; ordinary users receive
`DebugContext`, `DebugFrame`, `DebugLocal`, and `DebugUpvalue`.

The bridge installs one native hook trampoline. It prepares the same protected
operation context used by normal callbacks before entering Rust, so checked
value pushes remain protected and debugger-triggered Lua execution cannot
re-enter the hook. The Rust dispatcher catches panics before returning to C++.

`DebugHookGuard` captures the prior hook, mask, and count. Restoration is
explicit and requires the originating checked Lua callback frame. `Drop`
cannot safely call a VM owned and destroyed by Garry's Mod, so discarding an
active guard intentionally does not dereference its stored state identity.
Hook frames and local/upvalue guards remain callback-lifetime-bound; owned
`DebugFrameInfo` is copied before that lifetime ends.

Do not represent platforms as Cargo features.

Use:

```rust
#[cfg(target_os = "...")]
#[cfg(target_arch = "...")]
```

for platform selection.

---

# Safety invariants

These are non-negotiable.

## 1. Lua state is main-thread-only

The safe Lua handle must be equivalent conceptually to:

```text
Lua<'lua>
    !Send
    !Sync
```

Do not expose a convenient global Lua state getter as the primary API.

Pass Lua/main-thread capabilities explicitly.

---

## 2. No Lua exception may cross a Rust frame

This is the most important invariant in the framework:

```text
NO Lua longjmp may unwind/remove a Rust stack frame.
```

Do not directly call Lua/GMod APIs such as an unprotected:

```text
ThrowError
ArgError
CheckType
CheckString
CheckNumber
Call
```

from safe Rust paths if they can raise a Lua error through `longjmp`.

Argument/type validation in Rust must instead return ordinary Rust errors.

Calls into arbitrary Lua must use protected execution where available.

Potentially throwing stack, allocation, table, registry, call, and userdata
operations use a prepared executor:

```text
C++ trampoline
  -> reserve stack capacity and registry-store executor closure
  -> enter Rust
Rust
  -> fill one POD operation descriptor
  -> call C++ bridge
C++ bridge
  -> copy existing operands using reserved non-allocating stack slots
  -> invoke executor closure with ILuaBase::PCall
  -> return status and results to Rust
```

The executor closure is created before Rust is entered. A Lua error raised by
an allocating operation therefore lands in `PCall` inside C++ and becomes a
normal status before C++ returns to Rust. Callback re-entry while an operation
is active is rejected on the C++ side, so a second mutable Lua capability
cannot be created while the original Rust frame is live. One operation accepts
at most 64 copied arguments or results so its setup remains inside the stack
capacity reserved before Rust entry.

Each loaded module also records active Rust callback execution under the same
private Lua-registry key. A trampoline checks that key through its prepared
`PCall` executor before entering Rust, rejects cross-module re-entry in C++,
and clears only the marker it acquired after Rust returns. This registry state
is shared by separate `cdylib` copies where module-local thread storage is not.
GMod exposes `debug.getregistry()` as a metatable proxy rather than this real
`SPECIAL_REG` table, so ordinary Lua cannot clear the marker; hostile native
modules are outside this guard's threat model.

Safe extension traits do not supply trusted stack facts. Before a protected
call, the framework compares the reported argument count with the actual
frame-owned stack delta. After dispatch, C++ accepts stack-return mode only
when the return count equals the observed stack delta. Cleanup operations
remain available above the normal operation ceiling so failed calls can
restore the stack and clear the re-entry marker.

---

## 3. Rust errors remain normal Rust errors

Application developers should be able to write:

```rust
#[derive(Debug, thiserror::Error)]
enum UserError {
    #[error("user {0} does not exist")]
    NotFound(u64),

    #[error("storage operation failed")]
    Storage(#[source] std::io::Error),
}
```

and:

```rust
#[rsgdll::function]
fn get_user(id: u64) -> Result<User, UserError> {
    ...
}
```

Do not require application code to manually construct Lua error objects.

Do not require:

```lua
local value, err = module.get_user(...)
```

as the normal API style.

Lua should behave normally:

```lua
local value = module.get_user(...)
```

and when callers intentionally want to catch an error:

```lua
local ok, result = pcall(module.get_user, ...)
```

---

# Rust → Lua error architecture

Implement the error flow using a bounded, auditable C++ boundary.

The required call flow is:

```text
Lua
 │
 ▼
generic C++ callback trampoline
 │
 │ normal C ABI call
 ▼
Rust dispatcher
 │
 ├── decode arguments
 ├── invoke user Rust function
 ├── Result<T, E>
 ├── format error if needed
 ├── drop all Rust-owned values
 └── RETURN normally
 │
 ▼
generic C++ callback trampoline
 │
 ├── success → return number of Lua values
 │
 └── error   → ThrowError(...)
                   │
                   ▼
                Lua error
```

The `ThrowError` occurs only after Rust has completely returned.

The handwritten C++ bridge must stay within 600 pure lines. `build.rs`
enforces this budget on non-blank, non-`//` lines in `firewall.cpp`.
Shared POD layouts and numeric constants in `firewall_abi.h`, including
`ModuleRegistration`, are generated from the `#[repr(C)]` definitions in
`rsgdll-bridge`. C++ function-pointer aliases remain maintained by the header
generator. Generated declarations do not count toward the handwritten budget.

The protected executor necessarily performs stack choreography and every
potentially throwing Lua mutation on the C++ side. A lower line target must
not move those calls into Rust frames merely to reduce C++ size.

Do not move Lua abstractions, conversion logic, userdata management, runtime management, or application behavior into C++.

---

# C++ bridge constraints

Use a dedicated internal crate:

```text
rsgdll-bridge
```

It compiles one bounded handwritten C++ source with the `cc` build dependency.

Keep C++ limited to approximately:

- receiving the Lua callback;
- calling a Rust dispatcher function pointer;
- preparing and invoking the generic protected-operation executor;
- interpreting one fixed-layout Lua operation descriptor;
- examining a POD dispatch result;
- raising the Lua error after Rust returns;
- publishing the module global table after the Rust initializer returns.

Avoid:

- STL containers;
- exceptions;
- dynamic allocation;
- RAII objects with non-trivial destructors;
- application logic.

The bridge itself is intentionally on the `longjmp` side, so use simple POD/local C-style data.

Prefer a function-pointer registration model so multiple independently loaded `rsgdll` modules cannot accidentally resolve each other's dispatcher symbol.

Conceptual architecture:

```text
gmod13_open tail-jumps to C++
        │
        ├── C++ allocates fixed ModuleRegistration storage
        ├── Rust initializer performs non-Lua registration
        ├── Rust catches panics and returns POD metadata
        └── after every Rust frame returns, C++ publishes the global table
            with potentially longjmping Lua operations
        │
Lua callback
        ▼
rsgdll_bridge_trampoline(lua_State*)
        │
        ▼
stored Rust dispatcher
```

The entrypoint tail jump leaves no Rust entrypoint frame beneath C++. C++ does
not perform Lua operations while the Rust initializer is active. Keeping module
publication on the longjmp side is therefore part of the firewall contract, not
application logic.

The supported foreign runtime is the pinned default Garry's Mod `ILuaBase`
implementation. Direct exact-type reads must return normally; replacement
implementations that throw C++ exceptions are unsupported, and no C++
exception may cross the bridge C ABI or a Rust frame.

Version 0.1 does not support dynamic binary-module unload or reload. The host
may invoke `gmod13_close` only during Lua-state/process teardown after native
callbacks can no longer execute. By default the close entrypoint performs no
cleanup. A module may opt into `#[rsgdll::module(close = on_close)]`, where
`on_close` must be a safe `fn()` and cannot access Lua. Its panic is contained
at the FFI boundary when `panic = "unwind"`. This teardown hook does not make dynamic unload safe:
Lua-retained closures or userdata finalizers would still hold stale
shared-object function pointers.

The generic C++ callback should be reusable for every exported Rust function.

Function identity can be stored in the Lua closure/upvalue and decoded by the Rust dispatcher.

Do not generate a separate C++ function for every exported Rust function.

---

# Dispatch ABI

Design a small C-compatible result structure.

Conceptually:

```text
DispatchResult
    status
    return_count
    error_length
```

Statuses should distinguish at minimum:

```text
SUCCESS
RUST_ERROR
RUST_PANIC
INTERNAL_ERROR
```

Use explicit integer representation across FFI.

The C++ trampoline supplies a fixed-capacity error buffer to Rust.

A reasonable initial capacity is:

```text
32 KiB
```

Rust may allocate normally while formatting the error internally, but:

1. format the complete Rust report;
2. copy it into the supplied bridge buffer;
3. drop the Rust `String` and all other owned Rust values;
4. return to C++;
5. only then may C++ raise the Lua error.

No Rust allocation may depend on control returning after `ThrowError`.

Handle truncation deterministically and append a clear truncation marker.

---

# Error formatting

Create an internal Rust representation similar to:

```text
ErrorReport
    module/function context
    Display message
    Error::source() chain
    optional captured backtrace
    error category
```

For:

```rust
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("failed to load user {id}")]
    UserLoad {
        id: u64,
        #[source]
        source: std::io::Error,
    },
}
```

produce a useful message such as:

```text
[rsgdll] mymodule.get_user failed: failed to load user 42

Caused by:
  1: permission denied
```

Use:

```rust
std::error::Error::source()
```

to walk the error chain.

Do not require `thiserror`; support anything implementing the standard Rust error traits required by the callback API.

`thiserror` is simply expected to work naturally.

---

# Optional Rust backtraces

Normal `Result<T, E>` errors do not inherently preserve the original Rust call stack.

Therefore:

```text
backtrace
```

must be optional.

Without it, provide:

- exported function context;
- `Display`;
- error source chain;
- Lua's own call-site error behavior.

With `backtrace`, support an `rsgdll` diagnostic error/report type capable of recording a Rust backtrace.

Do not make backtrace capture mandatory for ordinary errors.

---

# Panic behavior

Never allow a Rust panic to unwind into C++ or Garry's Mod.

At the Rust dispatcher boundary:

```text
catch panic
    ↓
format PanicReport
    ↓
return normally to C++
    ↓
C++ raises Lua error
```

Distinguish panic output from normal `Result::Err`.

For example:

```text
[rsgdll panic] mymodule.process_user

index out of bounds: ...
```

If the final binary uses `panic = "abort"`, Rust terminates the process
immediately. Graceful panic conversion and Lua-visible `PanicReport`
diagnostics are available only with `panic = "unwind"`.

---

# Part 1 — Workspace and public facade

Implement only the repository/workspace foundation.

Create the minimum crates required to establish:

```text
rsgdll
rsgdll-abi
rsgdll-platform
rsgdll-lua
rsgdll-module
rsgdll-runtime
rsgdll-macros
```

Do not implement engine/hook/async crates yet.

The public crate must be:

```text
crates/rsgdll
```

and developer-facing imports must eventually look like:

```rust
use rsgdll::prelude::*;
```

Establish:

```text
rsgdll::lua
rsgdll::module
rsgdll::runtime
rsgdll::prelude
```

and a hidden:

```text
rsgdll::__private
```

namespace for proc-macro generated code.

Do not publicly dump all `rsgdll-abi` items from the facade.

Reserve:

```text
rsgdll::raw
```

for the later `raw` feature.

Implement the top-level feature definitions now, even when a future feature does not yet have an implementation crate. Do not add fake empty dependency crates solely to satisfy the feature.

Document the crate/dependency boundaries in:

```text
docs/architecture.md
```

### Part 1 acceptance

The workspace builds.

An empty consumer crate can add only:

```toml
rsgdll = { path = "..." }
```

and import:

```rust
use rsgdll::prelude::*;
```

No dependency on `gmod-rs` exists.

**STOP after Part 1.**

---

# Part 2 — Raw GMod/Lua ABI and platform layer

The implemented raw layout, exact pinned revisions, and target verification
status are recorded in [`docs/abi-reference.md`](abi-reference.md).

Implement `rsgdll-abi` as the handwritten raw ABI description.

Use current authoritative Garry's Mod/community headers as references.

Do not guess:

- `lua_State` layout;
- `ILuaBase` layout;
- virtual function ordering;
- calling conventions;
- architecture-specific offsets.

Document the exact upstream header/commit/version used as ABI reference.

`rsgdll-abi` should contain only low-level things such as:

```text
RawLuaState
RawLuaBase
RawUserData
LuaCFunction
LuaType
SpecialIndex
typed wrappers for required virtual slots
```

Keep safe ownership semantics out of this crate.

Avoid a giant generic "call arbitrary vtable index" API in public safe code.

Prefer explicit typed raw functions for ABI operations used by higher layers.

Add:

```text
#![deny(unsafe_op_in_unsafe_fn)]
```

where practical.

`rsgdll-platform` should contain architecture/platform-specific constants and behavior without creating Cargo features for OS/architecture.

Initial runtime target may focus on Linux x86_64, but structure the definitions so unsupported targets fail clearly rather than silently using incorrect layouts.

Do not claim a target is supported until its ABI is actually verified.

### Part 2 acceptance

There is a clearly isolated unsafe ABI layer.

Higher-level crates do not directly encode vtable offsets.

ABI source/reference versions are documented.

**STOP after Part 2.**

---

# Part 3 — Safe Lua core

Implement the first useful `rsgdll-lua` layer.

Primary type:

```text
Lua<'lua>
```

Requirements:

```text
!Send
!Sync
lifetime-bound
no global-state-oriented public API
```

Introduce a stack abstraction and a stack-frame guard.

Safe operations in this Part should include enough for:

- stack top inspection;
- primitive type inspection;
- booleans;
- numbers;
- strings;
- nil;
- push/pop;
- tables needed for module registration;
- C closure registration;
- closure upvalue access.

Do not use throwing `Check*` APIs for conversion.

Implement Rust-side validation:

```text
expected String
actual Entity
        ↓
Rust conversion error
```

rather than Lua `ArgError` inside Rust.

Introduce basic traits/types such as:

```text
FromLua
IntoLua
LuaError
LuaResult
StackFrame
```

Keep the initial trait surface minimal.

Any operation that may raise a Lua exception must either:

- be excluded from safe API;
- use protected execution;
- or remain explicitly unsafe/raw.

### Part 3 acceptance

A safe Rust wrapper can manipulate primitive Lua values without calling Lua error/longjmp APIs.

Safe Lua handles cannot move to another thread.

Stack balancing can be checked/restored reliably.

**STOP after Part 3.**

---

# Part 4 — C++ error firewall and Rust dispatcher

Now create:

```text
rsgdll-bridge
```

and implement the longjmp firewall described above.

The C++ bridge must remain within the measured handwritten budget and the
responsibilities above.

Integrate it with `rsgdll-module`.

Implement:

```text
generic C++ trampoline
dispatcher function-pointer registration
DispatchResult
bridge-provided error buffer
Rust dispatcher
ErrorReport
PanicReport
```

The dispatcher must:

1. determine which Rust function is being invoked;
2. construct the safe Lua callback context;
3. invoke the registered Rust callback;
4. convert success values;
5. convert Rust errors into `ErrorReport`;
6. catch supported Rust panics;
7. restore/validate Lua stack state on failure;
8. fully destroy Rust locals;
9. return POD status to C++.

Only C++ may perform the final throwing Lua error.

Audit `rsgdll-lua` at this point for any accidental throwing paths.

### Part 4 acceptance

An artificial Rust `Err` can travel:

```text
Rust Result::Err
→ Rust ErrorReport
→ Rust returns
→ C++ ThrowError
→ Lua error
```

without a Lua longjmp crossing Rust frames.

A deliberately triggered Rust panic is also converted at the boundary when panic unwinding is available.

**STOP after Part 4.**

---

# Part 5 — Module API, registration and proc macros

Implement the first developer-facing module experience.

Target syntax:

```rust
use rsgdll::prelude::*;

#[rsgdll::module]
fn module(module: &mut ModuleBuilder) {
module
.function("hello", hello)
.function("get_user", get_user);
}

#[rsgdll::function]
fn hello(name: String) -> String {
    format!("Hello {name}")
}

#[rsgdll::function]
fn get_user(id: u64) -> Result<User, UserError> {
    ...
}
```

Garry's Mod's `require` does not return a binary module table. The generated
entrypoint therefore publishes the table under the consumer crate name with
Rust identifier spelling (`my-module` becomes `_G.my_module`) and returns zero
Lua values, matching the official Facepunch module-base convention.

The macros must generate Rust glue, not application behavior.

Generated code should reference:

```text
rsgdll::__private
```

rather than requiring users to list internal crates in `Cargo.toml`.

Support:

- plain return values;
- `()`;
- appropriate tuples/multiple Lua return values when implemented;
- standard `Result<T, E>` where `E` is compatible with the framework error requirements.

Avoid clever trait-specialization designs that depend on unstable Rust.

If distinguishing a plain return from syntactic `Result<...>` is simplest and robust enough for the initial API, implement it explicitly and document any type-alias limitation.

The module DLL must expose the GMod-required module entrypoints.

Keep module initialization itself non-throwing from Rust unless a separate sound boundary is designed for initialization errors.

### Part 5 acceptance

A separate example `cdylib` can depend only on `rsgdll` and export callable Lua functions.

`Result::Err` becomes a normal Lua error.

The example crate does not depend on internal `rsgdll-*` crates.

**STOP after Part 5.**

---

# Part 6 — First real Garry's Mod E2E environment

Integrate:

```text
https://github.com/CFC-Servers/GLuaTest
```

Do not mock Garry's Mod for this layer.

Create a real consumer module under:

```text
e2e/module/
```

It must depend only on the public facade:

```toml
rsgdll = { path = "../../crates/rsgdll" }
```

and build a real Garry's Mod binary module.

Stage it using the proper GMod filename, for example on Linux x86_64:

```text
garrysmod/lua/bin/gmsv_rsgdll_e2e_linux64.dll
```

Create GLuaTest Lua tests under:

```text
e2e/addon/
```

The first E2E suite should verify at least:

```text
require() successfully loads the DLL
plain Rust function call
primitive argument conversion
primitive return conversion
Result::Ok
Result::Err
Lua pcall catching the Rust-originated error
thiserror Display message appears
source-chain information appears when applicable
panic boundary behavior
server remains alive after recoverable errors
```

Use the GLuaTest Docker environment but own enough of the runner/workflow to collect process crash information.

Do not depend exclusively on the reusable GLuaTest GitHub workflow if it prevents crash artifacts from being preserved.

### Core dump/crash capture

For Linux E2E:

- enable runtime core dumps with an actual runtime `RLIMIT_CORE`;
- configure a useful `core_pattern`;
- prevent automatic server restart;
- preserve GMod/GLuaTest logs;
- preserve `debug.log` when produced;
- preserve the core file when produced;
- keep the exact test DLL;
- preserve enough build/debug symbols for useful analysis.

On crash, automatically attempt:

```text
gdb
thread apply all bt full
```

and write:

```text
backtrace.txt
```

Treat these as **CI artifacts**, not build caches.

Artifact directory should conceptually contain:

```text
e2e-artifacts/
├── gluatest.log
├── console.log
├── debug.log
├── core
├── backtrace.txt
├── tested-module.dll
└── metadata.txt
```

`metadata.txt` should include at minimum:

```text
git commit
GMod branch
target architecture
enabled rsgdll features
Rust toolchain information
```

The collector must run even when the server exits non-zero.

Distinguish useful outcomes where practical:

```text
PASS
TEST_FAILURE
MODULE_LOAD_FAILURE
SERVER_CRASH
TIMEOUT
```

### Part 6 acceptance

CI can build a real `rsgdll` consumer DLL, inject it into a real GMod test server, call it from GLuaTest, and receive a meaningful failure when either Lua assertions fail or the native server crashes.

Native crashes produce diagnostic artifacts whenever the host/container permits core generation.

**STOP after Part 6.**

---

# Part 7 — Complete the Lua developer surface

Expand `rsgdll-lua` only after the minimal real E2E path works.

Add well-bounded support for:

```text
tables
functions
protected Rust → Lua calls
multiple return values
registry references
userdata
metatables
userdata methods
garbage collection
binary-safe strings
optional serde integration
```

Registry/reference types must encode lifetime/ownership rules clearly.

Do not make arbitrary Lua references `Send` unless there is a deliberately designed main-thread dispatcher abstraction that makes such use sound.

Rust → Lua function invocation must use protected calling behavior so a Lua error becomes a Rust `Result` rather than jumping through Rust frames.

Extend the E2E module and GLuaTest coverage for each major capability.

### Part 7 acceptance

The core Lua feature set is sufficient for realistic GMod binary modules without requiring normal users to access `rsgdll::raw`.

**STOP after Part 7.**

---

# Part 8 — Runtime and optional async/serde/backtrace capabilities

Implement the safe runtime model.

Introduce an explicit main-thread capability, conceptually:

```text
MainThread
```

and a dispatcher/completion queue for background work.

The invariant is:

```text
background worker
    │
    │ Send-safe Rust values only
    ▼
completion queue
    │
    ▼
GMod main thread
    │
    ▼
Lua access
```

Never send `Lua<'_>` or raw Lua references into an async worker.

Add `rsgdll-async` behind the public:

```text
async
```

feature.

Keep the core runtime independent from a specific async executor where practical. Do not support multiple executors speculatively unless doing so materially simplifies the design.

Implement optional:

```text
serde
backtrace
```

feature behavior.

Backtrace integration must enhance `ErrorReport`/`PanicReport` without changing ordinary Lua success syntax.

Extend E2E tests for async completion if async support is enabled.

### Implemented runtime model

`rsgdll-runtime` provides a non-`Send`, non-`Sync` `MainThread` capability
minted only by generated callback glue. Background producers receive a
bounded `CompletionSender<T>`; moving that sender to a worker requires
`T: Send`. The paired `CompletionQueue<T>` can run completion handlers only
when supplied with `&mut MainThread`, so Lua access remains in the callback
that drains the queue.

`rsgdll-async` is enabled only by the facade's `async` feature. Its
`complete` adapter accepts any `Send` future and forwards only the future's
owned `Send` output into the completion queue. It does not own or select an
executor.

The `serde` feature forwards to `rsgdll-lua`'s checked Lua conversion layer.
The `backtrace` feature forwards to `rsgdll-module` and adds captured Rust
backtraces to ordinary error and panic reports. All three features remain
disabled by default, and `full` still excludes `raw`.

### Part 8 acceptance

Background work can safely complete back onto the GMod main thread without transferring Lua state across threads.

Optional capabilities are disabled by default.

**STOP after Part 8.**

---

# Part 9 — Optional Source engine and hooking subsystems

Only now create:

```text
rsgdll-engine-sys
rsgdll-engine
rsgdll-sigscan
rsgdll-detour
```

The engine raw ABI must be separate from the Lua ABI.

Architecture:

```text
rsgdll-engine
       │
       ▼
rsgdll-engine-sys
       │
       ▼
Source engine ABI
```

`rsgdll-engine-sys` may contain:

- raw Source interface layouts;
- CreateInterface-related ABI;
- raw vtables;
- platform-specific library names.

`rsgdll-engine` provides checked higher-level wrappers.

Signature scanning and detouring must remain separate because their safety characteristics are materially worse than ordinary engine interfaces.

Public features:

```text
engine
sigscan
detour
hook
```

with:

```text
detour → sigscan
hook   → sigscan + detour
```

Do not expose hook functionality through ordinary core prelude imports unless explicitly enabled.

Extend E2E coverage where a stable real-engine assertion can be made.

### Part 9 acceptance

A developer who needs only Lua/module functionality does not compile or link engine/hooking dependencies.

**STOP after Part 9.**

---

# Part 10 — Packaging, target matrix and public developer experience

Finalize the framework as a consumable crate family while retaining `rsgdll` as the only normal dependency.

Ensure all internal crates can be packaged in a way compatible with publishing the facade.

Review public API visibility carefully.

Normal documentation should present only:

```text
rsgdll
```

as the dependency developers add.

Internal crates may be published as implementation components if required by Cargo packaging, but must not become the recommended developer interface.

Add `xtask` support for GMod module artifact naming/staging.

Target commands should eventually support outputs similar to:

```text
gmsv_name_linux.dll
gmsv_name_linux64.dll
gmsv_name_win32.dll
gmsv_name_win64.dll

gmcl_name_linux.dll
gmcl_name_linux64.dll
gmcl_name_win32.dll
gmcl_name_win64.dll
```

Do not claim runtime support for an architecture merely because it cross-compiles.

Document the difference between:

```text
build-supported
ABI-verified
E2E-verified
```

targets.

Add CI checks appropriate for the targets actually supported.

Keep Linux x86_64 real-GMod E2E as the baseline native runtime gate if that is the environment currently available.

Provide concise examples covering:

```text
basic module
Result<T, E> + thiserror
Lua callback
userdata
async feature
engine feature
```

### Final architecture review

Before declaring the framework complete, verify these invariants:

```text
1. Normal developers depend only on rsgdll.

2. default features are empty.

3. Core Lua/module functionality works with default features.

4. rsgdll-abi contains the raw GMod/Lua ABI boundary.

5. Unsafe ABI details do not leak through ordinary public APIs.

6. Lua handles are not Send/Sync.

7. No Lua longjmp crosses a Rust frame.

8. Result<T, E> remains ordinary Rust Result<T, E>.

9. thiserror errors format naturally through Display/source.

10. Handwritten C++ stays within its enforced budget and contains only the
    exception/longjmp firewall responsibilities listed above.

11. Rust panics never unwind into GMod/C++ when graceful handling
    is available.

12. Optional engine/hook/async functionality remains disabled unless
    explicitly requested.

13. GLuaTest loads an actual compiled rsgdll consumer module.

14. Recoverable Rust errors do not crash the server.

15. Native crashes preserve useful diagnostics/core/backtrace when
    the platform permits it.

16. No dependency on gmod-rs remains.
```

Only after all of these are satisfied should the project be considered ready for a first public framework release.
