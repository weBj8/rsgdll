# Raw Lua ABI reference

Part 2 is handwritten against these exact sources:

- `danielga/garrysmod_common` commit
  [`f77a18d86f780a59ea30e4237016b05b790d4b70`](https://github.com/danielga/garrysmod_common/commit/f77a18d86f780a59ea30e4237016b05b790d4b70):
  - [`include/GarrysMod/Lua/Interface.h`](https://github.com/danielga/garrysmod_common/blob/f77a18d86f780a59ea30e4237016b05b790d4b70/include/GarrysMod/Lua/Interface.h)
  - [`include/GarrysMod/Lua/LuaBase.h`](https://github.com/danielga/garrysmod_common/blob/f77a18d86f780a59ea30e4237016b05b790d4b70/include/GarrysMod/Lua/LuaBase.h)
  - [`include/GarrysMod/Lua/Types.h`](https://github.com/danielga/garrysmod_common/blob/f77a18d86f780a59ea30e4237016b05b790d4b70/include/GarrysMod/Lua/Types.h)
- Facepunch's original `gmod-module-base` commit
  [`4fdafe7d762fe63f0360c67d9e119a129292712e`](https://github.com/Facepunch/gmod-module-base/commit/4fdafe7d762fe63f0360c67d9e119a129292712e)
  is retained as historical provenance, not as the current 64-bit state-layout
  source.
- Itanium C++ ABI revision
  [1.86](https://refspecs.linuxbase.org/cxxabi-1.86.html), sections 2.5 and
  3.2, defines GCC-compatible vtable order and virtual-call mechanics.
- System V AMD64 ABI
  [version 1.0](https://raw.githubusercontent.com/wiki/hjl-tools/x86-psABI/x86-64-psABI-1.0.pdf)
  defines the Linux x86_64 C calling convention used by the explicit `this`
  pointer function signatures.
- System V i386 ABI
  [fourth edition](https://refspecs.linuxfoundation.org/elf/abi386-4.pdf)
  defines the Linux x86 calling convention.
- Microsoft documents the
  [x64 calling convention](https://learn.microsoft.com/en-us/cpp/build/x64-calling-convention)
  and Win32
  [`__thiscall`](https://learn.microsoft.com/en-us/cpp/cpp/thiscall)
  used by the Windows targets.

## Target layouts

`Interface.h` defines two `lua_State` module prefixes:

```text
Linux x86 / Windows x86:
70 opaque bytes
2 alignment bytes
ILuaBase* at byte 72
total Rust prefix size 76 bytes

Linux x86_64 / Windows x86_64:
114 opaque bytes
6 alignment bytes
ILuaBase* at byte 120
total Rust prefix size 128 bytes
```

`LuaBase.h` declares `ILuaBase` virtual functions in slot order. The raw Rust
object contains its private vtable and state pointers (8 bytes on x86, 16
bytes on x86_64), and its public operations are named, typed unsafe functions.
The vtable address point contains 55 callable slots: `Top` is slot 0,
`SetState` is slot 50, and `SetUserType` is slot 54. No public API accepts an
arbitrary slot index.
Linux and Windows x86_64 pass `this` using their platform C ABI. Windows x86
uses the C++ `thiscall` convention; both the Rust raw methods and generated C++
firewall function pointers select it explicitly.

This description assumes the default `ILuaBase` branch. Defining
`GMOD_USE_ILUAINTERFACE` selects a different interface and is unsupported.
Every raw call requires the caller to prevent Lua longjmp, C++ exceptions, or
Rust panics from crossing its Rust frame. Allocating and conversion operations
remain unsafe even when their names are not explicitly error-raising.

The checked `rsgdll-lua` layer does not invoke potentially throwing raw
operations directly. The C++ bridge prepares an executor closure before
entering Rust, then runs each POD-described mutation through
`ILuaBase::PCall`. Only exact-type, non-coercing reads remain direct.

## Foreign runtime contract

The supported foreign implementation is Garry's Mod's pinned default
`ILuaBase` runtime. Exact-type reads used directly by checked Rust code must
return normally; a replacement `ILuaBase` implementation that throws a C++
exception is unsupported. The bridge is compiled with C++ exceptions disabled,
and no C++ exception may cross its C ABI or a Rust frame.

Lua errors remain supported only through the documented `longjmp`/`PCall`
path. Potentially throwing mutations execute inside the prepared C++ executor,
while the direct reads are limited to the empirically verified non-throwing
operations described above.

## Module lifecycle

Version 0.1 does not support dynamically unloading or reloading a compiled
module. The module must remain loaded until its Lua state and process are being
torn down, after the host can no longer invoke exported closures or userdata
finalizers. `gmod13_close` performs no Rust cleanup by default. Modules may
register a teardown-only safe `fn()` with
`#[rsgdll::module(close = on_close)]`; the hook cannot access Lua and its panic
is contained at the FFI boundary when `panic = "unwind"`. Unloading the shared
object while Lua retains native callbacks would still leave stale function
pointers and cannot be made safe by this hook.

`Types.h` supplies type tags through `SurfaceInfo = 43` and
`Type_Count = 44`. `LuaType` is a transparent integer newtype so an unknown
future foreign value cannot create an invalid Rust enum.

## Support status

| Target | Build status | ABI status | E2E status |
| --- | --- | --- | --- |
| Linux x86_64 | build-supported | ABI-verified | E2E-verified (server) |
| Linux x86 | build-supported | ABI-verified | E2E-verified (server) |
| Windows x86 | build-supported | header-defined, runtime untested | untested |
| Windows x86_64 | build-supported | header-defined, runtime untested | untested |
| all others | compile-time error | not reviewed | not verified |

The community `Interface.h` labels its 64-bit layout "not tested". rsgdll's
Linux x86_64 status therefore comes from the real Garry's Mod server and
GLuaTest gate, not from that comment or from compilation alone. See
[`targets.md`](targets.md) for the exact status vocabulary and matrix.
