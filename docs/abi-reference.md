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

## Linux x86_64 layout

`Interface.h` defines the 64-bit `lua_State` module prefix as:

```text
114 opaque bytes
6 alignment bytes
ILuaBase* at byte 120
total Rust prefix size 128 bytes
```

`LuaBase.h` declares `ILuaBase` virtual functions in slot order. The raw Rust
object contains its private vtable and state pointers (16 bytes total), and
its public operations are named, typed unsafe functions. The vtable address
point contains 55 callable slots: `Top` is slot 0, `SetState` is slot 50, and
`SetUserType` is slot 54. No public API accepts an arbitrary slot index.

This description assumes the default `ILuaBase` branch. Defining
`GMOD_USE_ILUAINTERFACE` selects a different interface and is unsupported.
Every raw call requires the caller to prevent Lua longjmp, C++ exceptions, or
Rust panics from crossing its Rust frame. Allocating and conversion operations
remain unsafe even when their names are not explicitly error-raising.

`Types.h` supplies type tags through `SurfaceInfo = 43` and
`Type_Count = 44`. `LuaType` is a transparent integer newtype so an unknown
future foreign value cannot create an invalid Rust enum.

## Support status

| Target | Build status | ABI status | E2E status |
| --- | --- | --- | --- |
| Linux x86_64 | build-supported | ABI-verified | E2E-verified (server) |
| all others | compile-time error | not reviewed | not verified |

The community `Interface.h` labels its 64-bit layout "not tested". rsgdll's
Linux x86_64 status therefore comes from the real Garry's Mod server and
GLuaTest gate, not from that comment or from compilation alone. See
[`targets.md`](targets.md) for the exact status vocabulary and matrix.
