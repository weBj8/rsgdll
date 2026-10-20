# rsgdll: Garry's Mod binary module framework for Rust

Status: draft spec (research phase, nothing implemented).

## 1. Goals

- A developer writes ordinary Rust: plain functions, plain types, `Result<T, E>`, `?`.
  They should not need to know the Lua C API, the stack, or how GMod loads modules.
- All Lua ↔ Rust conversion is automatic (traits + derive macros, optional serde).
- Rust errors and panics become real Lua errors (`error()` semantics), with the Rust
  backtrace in the message when capture is enabled (§7.1). No `ok, err` boolean returns.
- Attribute macros export functions into `_G` (or nested tables like `mymod.sub.fn`).
- A `lua!` macro runs inline Lua that captures Rust variables and returns typed values,
  checked at compile time against the GMod API from
  [glua-api-snippets](https://github.com/luttje/glua-api-snippets).

Non-goals: a full Lua type checker; wrapping Source engine interfaces beyond what Lua
access needs.

Required: the GMod C++ interfaces `ILuaBase`, `ILuaInterface` (via `ILuaShared`) and
`CreateInterface` are bound in Rust (§5.2) and exposed safely. The plain Lua C API is used
for stack work; the vtables are used for GMod-specific features (Vector/Angle push/get,
threaded calls, convars, realm info).

Developer contract: user code contains no `extern`, no `unsafe`, no `#[no_mangle]`.
Macros generate every FFI entry point. The only `unsafe` a user ever writes is opt-in raw
access (`lua.raw()` returning `*mut lua_State` / `&ILuaBase`) and declaring extra engine
interfaces with `#[rsgdll::interface] unsafe trait` (§5.2), because the slot layout is an
unchecked claim. Goal (unverified): user crates may use `#![forbid(unsafe_code)]`, with macro-generated `unsafe`
and `#[unsafe(no_mangle)]` items emitted with `Span::mixed_site()` (or inside items
defined in rsgdll) so the lint does not fire on generated code. Whether rustc skips the lint
for such proc-macro output is not documented; a `trybuild` test gates it (§15). If the lint
fires, this guarantee is dropped and user crates document `#![allow(unsafe_code)]` only where
macros expand.

Language policy: Rust edition 2024 (MSRV 1.85). Everything is pure Rust. No C/C++ source,
no `cc` build step, no C headers or bindgen: the Lua C API is called through Rust
`extern "C-unwind"` fn pointers resolved at runtime, and protected calls are Rust
`extern "C-unwind" fn` callbacks passed to `lua_pcall`/`lua_cpcall`. C is added only if
a target proves impossible without it (none known). C++ vtables are called from Rust
with `#[repr(C)]` vtable structs (`extern "thiscall-unwind"` on Windows x86, `extern "C-unwind"`
elsewhere; variadics `extern "C"`, §5.2), so no C++ shim is needed either.

## 2. Decisions (confirmed with user)

| Topic | Decision |
|---|---|
| Targets | Linux x86-64, Windows x86-64, Linux x86, Windows x86 (all four in v1) |
| Bindings | Own raw FFI to `lua_shared` + `ILuaBase`/`ILuaInterface`/`CreateInterface` (no mlua, no gmod-rs) |
| `lua!` checking | Syntax + known globals/library members + arity, from glua-api-snippets |
| Data conversion | Own `#[derive(LuaValue)]`, plus optional `serde` feature |
| Scope | Full v1 (everything in this spec) in one milestone; risks gated by tests (§15) |
| Vtables | Full `ILuaBase`/`ILuaInterface` binding kept and used by the safe layer |

## 3. Background facts (researched)

- GMod loads `garrysmod/lua/bin/gm{sv,cl}_<name>_<platform>.dll` on `require("<name>")`.
  Platform suffixes: `win32`, `win64`, `linux`, `linux64`. Extension is always `.dll`.
- Entry points: `gmod13_open(L: *mut lua_State) -> c_int` and `gmod13_close`. Both run on
  the main thread of that realm's Lua state. rsgdll emits them as `extern "C-unwind"` with the
  same trampoline shape as exports (§7.3), because a failing pre/postopen raises a Lua error
  out of `gmod13_open`.
- Lua is LuaJIT 2.1.0-beta3 inside `lua_shared` (verified on the local server:
  `/mnt/ssd/gmod_server/bin/linux64/lua_shared.so` exports 137 `lua_*/luaL_*/luaJIT_*`
  symbols, including `luaL_traceback`, plus `CreateInterface`). Only linux64 is verified.
  The 32-bit `main` branch is believed to ship LuaJIT 2.0.4 (unconfirmed), which differs in
  API surface and unwinding. `cargo xtask probe-luajit` records the exact version, the
  exported symbol set and the unwinding mode for each of the four targets; the bound symbol
  set (§5.1) is the intersection, and the §7.3 unwinding assumptions are only relied on for
  targets the probe has confirmed.
- `lua_shared` path differs per platform/branch (`bin/lua_shared.so`,
  `bin/linux64/lua_shared.so`, `garrysmod/bin/lua_shared(_srv).so`, `bin/win64/lua_shared.dll`).
  Symbols are resolved at runtime with `libloading` against the already-loaded library
  (`dlopen(RTLD_NOLOAD)` / `GetModuleHandle`), never linked at build time.
- GLua extends Lua syntax: `!=`, `!`, `&&`, `||`, `//` and `/* */` comments, `continue`.
- GMod type ids extend Lua's (`TYPE_ENTITY = 9`, `TYPE_VECTOR = 10`, `TYPE_ANGLE = 11`, ...);
  userdata types are detected with `TypeID(v)`/metatable name, not `lua_type`.
- glua-api-snippets branch `lua-language-server-addon` holds `library/*.lua` (271 files)
  as LuaLS annotations: `---@param name type`, `---@return type`, `---@type`,
  `function lib.Fn(a, b) end`, `function Class:Method() end`, and a realm badge
  `(Server)` / `(Client)` / `(Shared)` / `(Menu)` in each doc comment.
- Error unwinding (expected, unverified except linux64; confirmed per target by
  `probe-luajit`): LuaJIT x64 (Linux + Windows) and Windows x86 throw errors as
  native exceptions (DWARF / SEH) through C frames; Linux x86 LuaJIT uses its own
  `longjmp`-style unwinding. Either way **no Rust frame with a pending `Drop` may be live
  when `lua_error` is called**, and every frame crossed must use a `-unwind` ABI. The only
  Rust frame crossed is the trampoline, which owns nothing (§7.3).

## 4. Workspace layout

```
crates/
  rsgdll/            facade, re-exports everything users need (`use rsgdll::prelude::*`)
  rsgdll-sys/        raw FFI: lua_State, constants, fn-pointer table loaded from lua_shared
  rsgdll-core/       safe layer: Lua handle, stack guard, conversion traits, errors, refs
  rsgdll-macros/     proc macros: #[module], #[function], #[derive(LuaValue)], lua!
  rsgdll-luacheck/   GLua parser front-end + API database + checker (used by macros)
  rsgdll-apidb/      generated, vendored API database (pinned snippets commit)
  rsgdll-gmod/       safe wrappers for GMod libraries: hook, timer, concommand, net helpers
xtask/               gen-apidb, build (all targets, correct file names), install, test
examples/
  minimal/           §13.1
  showcase/          §13.2, doubles as the feature test suite
tests/               integration tests run in a real srcds (see §12)
```

One concern per crate/folder. `rsgdll-luacheck` has no dependency on FFI code so it can
run inside the proc macro.

## 5. Raw layer (`rsgdll-sys`)

### 5.1 Lua C API

- `struct LuaShared { lua_pushnumber: unsafe extern "C-unwind" fn(...), ... }` loaded once
  per DLL into a `OnceLock` (§6.1a). Missing symbol = module load fails with a clear message.
- All fn pointer types are `extern "C-unwind"` so a Lua error passing through is not UB.
- Only the C API subset needed by `rsgdll-core` is bound; `luaL_traceback`,
  `lua_pcall`, `luaL_ref/unref`, `lua_newuserdata`, `luaL_newmetatable`,
  `luaL_checkudata`, `luaL_loadbuffer`, table and stack functions. Raising helpers such as
  `luaL_checkudata` are bound for raw users only; the safe layer never calls them.
- 32-bit Windows: functions are `cdecl`, same as 64-bit for the C API. Verify per target.
- Stack space: a C function only has `LUA_MINSTACK` (20) guaranteed slots. Every push path
  that can exceed that (multi-returns, `Variadic`, batched table builds, `lua!` captures)
  first calls `lua_checkstack` for the exact count. It may reallocate (and raise), so it runs
  inside the same protected callback as the pushes (§7.3).
- Stack reserve: starting a protected op needs free slots itself (the registry-fetched
  callback + its light-userdata arg). Invariant enforced by `StackGuard`: outside protected
  ops, safe code never uses more than `LUA_MINSTACK - 2` slots above the entry top, so a
  protected op can always be started without an unprotected `lua_checkstack`.

### 5.2 GMod C++ interfaces

Source of truth: [danielga/garrysmod_common](https://github.com/danielga/garrysmod_common)
(`include/GarrysMod/Lua/*.h`, maintained; checked at `f77a18d` 2026-06-04) and
[danielga/sourcesdk-minimal](https://github.com/danielga/sourcesdk-minimal) (`public/tier1/interface.h`,
`tier0/dbg.h`, engine interface headers). These are community reverse-engineered headers that
real modules have used for years, so `rsgdll-sys` transcribes them 1:1 into Rust instead of
doing new reverse engineering. A `cargo xtask gen-vtables` script parses the pinned headers
(simple `virtual ... = 0;` lines, in order) and emits the Rust vtable structs, so updates are a
re-run plus diff review.

Classes bound (all `#[repr(C)]` struct with `vtable: *const VTable` first field):

| Class | Header | Notes |
|---|---|---|
| `lua_State` (GMod) | `Interface.h` | header `48+22` bytes (32-bit) / `92+22` (64-bit), then `ILuaBase* luabase`; the field offset (after alignment padding) is pinned per target by a `const` assert against garrysmod_common's struct |
| `ILuaBase` | `LuaBase.h` | Top/Push/Pop/GetField/SetField/Call/PCall/ReferenceCreate/Push/Free, PushUserType/GetUserType, PushSpecial, CheckType, ... |
| `ILuaInterface : ILuaBase` | `LuaInterface.h` | 76 more virtuals after the ILuaBase slots: Global, IsServer/IsClient/IsMenu, RunString(Ex), FindAndRunScript, ErrorNoHalt/Msg/MsgColour (`Error(const char*)` is a padding slot: it raises), GetPathID, Require, AddThreadedCall, CreateConVar/CreateConCommand, PushPooledString, ... |
| `CLuaInterface` fields | `LuaInterface.h` | data layout after vtable: `m_iRealm`, `m_pGameCallback`, `m_sPathID[32]`, `m_pMetaTables[255]`; contains `std::list`, so only read via computed offsets, never constructed |
| `ILuaShared` | `LuaShared.h` | `CreateInterface("LUASHARED003")` from `lua_shared`; `GetLuaInterface(realm)` 0=client 1=server 2=menu, `GetStackTraces`, `ScriptExists`, `LoadFile` |
| `ILuaGameCallback` | `LuaGameCallback.h` | read-only use (ErrorPrint, Msg); not implemented by Rust (its `CLuaError` uses `std::string`/`std::vector`) |
| `ILuaObject` | `LuaObject.h` | bound, but the safe API prefers stack ops; exposed as raw only |
| `ILuaThreadedCall` | `LuaInterface.h` | Rust implements this vtable (`Init`, `Run(ILuaBase*)`); used by §10 to run work on the main thread via `AddThreadedCall` instead of a Think hook where available. Thread safety of `AddThreadedCall` is **unverified** (§10.2) |
| `CreateInterfaceFn` | sourcesdk `interface.h` | `void* (*)(const char*, int*)`, return code `IFACE_OK/IFACE_FAILED` |
| tier0 `Msg/Warning/ConColorMsg`, `Plat_FloatTime` | sourcesdk `tier0` | C exports of `tier0`, used for logging before Lua is ready |

Module discovery (ported from `garrysmod_common/helpers/source/ModuleLoader.cpp`): the
already-loaded library is located with `GetModuleHandleExA` / `dlopen(RTLD_NOLOAD)`, trying in
order `bin/win64/%s.dll`, `garrysmod/bin/%s.dll`, `bin/%s.dll` (Windows) and
`bin/linux64/lib%s_client.so`, `bin/linux64/%s_client.so`, `bin/linux64/lib%s.so`,
`bin/linux64/%s.so`, `bin/linux32/lib%s.so`, `bin/linux32/%s.so`, `garrysmod/bin/%s_srv.so`,
`garrysmod/bin/%s.so` (Linux). The engine factory (`"engine"`, `"server"`, `"client"`,
`"filesystem_stdio"`) uses the same loader; `rsgdll::interfaces::create_interface::<T>(module, "VEngineServer023")`
returns `Option<&'static T>`. Engine interface bindings beyond Lua are out of scope for v1
except `ICvar` (`VEngineCvar004`) and `IFileSystem` (`VFileSystem022`) which the helpers need.

ABI rules the generator must apply (headers target both MSVC and GCC):
- Calling convention: `extern "thiscall-unwind"` on `i686-pc-windows-msvc`; `extern "C-unwind"`
  (`this` as first arg) on every other target. Many methods can raise a Lua error (`Call`,
  `GetField`, `SetField`, `CheckType`, `ThrowError`, and any allocating method on OOM), so the
  `-unwind` ABI is mandatory and the safe layer only calls raising/allocating methods from
  inside the fixed protected callbacks of §7.3, same as the C API. `ILuaInterface::Error(const char*)`
  raises a Lua error and has no use outside the trampoline, so it is not bound (padding slot); Lua errors are raised only
  by the trampoline's `lua_error`. Variadic members (`Msg`, `ErrorNoHalt`, `MsgColour`; `ErrorFromLua` is a padding slot) are called only
  with `"%s"` + one pre-formatted C string. MSVC compiles variadic member functions as
  `__cdecl` with `this` on the stack (never `thiscall`), so the generator emits them as
  `extern "C" fn(this, fmt, ...)` on every target; they cannot raise, so no `-unwind` is needed.
- Virtual destructors: Itanium (Linux) emits 2 slots (complete + deleting), MSVC emits 1.
  `ILuaShared` starts with one, so its slot indexes differ per OS.
- Overloads: MSVC groups overloaded virtuals by name and puts them in reverse declaration
  order; GCC keeps declaration order. `ILuaBase`/`ILuaInterface` have overloads
  (`SetMember` x5, `GetMetaTableObject` x2 in `ILuaInterface`; `SetMember` x2 in `ILuaObject`), so the generator emits a
  separate slot order for Windows. Verified per target by the showcase interface tests.
- Never pass/return C++ objects by value or `std::string`/`std::list` across the boundary;
  those methods (`GetCurrentFile`, `CompileString`, `FindScripts`, `LoadFile`) are left as
  padding slots.

Safe API (in `rsgdll-core`):
- `lua.base()` -> `&LuaBase`, `lua.interface()` -> `&LuaInterface`: `is_server()`,
  `is_client()`, `is_menu()`, `realm()`, `path_id()`, `run_string(name, path, code) -> Result<()>`,
  `find_and_run_script(..)`, `msg(..)`, `error_no_halt(..)`, `require(name)`, `push_pooled_string(..)`.
- `rsgdll::interfaces::lua_shared()` -> `&LuaShared`: `lua_interface(Realm) -> Option<RealmInterface>`
  (cross-realm access, e.g. menu -> client). `RealmInterface` is `!Send`, not `'static`: it
  re-fetches `GetLuaInterface(realm)` on every use and returns `Err(StateClosed)` when the
  pointer changed or became null (client state destroyed on disconnect), `stack_traces()`, `script_exists(..)`.
- `rsgdll::interfaces::create_interface` for engine interfaces, with
  `#[rsgdll::interface("VEngineServer023")] unsafe trait EngineServer { fn ... }` so users can declare
  extra vtable layouts (the macro emits the glue; the slot list is the user's unchecked claim,
  hence `unsafe trait`).
- Uses inside the framework: realm detection, `PushVector`/`PushAngle`/`GetVector`/`GetAngle`
  for Vector/Angle (GMod allocates and copies; only `const Vector&` = pointer to 3 floats crosses),
  `AddThreadedCall` for §10, `ErrorNoHalt` for async errors, `CreateConVar/CreateConCommand`
  for §10.3.

## 6. Core runtime (`rsgdll-core`)

### 6.1 Handle

```rust
pub struct Lua { state: NonNull<lua_State>, _not_send: PhantomData<*mut ()> }
```

- `!Send + !Sync`. Users get `&Lua` (or nothing, if their function does not ask for it).
- Realm is known at compile time from the crate feature/cfg (`server`/`client`/`menu`),
  and also at runtime (`lua.realm()`).
- Every safe operation uses a `StackGuard` that restores `lua_gettop` on drop, so user
  code can never leak stack slots. Trampolines restore to `entry_top` before pushing returns
  (the guard is dropped first), so return values are never popped by it.

### 6.1a Multiple states per process

One DLL can be opened by several Lua states in one process (e.g. a `gmcl_` module required
by both the client and menu state), so `gmod13_open`/`gmod13_close` may run several times.

- Per DLL (`OnceLock`; every rsgdll `cdylib` has its own statics, `std` and `linkme` slices,
  so two rsgdll modules never share these): the resolved `lua_shared` fn table and the panic hook.
- Per state, in a main-thread map keyed by the state's **main thread** (not the incoming
  `lua_State*`: exports called from a coroutine receive the coroutine's pointer, and GMod's
  `luabase` field is only valid on the main thread). Lua 5.1 has no `LUA_RIDX_MAINTHREAD`, so the key is
  `lua_topointer(L, LUA_REGISTRYINDEX)`: the registry table is shared by every coroutine of a
  state, so this needs no allocation and no stored value. Address reuse by a later state is
  handled by the generation counter. The main `lua_State*` itself is captured at open and kept
  in the map entry (no integer registry slot, which would collide with `luaL_ref` keys).
  `gmod13_open` checks it really runs on the main thread (`lua_pushthread` returns 1, then
  pop) and refuses to load otherwise (`require` from inside a coroutine). `Lua` holds the current `L` for stack work only; refs,
  `LocalLua` and `LuaHandle` never store a coroutine pointer. Map contents: runtime, generation
  counter, executor, registry refs, chunk cache, tracked hooks/timers, `LuaHandle` channel.
- `ILuaBase` acts on the main thread's stack. When an export runs with `L != mainthread`
  (called from a coroutine), every `ILuaBase` push (`PushVector`, `PushAngle`,
  `PushPooledString`) targets the main stack and the result is moved to `L` with
  `lua_xmove(main, L, n)` inside the same protected op, which first calls
  `lua_checkstack(main, n)` (the §5.1 reserve only covers `L`, and the main thread is
  suspended inside `lua_resume` with unknown free space); for `GetVector`/`GetAngle` the
  checked value is copied (`lua_pushvalue`, so the arg slot stays) and `lua_xmove`d to the
  main stack, read, then popped. Like all argument conversion (§6.2) this runs protected. A showcase test
  calls every Vector/Angle export from inside a coroutine.
- Reentrancy: Rust -> Lua -> Rust export -> Lua is normal. Runtime state uses `Cell`/short
  `RefCell` borrows that are never held across a Lua call (enforced by the API: no guard is
  returned to user code). A panic is caught by the trampoline and the module keeps working;
  there is no poisoning.
- Several rsgdll modules may share one state; every registry key, chunk cache entry,
  framework hook/timer id and metatable name is prefixed `rsgdll.<module>.`, and each module
  carries its own runtime.

### 6.2 Values

| Lua | Rust (FromLua / IntoLua) |
|---|---|
| nil | `()`, `Option::None`, `Nil` |
| boolean | `bool` |
| number | `f64`, `f32` (`f64 -> f32` rounds, allowed to lose precision), all ints (checked both ways: Lua -> int errors on non-integral or out of range; i64/u64/i128 -> Lua errors when `|v| > 2^53`, never silently lossy) |
| string (64-bit id) | `SteamId64` (string-backed, e.g. `"76561198000000000"`), and `#[lua(as_string)]` on any `i64`/`u64` field or arg |
| string | `String`, `&str` (borrowed from the stack slot; its lifetime is tied to the arg/`StackGuard` borrow, so it cannot outlive the slot), `Cow<str>`: numbers are coerced like `luaL_checkstring` (allocates, so it runs protected); invalid UTF-8 is a conversion error (`expected UTF-8 string`), never lossy. Binary data (net, files) uses `LuaBytes` / `&[u8]` |
| table (array) | `Vec<T>`, `[T; N]`, `VecDeque<T>`: reads `1..=#t`; a `nil` hole inside that range is a conversion error (`expected sequence, hole at 3`); non-sequence keys are ignored |
| table (map) | `HashMap<K, V>`, `BTreeMap<K, V>`, `IndexMap` (feature) |
| table (struct) | `#[derive(LuaValue)]` struct / enum |
| function | `LuaFunction` (registry ref), Rust closures via `lua.create_function(..)` |
| Vector / Angle / Color | `Vector { x, y, z }`, `Angle { p, y, r }`, `Color { r, g, b, a }` |
| Entity / Player | args (exports and hooks): `EntityRef<'_>`, `PlayerRef<'_>` (borrow the stack slot, no registry ref); owned `Entity`, `Player` (registry ref + `IsValid` check on use) via `.to_owned()` only when stored. Owned types do not implement `FromLua` for args. A NULL entity converts to `Option::None`; a non-`Option` ref arg receiving NULL is a `bad argument` error |
| any | `LuaValue` enum, `LuaTable`, `LuaAny` (registry ref) |
| userdata of a Rust type | `#[derive(LuaUserData)]` / `#[rsgdll::methods]` impl |

- Traits: `FromLua::from_lua(lua, idx) -> Result<Self, ConvError>` and
  `IntoLua::into_lua(self, lua) -> Result<(), ConvError>` are both fallible (ints beyond 2^53).
  `into_lua` only stages values into the batch buffer; the batched protected op of §7.3 does
  the pushes, and a Lua error there (OOM) surfaces as `ErrorKind::Lua`, not `ConvError`.
- Multiple arguments/returns: tuples implement `FromLuaMulti`/`IntoLuaMulti`;
  `Variadic<T>` for `...`.
- Vector/Angle use `ILuaBase::PushVector`/`PushAngle` (GMod allocates its own userdata and
  copies the 3 floats) and `GetVector`/`GetAngle` (return `const Vector&`/`const QAngle&`,
  i.e. a pointer to 3 `f32`; copied out immediately, only after a type check). Never
  `PushUserType` with Rust memory: it stores the raw pointer. Color is a table with GMod's
  `Color` metatable (fetched once per state, as `FindMetaTable("Color")`, and cached as a
  registry ref), so `IsColor` and color methods work on values Rust returns; `FromLua`
  accepts any table with `r,g,b[,a]`. All Vector/Angle/Color/Entity calls run protected.
- All `FromLua` reads run protected: arg conversion of an export (and of hook/`lua!` results)
  is one protected op that first calls `lua_checkstack` for the nesting depth it reaches, so
  nested tables cannot exhaust the `LUA_MINSTACK` reserve of §5.1. Reads honour metamethods
  (`lua_gettable`, `__len` via `lua_objlen` + `__len` check, `__index` class tables), matching
  what GLua code sees. Entity NULL/`IsValid`/`TypeID` checks run in the same op. Rust-side
  work between reads (building `String`, `Vec`) happens outside the callback: the op copies
  raw values (pointers/lengths/numbers) into a `Copy` buffer, Rust converts after it returns,
  with the stack slots kept alive by the `StackGuard`.
- Userdata of Rust types is checked with `lua_touserdata` + `lua_getmetatable` +
  `lua_rawequal` against the cached metatable ref; never `luaL_checkudata` (it raises).
- Map conversion copies a key (`lua_pushvalue`) before converting it to a string, so
  `lua_tolstring` never mutates the key used by `lua_next`.
- Registry-ref types (`LuaTable`, `LuaFunction`, `Entity`, ...) are `!Send` and refuse to
  work after `gmod13_close` (state generation counter). `Drop` never calls Lua: `luaL_unref`
  writes the freelist into `t[0]` and can allocate (raise). Instead `Drop` pushes the ref id
  onto a per-state deferred-unref queue (`RefCell<Vec<c_int>>`, never borrowed across Lua
  calls), flushed inside the next protected op and at close. `Drop` checks the generation
  (atomic) before touching the runtime map; after close (stale generation) it just discards
  the id. This keeps the `__gc` rule below intact: a boxed closure holding a `LuaFunction`
  dropped during `lua_close` sees a stale generation (close ran first) and never reaches the map.
  Before a protected op starts, the queue is drained into a fixed `Copy` array passed in its
  args struct (the callback never touches the `RefCell`); leftovers go to the next op.
- Rust closures from `create_function` live in a full userdata (upvalue of the C closure)
  whose `__gc` drops the boxed closure. During `lua_close` the per-state runtime may already
  be gone; `__gc` therefore only drops the box and never touches the runtime map or Lua.
  Every `__gc` (closures and `LuaUserData`) wraps the drop in `catch_unwind`: user `Drop` code
  must not unwind into LuaJIT's GC. A caught panic is logged via tier0 `Msg` and discarded.
- State generation is a per-DLL monotonic `AtomicU64`, never derived from the state address
  (a new state after map change may reuse the old main-thread pointer).

### 6.3 Derive

```rust
#[derive(LuaValue)]                    // FromLua + IntoLua
#[lua(rename_all = "PascalCase")]
struct Config {
    name: String,
    #[lua(default)] retries: u32,      // missing key -> Default
    #[lua(rename = "steamID")] steam_id: Option<String>,
    #[lua(skip)] cache: Vec<u8>,
}

#[derive(LuaValue)]
#[lua(tag = "kind")]                   // { kind = "Ban", minutes = 5 }
enum Action { Kick { reason: String }, Ban { minutes: u32 } }
```

- Unit-only enums map to strings (`"Red"`), with `#[lua(as_number)]` to use integers.
- `#[lua(as_string)]` on an `i64`/`u64` field converts via decimal string (SteamID64 etc.).
- Conversion errors carry a path: `bad argument #2 to 'mymod.Setup' (field 'Retries': expected number, got string)`.

### 6.4 Serde (feature `serde`)

- `LuaSerde<T>` wrapper implements `FromLua/IntoLua` for any `Serialize + DeserializeOwned`.
- Own `Serializer`/`Deserializer` over the stack (no JSON round trip).
- Same mapping rules as the derive; derive is preferred for speed and better errors.

## 7. Error model

### 7.1 For the developer

```rust
pub type Result<T, E = rsgdll::Error> = std::result::Result<T, E>;
```

- `rsgdll::Error` is an anyhow-style struct around a matchable kind:
  ```rust
  pub struct Error { kind: ErrorKind, context: Vec<String>, backtrace: Option<Box<Backtrace>> }

  #[non_exhaustive]
  pub enum ErrorKind {
      Lua { message: String, lua_traceback: Option<String> },
      StateClosed,
      Conversion(ConvError),
      Other(Box<dyn std::error::Error + Send + Sync>),
  }
  ```
  `err.kind()` returns `&ErrorKind` for matching. A blanket
  `impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error` makes `?` work on any
  std error (into `Other`). Coherence forbids that blanket impl if `Error` itself implements
  `std::error::Error`, so, like `anyhow::Error`, it does not; it implements `Display`/`Debug`,
  and `err.into_std()` returns a `Box<dyn std::error::Error + Send + Sync>` for interop with
  `anyhow`, `thiserror` `#[from]`, etc. `ConvError` does not implement `std::error::Error`
  either (its own `From<ConvError> for Error` would otherwise overlap the blanket impl).
  `.context("...")` (extension trait on `Result`/`Option`) pushes onto `context`; `Display`
  prints contexts outermost first, then the kind as `caused by: ...` (§7.2), and `kind()`
  matching is unaffected by context layers;
  `rsgdll::anyhow!` builds an `Other`. A `std::backtrace::Backtrace` is captured when the
  error is created as (or converted into) `rsgdll::Error` and stored in the `backtrace` field
  (`RUST_BACKTRACE` not required). Default: on in debug builds (`cfg(debug_assertions)`), off
  in release builds unless the cargo feature `backtrace` is enabled; `rsgdll::set_backtrace(bool)`
  (runtime, per DLL, `AtomicBool`) overrides either way. Reason: capture is slow, and scripts
  that use `pcall` for control flow would pay it on every `Err`. Conversion code (`FromLua`, enum/`Option` probing,
  `LuaValue` matching) uses a cheap internal `ConvError` with no backtrace; it only gets one
  when it becomes an `rsgdll::Error` or is raised by the trampoline, so probing failures are
  cheap. Symbol resolution is lazy and happens only when the error is formatted.
  Release builds with backtraces enabled should keep `debug = "line-tables-only"` so frames have file:line
  (Windows needs the `.pdb` next to the DLL, otherwise frames show addresses only).
- Exported functions may return `T`, `Result<T, E>` for any `E: Display`, or `()`. For an `E`
  that is not `rsgdll::Error` the trampoline has no useful backtrace (it would only show
  trampoline frames), so none is printed; return `rsgdll::Error` to get one.
- Calling Lua from Rust (`lua!`, `LuaFunction::call`) returns
  `Result<T, rsgdll::Error>`; a Lua error becomes kind `ErrorKind::Lua { message, lua_traceback }`
  so it composes with `?`.

### 7.2 What Lua sees

An exported function that returns `Err` or panics raises a normal Lua error, exactly as
`error()` would. `pcall` works as usual; without `pcall` GMod prints it in its standard
error format. Message format:

```
[mymod] mymod.LoadConfig: failed to read config.json: No such file or directory (os error 2)
caused by: ...
rust stack traceback:
    0: mymod::load_config at src/lib.rs:42
    1: ...
```

The `rust stack traceback` block appears only when a backtrace was captured (§7.1).
GMod then appends its own Lua stack traceback below, so the user sees both. Rust frames
belonging to rsgdll internals and std are trimmed.

Decided: the error value is a plain string (works with every existing `pcall` user and the
GMod error reporter).

### 7.3 How it is done safely (trampoline)

Every exported function is generated as:

```rust
unsafe extern "C-unwind" fn __rsgdll_tramp_load_config(l: *mut lua_State) -> c_int {
    // 1. All Rust work happens in a separate, non-inlined fn inside catch_unwind.
    //    `run` converts a panic payload into a pushed message and drops the payload itself,
    //    returning only a plain `Option<c_int>`.
    let nret: Option<c_int> = run(l, inner);
    // 2. Every Rust value (incl. any panic Box) is dropped by now; only a c_int remains.
    match nret {
        Some(n) => n,
        None => lua_error(l), // message already on stack top
    }
}
```

- `run` = `catch_unwind` + explicit handling: on `Err(payload)` it formats the panic message,
  `drop(payload)`, pushes the message, and returns `None`. Nothing with `Drop` is live in the
  trampoline frame when `lua_error` runs.
- Pushing the error message itself is a protected op and can fail on OOM. Fallback: each
  state pre-interns a static message (`"[mymod] error (message allocation failed)"`) in the
  registry at open; on failure it is pushed with `lua_rawgeti` (no allocation).

- `inner` converts args, calls the user fn, pushes returns or pushes the formatted error
  string, and returns. When `lua_error` runs, the only Rust frame is the trampoline,
  which owns nothing with `Drop`.
- Panics: a panic hook records the message + backtrace into a thread-local, then the
  trampoline formats it the same way (`rust panic: ...`). Inside a trampoline the default
  output is suppressed; outside, the hook chains to the previously installed hook
  (`take_hook` at install), so user/other hooks keep working.
- Accepted risk: on Linux x86 LuaJIT `longjmp`s over the trampoline frame. Rust permits this
  only for frames with no pending `Drop` ("plain old frames"); the trampoline is kept that
  way by construction, and the `probe-luajit` + showcase error tests verify it per target.
- Rust never calls a Lua API that can raise outside `lua_pcall`. This includes metamethod
  ops (`lua_gettable`, `lua_settable`, `lua_call`, comparisons, concat) **and every allocating
  call** (`lua_pushstring`/`pushlstring`, `lua_createtable`, `lua_newuserdata`,
  `lua_tolstring` on numbers, `luaL_ref`), because any of them can raise on out-of-memory.
- Protected ops are a fixed internal set of Rust `extern "C-unwind"` callbacks run with
  `lua_pcall` (no C code). `lua_cpcall` is not used for anything that returns values, since it
  discards the callback's results. Pushing a C function allocates, so each callback is created
  once at open (inside one `lua_cpcall` bootstrap) and stored in the registry; later
  protected calls fetch it with `lua_rawgeti` (no allocation), push the light-userdata args,
  and call `lua_pcall(L, 1, LUA_MULTRET, 0)`. Rules for every protected callback:
  - It only performs raw Lua API calls on `Copy` data (pointers, ints, lengths) read from
    a `#[repr(C)]` args struct passed as light userdata. It owns no value with `Drop`, so a
    longjmp (Linux x86) or foreign exception (x64, Win32) through it skips nothing.
  - It never calls user code or allocates in Rust, so it cannot panic. It has **no**
    `catch_unwind`: a LuaJIT error is a foreign exception on x64/Win32, and a foreign
    exception reaching `catch_unwind` aborts the process, turning every recoverable Lua error
    (including OOM) into a crash. Panic-freedom is by construction, checked with
    `#[deny(clippy::panic, clippy::unwrap_used, clippy::indexing_slicing)]` on the module.
  - There is no public `lua.protect(|| ...)` closure API: running arbitrary Rust inside
    `lua_pcall` is unsound. Safe operations (`get`, `set`, `call`, push of strings/tables)
    are built from these fixed callbacks.
- Batching keeps the cost down: e.g. pushing all return values or building a whole table
  happens in one protected call over a prepared args buffer, not one pcall per push.
- A Lua error from a protected op becomes `Err` with kind `ErrorKind::Lua` (out-of-memory included);
  a Lua exception never reaches a Rust `catch_unwind`.

## 8. Macros

### 8.1 Module entry and lifecycle

```rust
use rsgdll::prelude::*;

rsgdll::module!(name = "mymod");            // generates gmod13_open / gmod13_close

#[rsgdll::preopen]                           // before any export is registered
fn preopen(lua: &Lua) -> Result<()> { Ok(()) }

#[rsgdll::postopen]                          // after all #[function] exports are in _G
fn postopen(lua: &Lua) -> Result<()> {
    lua.print("mymod loaded")?;
    Ok(())
}

#[rsgdll::preclose]                          // Lua still fully usable, hooks/timers alive
fn preclose(lua: &Lua) {}

#[rsgdll::postclose]                         // after framework teardown, no Lua access
fn postclose() {}
```

Generated `gmod13_open` order:
1. Resolve `lua_shared` symbols and interfaces (once per DLL), pin the DLL
   (`GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN)` / `dlopen(self, RTLD_NODELETE)`) so it
   is never unmapped, install panic hook, create this state's runtime entry (§6.1a).
   Vtable sanity check: `ILuaInterface::IsServer/IsClient/IsMenu` must match the realm
   (exactly one true) and `ILuaBase::Top()` must equal `lua_gettop(L)`; on mismatch the module
   refuses to load with `vtable layout mismatch (GMod update?)` instead of risking corruption.
2. Run all `#[preopen]` fns (any number, any file, order = `#[preopen(order = N)]`, then name).
3. Register every `#[function]` / `#[methods]` / `#[hook]` / `#[timer]` (collected with
   `linkme` distributed slices, so items register themselves from any file of the `cdylib`
   crate). Registrations must live in the `cdylib` crate itself: items in a dependency rlib
   that nothing references can be discarded by the linker (LTO / `--gc-sections`). Documented
   limitation; dependency crates expose plain fns that the cdylib wraps.
4. Run all `#[postopen]` fns.
5. Start the main-thread executor pump (§10).

`gmod13_close`: `#[preclose]` -> remove framework hooks/timers -> fail every pending
`LuaHandle::call`/`call_blocking` with `ErrorKind::StateClosed` -> stop executor and signal worker
tasks (best effort, with timeout) -> drop registry refs, bump state generation, remove this
state's runtime entry -> `#[postclose]`.

- Because the DLL is pinned, worker threads or `thread_local!` destructors that outlive close
  cannot crash the process. Worker shutdown waits up to a timeout (default 2 s); threads still
  running are detached and logged (`Msg`/`ErrorNoHalt`: count + task names). The worker pool is
  per DLL and capped (default 2x CPUs), so detached threads cannot pile up across map changes.
  Consequence: a new build of the DLL is only picked up after a
  game/server restart; map change reuses the loaded image (with fresh per-state runtime).

- Every hook is optional. All are plain safe Rust fns; `Err` from a pre/postopen makes
  `require` fail with the formatted Lua error, and later steps are skipped. Cleanup runs
  inside `gmod13_open` itself before the raise (GMod is not assumed to call `gmod13_close`
  after a failed open): already-registered exports, hooks and timers are removed (through
  protected ops; a failure there is logged, not raised over the original error), the
  runtime entry is dropped, and `#[preclose]`/`#[postclose]` do not run. A later
  `gmod13_close` for that state is a no-op. Showcase test: a failing open followed by a
  second `require` in the same state.
- `module!` without hooks is valid; a module may consist of only `#[function]`s.
- `module!` emits `#[cfg(not(any(feature = "server", feature = "client", feature = "menu")))]
  compile_error!("declare and enable a realm feature")`, so a crate that forgot to declare the
  realm features (which would silently disable every cfg-gated realm check) fails to build.
  The user crate must declare all three features `server`, `client`, `menu` in `Cargo.toml`
  (the template does), otherwise the cfg-gated checks trigger `unexpected_cfgs` warnings.
- The module `name` is checked against the API database like export names (§8.2):
  `module!(name = "hook")` is a compile error unless `overwrite` is given.

### 8.2 Exported functions

```rust
#[rsgdll::function]                          // mymod.Add (module table)
fn add(a: i64, b: i64) -> i64 { a + b }

#[rsgdll::function(name = "Add")]            // _G.Add (explicit global)
fn add_global(a: i64, b: i64) -> i64 { a + b }

#[rsgdll::function(name = "mymod.config.Load")] // creates mymod / mymod.config tables if missing
fn load_config(path: String) -> Result<Config> { ... }

#[rsgdll::function(realm = "server")]        // only registered in the server state
fn kick(lua: &Lua, ply: PlayerRef<'_>, reason: Option<String>) -> Result<()> { ... }
```

- Arg types: anything `FromLua`; `&Lua` may appear as the first parameter; `Variadic<T>`
  last. `Option<T>` args are optional in Lua.
- Return: `IntoLuaMulti` or `Result` of it.
- Default name: `<module>.<PascalCase of the Rust fn>` (e.g. `mymod.Add`), so modules do not
  pollute `_G`. An explicit `name` is used as written: a dotted path is created from `_G`,
  a bare name goes into `_G`.
- Realm: `realm = "client"` fns in a `gmcl_` DLL also check `lua.realm()` at registration, so
  they are not registered when the same DLL is opened by the menu state.
- An export name (default or explicit) that equals a known GMod global or library from the
  API database (`Vector`, `Entity`, `Error`, `net`, ...) is a compile error; opt in with
  `#[rsgdll::function(overwrite)]`.
- Bad argument errors use Lua's style with the full export path:
  `bad argument #1 to 'mymod.Add' (number expected, got nil)`.
- Also `#[rsgdll::methods] impl Counter { ... }` for userdata types and
  `#[rsgdll::hook("Think")]` to add a `hook.Add` callback with an auto identifier.

### 8.3 `lua!`: inline Lua

```rust
let ply: Player = ...;
let limit = 5;
let count: i32 = lua!(lua, r#"
    local n = 0
    for _, e in ipairs(ents.FindByClass("prop_physics")) do
        if e:GetOwner() == $ply then n = n + 1 end
    end
    return math.min(n, $limit)
"#)?;
```

- The first argument is always the `Lua` handle expression (`&Lua` or `&LocalLua`, §10.2).
  It is explicit because macro hygiene cannot pick up a caller's local silently, and it keeps
  `lua!` usable in any scope.
- Two forms, same semantics and checks:
  - String form (full GLua): `lua!(lua, r#"..."#)`, captures written `$ident`.
  - Token form for short snippets: `lua!(lua, print("hi", $name))`, `let n: i32 = lua!(lua, #player.GetAll())?`.
    The macro rebuilds Lua source from tokens (using `Spacing::Joint` to rejoin `~=`, `..`, `::`).
    Captures are `$ident` (Lua `{}` stays a table constructor). Limits, caused by Rust's
    tokenizer: double-quoted strings only, no `[[...]]` long strings, no `--` comments
    (Rust `//` comments are dropped, which is fine), no single-quoted strings, no Lua number
    forms Rust cannot tokenize (`0x1p4`, `1e`), and newlines are not preserved (so the
    ambiguous `f\n(x)` call/statement split cannot be expressed). Inside a `macro_rules!`
    body `$ident` collides with metavariables; use the string form there. Violations
    give a `compile_error!` suggesting the string form. Token form has exact per-token spans
    on stable, so check errors point at the right token.
- `$ident` captures a Rust variable in both forms (like `format!` implicit capture). Lua never
  uses `$`, so there is no clash with table constructors and no escape syntax is needed.
  Named args: `lua!(lua, "... $name ...", name = expr)`.
- Escape hatch for wrong snippet data: `lua!(lua, unchecked, ...)` keeps the syntax check but
  turns member/arity/realm errors at that call site into nothing (data fixes still belong in
  `xtask/apidb-overrides.toml`).
- Captures are **not** pasted as text (no injection). The macro rewrites the chunk to
  `local __a1, __a2 = ...` + body, and passes captured values as call arguments
  through `IntoLua`.
- Chunks are compiled with `luaL_loadbuffer`. Verified on linux64 (`lua_shared.so`, fresh
  `luaL_newstate`): `!=`, `!`, `&&`, `||`, `//` and `/* */` comments, `continue` and `goto`
  all load, so GLua syntax lives in the patched lexer/parser, not in `RunString`.
  `cargo xtask probe-luajit` repeats this on the other 3 targets; where it fails, the macro desugars GLua to Lua 5.1 before emitting the chunk
  (`!=` -> `~=`, `!` -> `not`, `&&`/`||` -> `and`/`or`, comments stripped, `continue` ->
  `goto __continue_N` with a `::__continue_N::` label at the end of the loop body (LuaJIT
  supports `goto`; a `repeat ... until true` wrapper would break real `break`s); spans are
  kept via the AST).
- Each call site compiles its chunk once per Lua state (`luaL_loadbuffer`, chunk name
  `=file.rs:line:col`) and caches it in the registry, keyed by the address of a per-site
  `static` emitted by the macro (unique even for two `lua!` on one line).
- Return type is inferred from the binding (`FromLuaMulti`): `()`, `T`, tuple, `LuaValue`.
- Result: always `Result<T, rsgdll::Error>`; Lua runtime errors include Lua traceback
  and the Rust call site.
- Error positions from the compile-time checker map back to the exact span inside the
  string literal (proc-macro span of the literal + line/column offset; nightly-only
  sub-spans fall back to the whole literal on stable).

## 9. Compile-time Lua checking (`rsgdll-luacheck` + `rsgdll-apidb`)

### 9.1 API database

- `cargo xtask gen-apidb` clones glua-api-snippets branch `lua-language-server-addon`
  at a pinned commit, parses `library/*.lua` and `_globals.lua`, and writes a compact
  generated Rust/`bincode` table into `rsgdll-apidb`. It is vendored, so builds are
  offline and deterministic. The proc macro never touches the network.
- Extracted per entry:
  - globals and libraries (`net`, `ents`, `hook`, ...), global constants/enums
    (`TYPE_*`, `HUD_PRINTTALK`, ...), global variables (`SERVER`, `CLIENT`, ...).
  - functions: name, params (name, type, optional via `?` or `@param x? T`, vararg),
    returns, realm (from the `(Server)/(Client)/(Shared)/(Menu)` badge), deprecated flag.
  - class methods (`Entity:GetPos`, `Player:Nick`) and class inheritance (`---@class Player : Entity`).
- Hooks/gamemode callbacks (`GM:Think`) are indexed but only used for `hook.Add` name checks.

### 9.2 Checks

1. Parse with an own hand-written GLua parser (Lua 5.1 + LuaJIT + GLua: `!=`, `!`, `&&`,
   `||`, `//`, `/* */`, `continue`). Syntax errors = `compile_error!`.
   Reason: `full_moon` has a `luajit` feature but no GLua syntax, and rewriting `&&` or
   `continue` before parsing breaks spans. Lua 5.1 grammar is small, and an own AST keeps
   exact byte spans for error mapping and for the `{capture}` rewrite.
2. Unknown global read: warning, unless it is a local, a capture, or listed in
   `#[rsgdll::lua_globals(...)]` / a crate-level allowlist (addons define their own globals).
3. Unknown member of a known library (`net.WriteStrng`): error with "did you mean" suggestion.
4. Arity of calls to known library functions: too many args, or fewer than required
   non-optional args = warning in v1 (snippet data is community-maintained); promoted to an
   error per entry once confirmed or listed in `xtask/apidb-overrides.toml`. Vararg functions
   skip the max check.
5. Realm: calling a `(Client)` function in a server-only crate = error.
   Exactly one of `server`/`client` per build (`menu` may be added to `client`); enabling
   both (e.g. via workspace feature unification) is a `compile_error!`. A shared module is
   one crate built twice; `xtask build` builds each realm in a separate cargo invocation. A `client` crate may also be
   opened by the menu state, so `(Client)`-only calls are additionally guarded at runtime
   (`lua.realm()` check, `Err` instead of calling into a missing function). Proc macros cannot see the user crate's features, so
   the macro emits the check as cfg-gated code that rustc evaluates in the user crate:
   `#[cfg(all(feature = "server", not(feature = "client")))] compile_error!("...")` (warnings
   via a cfg-gated `#[deprecated]` item). The same mechanism is used for hook realms (§10.4).
6. Deprecated functions: warning (via `#[deprecated]` trick or `proc-macro-warning`).
7. Method calls (`e:GetOwner()`) are only checked when the receiver type is known: a
   capture with a known Rust type (`Player`, `Entity`, `Vector`), or a direct result of a
   known-return function. No general type inference in v1.
8. Return count: if the Rust binding expects a tuple of N, warn when every `return` in the
   chunk returns a different count.

Not checked in v1: argument types, table shapes, flow typing. A later option could run
`lua-language-server --check` from `xtask` (not from the macro).

## 10. Async and multithreading

Lua (LuaJIT) is single-threaded; the framework makes calling it from any thread safe by
always executing Lua work on the main thread and handing results back.

### 10.1 Core types

- `Lua` and all ref types stay `!Send`. Other threads get `LuaHandle`: `Send + Sync + Clone`,
  holding only a channel sender and a state generation id.
- Main-thread executor: a single-threaded async executor (own, small; `async-task` based)
  polled from a framework-owned `Think` hook (server/client) and from `Tick`/`DrawOverlay`
  fallback in menu state. Per-pump time budget (default 2 ms) so the game does not stall; the
  budget is checked between polls, so a single long `poll` is not preempted.

### 10.2 API

```rust
// From any thread: run a closure on the main thread, await its result. The closure returns
// `Result<T>`; `call` flattens it, so the await yields `Result<T>`, not `Result<Result<T>>`.
let name: String = handle.call(|lua| lua!(lua, GetHostName())).await?;
handle.spawn(|lua| { lua.print("hi")?; Ok(()) });       // fire and forget, errors logged
let v = handle.call_blocking(|lua| ...)?;               // for plain std threads; errors if
                                                        // called on the main thread (deadlock)

// From the main thread: run async Rust whose futures resolve across frames.
rsgdll::spawn_local(lua, |lua: LocalLua| async move {  // LocalLua: !Send, checks generation
    let body = rsgdll::spawn_worker(|| reqwest::blocking::get(url)?.text()).await?;
    lua!(&lua, print("got", #$body))?;                 // back on main thread here
    rsgdll::sleep(Duration::from_secs(1)).await;         // timer-backed, not thread sleep
    Ok::<_, rsgdll::Error>(())
});

// Async exported function: Lua passes a required callback (last function arg).
#[rsgdll::function]
async fn fetch(url: String) -> Result<String> { ... }   // Lua: mymod.Fetch(url, function(err, body) end)
```

- `spawn_worker` uses a framework thread pool; with feature `tokio`, one tokio runtime per
  DLL is started lazily on first use, shared by all states, and never shut down at close
  (close only cancels that state's tasks), so close never blocks on runtime shutdown.
  `rsgdll::tokio::spawn` futures can `handle.call(...)` back.
- `LocalLua` is a `!Send` handle held by a local future; each use checks the state generation
  and returns `Err` (kind `ErrorKind::StateClosed`) after close.
- Async exported functions: the future runs on the per-state main-thread executor (§10.1),
  so it may be `!Send` and may hold `LocalLua` and use `lua!` directly between awaits;
  blocking or CPU-heavy work goes through `spawn_worker`. The generated wrapper converts args
  synchronously, then `spawn_local`s the future.
- The Lua callback `function(err, ...)` is a required argument.
  By default it is the last positional Lua argument. If the export also has any `Option<T>` or `LuaFunction` param, the callback position must
  be marked with an explicit `#[lua(callback)] cb: Callback` param, otherwise compile error
  (a Lua call `f(url, cb)` would otherwise bind `cb` to the optional slot; no guessing).
  `Variadic<T>` is not allowed in async exports. A missing/non-function callback is a
  `bad argument` error raised synchronously. Rust
  `Err` is passed as `err` (string, same format as §7.2) because there is no caller stack
  to raise into anymore. If the Lua callback itself raises, the error is printed with
  `ErrorNoHalt` (module prefix + Lua traceback) and not returned to Rust. Uncaught errors in
  `spawn` are printed with `ErrorNoHalt`.
- After `gmod13_close` (or map change), `LuaHandle` calls return `Err` (kind `ErrorKind::StateClosed`)
  instead of touching a dead state. Pending futures are cancelled, and blocked
  `call_blocking` callers are released with `StateClosed` before close waits for workers.
- Wake-up mechanism: primary is `ILuaInterface::AddThreadedCall` with a Rust-implemented
  `ILuaThreadedCall` (GMod runs it on the main thread next cycle, no polling); fallback is the
  framework `Think` hook. Chosen at open by realm/branch support, tested both ways, including
  whether threaded calls still run while an empty server hibernates.
  **Unverified assumption:** that `AddThreadedCall` is safe to call from a non-main thread.
  `cargo xtask probe-threadedcall` (stress test from many threads in srcds) must pass per
  target before workers call it directly; until then the Think fallback is the default path.
- Hibernation: with no players and `sv_hibernate_think 0`, srcds stops `Think` and GMod
  timers. If threaded calls are unavailable, the executor, `handle.call`, `rsgdll::sleep`
  and `#[timer]` freeze until a player joins, exactly like GLua. Documented, not overridden.

### 10.3 GMod library helpers (`rsgdll-gmod`)

Thin safe wrappers that call the real Lua libraries (`hook`, `timer`, ...), so behaviour
matches GLua exactly. Callbacks are Rust closures; errors follow §7 (raised into the
GMod hook/timer runner, which prints them the normal way).

```rust
// hooks (string names: `*_custom`; typed built-ins: §10.4)
hook::add_custom("PlayerSay", "mymod.chat", |lua, (ply, text): (Option<PlayerRef<'_>>, String)| -> Result<Option<String>> {
    Ok(text.starts_with("!ping").then(|| "pong".into()))
})?;
hook::remove("PlayerSay", "mymod.chat")?;
let r: Option<bool> = hook::run_custom("MyModEvent", (1, "x"))?;
hook::call_custom("Name", gm_table, args)?;

#[rsgdll::hook(Think)]                       // registered at open, removed at close
fn on_think() -> Result<()> { Ok(()) }

// timers
timer::create("mymod.tick", Duration::from_secs(1), Repeat::Forever, |lua| Ok(()))?;
timer::simple(Duration::from_millis(500), |lua| lua.print("later"))?;
timer::adjust("mymod.tick", Duration::from_secs(2), Repeat::Times(5))?;
timer::remove("mymod.tick")?; timer::exists("mymod.tick")?; timer::pause/unpause/toggle(..)?;

#[rsgdll::timer(every = "1s")]               // or `delay = "500ms", repeat = 3`
fn tick() -> Result<()> { Ok(()) }

// also: concommand::add, cvars (CreateConVar/GetConVar), net::receive/start helpers,
// print/msg/error_no_halt, Realm checks.
```

- Identifiers created by the framework are tracked and removed in close, so reloading
  the module never leaves dangling hooks or timers pointing into an unloaded DLL
  (this is the classic GMod module crash).
- Hook callbacks get typed args via `FromLuaMulti` and typed return via `IntoLuaMulti`;
  return `None`/`()` to not override.

### 10.4 Type-checked built-in hooks

Possible, and checked by rustc itself rather than a custom checker. Data: glua-api-snippets
`gm.lua` has 267 `function GM:Name(...)` entries and `sandbox.lua` 53 `SANDBOX:` entries,
each with `---@param name Type`, optional `name?`, `---@return Type`, and a realm badge.

`gen-apidb` emits one marker type per hook in `rsgdll_gmod::hooks`:

```rust
/// Server. Called when a player dispatched a chat message. (wiki link)
pub struct PlayerSay;
impl Hook for PlayerSay {
    const NAME: &str = "PlayerSay";
    const REALM: Realm = Realm::Server;
    type Args<'a> = (Option<PlayerRef<'a>>, String, bool); // sender, text, teamChat
    type Ret = Option<String>;                // every hook return is optional (nil = no override)
}
```

Usage:

```rust
// Typed: closure params are inferred, wrong types/arity fail to compile with normal rustc errors.
// The closure always takes the full args tuple (use `_` for unused); only the attribute form
// below may declare fewer params.
// Risk: this needs `F: for<'a> Fn(&Lua, H::Args<'a>)` (HRTB over a GAT), which rustc closure
// inference often rejects ("not general enough"). Prototype first; if it fails, typed hooks
// are attribute-only (`#[rsgdll::hook(Ident)]` on fn items) and closures use `add_custom`.
hook::add::<hooks::PlayerSay>("mymod.chat", |lua, (ply, text, _team)| {
    Ok(text.starts_with("!ping").then(|| "pong".to_owned()))
})?;
hook::run::<hooks::PlayerSay>((ply, text, false))?;   // typed hook.Run too

#[rsgdll::hook(PlayerSay)]                           // ident = built-in, checked
fn on_say(ply: Option<PlayerRef<'_>>, text: String) -> Result<Option<String>> { ... }

// Custom/addon hooks: string name, no checking, any FromLuaMulti/IntoLuaMulti.
hook::add_custom("MyAddon.Event", "id", |lua, (a, b): (i32, String)| Ok(()))?;
#[rsgdll::hook("MyAddon.Event")]                     // string = unchecked
fn on_event(a: i32) -> Result<()> { Ok(()) }
```

Attribute macro rules (`#[rsgdll::hook(Ident)]`): the macro expands to a call that
requires `fn` params to implement `HookArgCompat<Hook::Args>`; it does not re-parse types itself.
- Entity-typed hook params (`Entity`, `Player`, `Weapon`, ...) are always generated as
  `Option<XRef<'a>>`: snippets do not encode nullability and many hooks pass NULL
  (attacker, inflictor). NULL -> `None`.
- Fewer params than the hook is allowed (Lua drops extras); more = compile error.
- A param may be a supertype-compatible Rust type: `Player` arg accepts `PlayerRef`, `EntityRef`,
  `LuaValue`; `number` accepts any numeric type; `?` optional params must be `Option<T>`.
- Return: `()`, `Option<T>`, or `Result` of either. `T` must match `Ret`.
- Realm: a server-only hook in a client-only crate = compile error (cfg-gated `compile_error!`
  emitted into the user crate, same mechanism as §9.2 check 5).
- Unknown ident (typo `PlayerSya`) = compile error with "did you mean"; string form skips all checks.
- Escape hatch for wrong snippet data: `#[rsgdll::hook(PlayerSay, unchecked)]`.

Snippet type -> Rust mapping (generated, overridable in `xtask/apidb-overrides.toml`):
`number` -> `f64` (int types accepted), `string` -> `String`, `boolean` -> `bool`,
`Player/Entity/Weapon/NPC/Vehicle` -> `Option<` matching ref type `>` (subclassing from `---@class X : Y`),
`Vector/Angle/Color` -> value types, `table`/`TraceResult` -> `LuaTable` (or typed struct when
overridden), `CTakeDamageInfo/CMoveData/CUserCmd/PhysObj/Panel` -> opaque ref types,
`any` and unknown names -> `LuaValue`, enum aliases (`BUTTON_CODE`) -> `i32`.
`SANDBOX:` hooks live in `hooks::sandbox` (only fire when the gamemode derives sandbox).

## 11. Build and packaging

- `cargo xtask build [--target all|win32|win64|linux|linux64] [--realm sv|cl]` builds
  `cdylib`s and renames to `gmsv_<name>_<platform>.dll` (`--realm sv`) or
  `gmcl_<name>_<platform>.dll` (`--realm cl`; also used by the menu state, feature `menu`
  adds the menu realm to a `gmcl_` build).
- Targets: `i686-pc-windows-msvc`, `x86_64-pc-windows-msvc`, `i686-unknown-linux-gnu`,
  `x86_64-unknown-linux-gnu`. Linux builds use an old glibc baseline (srcds compatibility)
  via `cargo-zigbuild` with a versioned target (e.g. `x86_64-unknown-linux-gnu.2.17`);
  Windows cross builds via `cargo-xwin`.
- `panic = "unwind"` is required (catch_unwind). The crate fails to build with
  `panic = "abort"` via a `cfg(panic = "abort")` `compile_error!`.

## 12. Testing

- Unit tests: conversion, derive, error formatting, luacheck (pure Rust, no GMod).
- Runtime tests without GMod: `rsgdll-sys` can also load a stock LuaJIT 2.1 shared library,
  so core behaviour (errors, unwinding, refs) is tested in `cargo test`. Stock LuaJIT does not
  accept GLua syntax, so these tests use plain Lua 5.1 only; GLua-syntax `lua!` runs are
  covered by srcds tests. GMod interface
  (`ILuaBase`/`ILuaInterface`) paths are only testable in real GMod.
- Local paths come from `xtask.toml` (gitignored; `xtask.toml.example` is committed with
  sample values) or env vars `RSGDLL_SRCDS_DIR` / `RSGDLL_GMOD_CLIENT_DIR`. No default
  paths are hard-coded; a missing value is an xtask error naming the key.
- Integration (actual run): `cargo xtask test-srcds` builds `examples/minimal` and
  `examples/showcase`, installs them into
  `$RSGDLL_SRCDS_DIR/garrysmod/lua/bin`, runs `srcds_run` with a test script via
  `+exec`/autorun test script (`examples/*/lua/autorun/server/*_test.lua`), and asserts on
  console lines `RSGDLL_TEST <name> PASS|FAIL <detail>` plus a final `RSGDLL_TEST_DONE`,
  then quits the server. Timeout = failure. Per platform branch.
- CI (GitHub Actions): Linux runners run `test-srcds` for linux64 and linux x86; Windows
  runners install a Windows srcds (SteamCMD, app 4020, cached) and run `test-srcds` for win64
  and win32. These are the only checks of MSVC vtable slot order (overloads, single destructor
  slot) and `thiscall`, so Windows targets are not released without a green Windows run.
- Client modules (`gmcl_*`): `cargo xtask install --realm cl` copies into
  `$RSGDLL_GMOD_CLIENT_DIR/garrysmod/lua/bin`;
  client tests are manual (game launched via Steam).

## 13. Examples

Both are real crates in the workspace, built and run by `cargo xtask test-srcds`.

### 13.1 `examples/minimal`

Smallest complete module; the template users copy.

```rust
use rsgdll::prelude::*;

rsgdll::module!(name = "minimal");

#[rsgdll::postopen]
fn open(lua: &Lua) -> Result<()> { lua.print("minimal: open") }

#[rsgdll::preclose]
fn close(lua: &Lua) { let _ = lua.print("minimal: close"); }

#[rsgdll::function]                       // Lua: minimal.Add(1, 2) -> 3
fn add(a: i64, b: i64) -> i64 { a + b }

#[rsgdll::function]                       // Lua: minimal.Divide(1, 0) -> raises Lua error
fn divide(a: f64, b: f64) -> Result<f64> {
    (b != 0.0).then(|| a / b).ok_or_else(|| rsgdll::anyhow!("division by zero"))
}

#[rsgdll::function]                       // quick demo: Rust -> Lua -> Rust
fn greet(lua: &Lua, name: String) -> Result<String> {
    let host: String = lua!(lua, GetHostName())?;
    Ok(format!("hello {name} from {host}"))
}
```

`xtask test-srcds` builds examples with feature `backtrace` so the traceback assertion holds
in release builds. Test script checks: `require("minimal")` succeeds, `minimal.Add(1, 2) == 3`,
`pcall(minimal.Divide, 1, 0)` returns `false` and the message contains `division by zero` and
`rust stack traceback`, `minimal.Divide(4, 2) == 2`, `minimal.Add("x")` gives a `bad argument #1` error,
`minimal.Greet("a")` returns a string; close message appears on shutdown.

### 13.2 `examples/showcase`

Exercises every feature; each block is one named test reported as `RSGDLL_TEST`.

| Area | What it shows / tests |
|---|---|
| Lifecycle | preopen/postopen/preclose/postclose all run, in order (logged sequence) |
| Exports | default `showcase.*` fn, explicit `_G` fn, nested `showcase.math.Clamp`, `realm = "server"` fn absent on client, varargs, optional args, multi-return; `trybuild` fail case for a name shadowing a GMod global without `overwrite` |
| Conversion | round trip of every §6.2 type; `#[derive(LuaValue)]` struct + tagged enum; serde feature type; Vector/Angle/Color/Entity/Player; i64 beyond 2^53 errors |
| Errors | `Err` -> Lua error string with backtrace; panic -> Lua error, server keeps running; Lua error inside `lua!` -> Rust `Err` with Lua traceback; conversion error path text |
| Userdata | `#[derive(LuaUserData)] Counter` with methods, `__tostring`, GC drop observed |
| `lua!` | string form + token form, captures, typed returns, tuple returns; compile-fail cases in `trybuild` tests (unknown lib member, wrong arity, realm misuse; unknown global = warning) |
| Interfaces | `ILuaBase` via `lua_State`, `CreateInterface("LUASHARED003")`, `ILuaInterface::is_server`, `run_string` |
| hook | `hook::add`/`run`/`remove`, `#[rsgdll::hook]`, return value overriding, typed built-in vs custom; `trybuild` fail cases (wrong arg type, too many params, realm mismatch, typo) |
| timer | `timer::simple`, `create` with repeat count, `adjust`, `remove`, `#[rsgdll::timer]` |
| Async | `LuaHandle::call` from std thread, `spawn_local` + `sleep`, `spawn_worker`, async `#[function]` callback, `StateClosed` after close, `call_blocking` released on close, threaded-call wake-up while hibernating |
| Coroutines | every Vector/Angle/pooled-string export called from inside a coroutine returns correct values (main-stack push + `lua_xmove`, §6.1a) |
| Lints | `trybuild` case: user crate with `#![forbid(unsafe_code)]` using `module!`/`#[function]`/`lua!` compiles (§1) |
| Reload | `changelevel` tears down and recreates the state; module is required again in the new state; no leftover hooks/timers, no crash. (Re-`require` in the same state returns the cached module, so it is not a reload test.) |
| Multi-module | two rsgdll modules loaded in one state, no key/hook/timer collisions |

## 14. Decisions log

One line per decision; the linked section is the source of truth.

| Area | Decision | § |
|---|---|---|
| Scope | Full v1 in one milestone; risks gated by tests (§15) | 2 |
| Language | Edition 2024, MSRV 1.85; pure Rust, no C/C++ shim | 1 |
| User contract | No `extern`/`unsafe` in user code; `forbid(unsafe_code)` support pending a test | 1 |
| Bindings | Own FFI; `ILuaBase`/`ILuaInterface`/`CreateInterface` from garrysmod_common via generator | 5.2 |
| Vtable ABI | `-unwind` ABIs; variadics `extern "C"`; raising/allocating methods only protected | 5.2 |
| Stack | `lua_checkstack` inside protected ops; 2-slot reserve kept outside | 5.1 |
| States | Per-DLL fn table/panic hook; per-state runtime keyed by `lua_topointer(registry)`; no poisoning | 6.1a |
| Refs | `Drop` queues ref id; unref flushed in next protected op / close | 6.2 |
| Coroutines | `ILuaBase` pushes on main stack (`lua_checkstack(main)`) + `lua_xmove` | 6.1a |
| Values | Ints checked to 2^53; `f64 -> f32` lossy; UTF-8 strict; `SteamId64`; `EntityRef` borrows; `Vec` holes error; `IntoLua` fallible; all reads protected, metamethods honoured; Color has metatable | 6.2 |
| Userdata | Metatable compare, never `luaL_checkudata`; `__gc` under `catch_unwind` | 6.2 |
| Error type | anyhow-style `Error { kind, context, backtrace }`; does not impl `std::error::Error`; `into_std()` | 7.1 |
| Backtrace | On in debug, off in release unless feature `backtrace`; runtime switch | 7.1 |
| Error value | Plain string to Lua | 7.2 |
| Trampoline | Owns nothing at `lua_error`; fixed protected callbacks, no `catch_unwind` in them; OOM -> `Err` | 7.3 |
| Lifecycle | pre/postopen, pre/postclose; DLL pinned; `linkme` in cdylib only; vtable sanity check at open; failed open cleans up itself | 8.1 |
| Exports | Default `mymod.Name`; shadowing GMod globals needs `overwrite` | 8.2 |
| `lua!` | Explicit handle arg; `$ident` captures; string + token forms; per-site cache; `unchecked` | 8.3 |
| Checker | Own GLua parser; unknown globals and arity warn (v1); realm via cfg-gated `compile_error!`; exactly one of server/client per build | 9 |
| Async | Main-thread executor; `AddThreadedCall` gated on probe, Think default; async exports on executor, callback last, no implicit callback with `Option` params | 10 |
| tokio | One lazy runtime per DLL, never shut down | 10.2 |
| Hooks | Typed `add`/`run`/`call::<H>`; string `*_custom`; entity args `Option<XRef>`; closure typing prototype-first | 10.4 |
| Build | `cargo-zigbuild` old glibc, `cargo-xwin`; `panic = "unwind"` required | 11 |
| Tests | srcds integration; Windows srcds CI before Windows release; paths via `xtask.toml` | 12 |

## 15. Open risks

Each risk has a gating check; the dependent feature ships only when it passes.

| Risk | Gate | Fallback if it fails |
|---|---|---|
| LuaJIT version/unwinding differs per target (32-bit may be 2.0.4) | `cargo xtask probe-luajit` on all 4 targets | Bind symbol intersection; per-target unwinding notes |
| Linux x86 `longjmp` over trampoline frame | `probe-luajit` + showcase error tests | Drop Linux x86 target |
| GLua syntax accepted by `luaL_loadbuffer` (linux64: verified) | `probe-luajit` per target | `lua!` desugars GLua to Lua 5.1 (§8.3) |
| `AddThreadedCall` from worker threads | `cargo xtask probe-threadedcall` | Think-hook wake-up (default until passed) |
| Typed hook closures (HRTB over GAT) | Prototype in `rsgdll-gmod` | Attribute-only typing; closures use `add_custom` |
| `forbid(unsafe_code)` with `mixed_site` spans | `trybuild` lint case (§13.2) | Drop guarantee, document `allow(unsafe_code)` |
| MSVC vtable slot order / `thiscall` | Windows srcds CI showcase interface tests | Fix generator overrides |
| Protected-op overhead (pcall per allocating op) | Benchmark before `rsgdll-core` API freeze | More batching in protected callbacks |
