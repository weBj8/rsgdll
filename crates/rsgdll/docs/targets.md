# Target support

rsgdll uses three independent support claims:

- **build-supported**: a default-feature external consumer builds a loadable
  module artifact for the target. This says nothing about ABI correctness or
  whether Garry's Mod can load the result.
- **ABI-verified**: the Lua/Source layouts, virtual slots, and calling
  convention are tied to authoritative definitions and have passed relevant
  runtime checks.
- **E2E-verified**: a compiled external consumer module was loaded by a real
  Garry's Mod process and passed GLuaTest.

| Rust target | Build | ABI | Real GMod E2E |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | build-supported | ABI-verified | E2E-verified (server) |
| `i686-unknown-linux-gnu` | build-supported | ABI-verified | E2E-verified (server) |
| `i686-pc-windows-msvc` | build-supported | header-defined, runtime untested | untested |
| `x86_64-pc-windows-msvc` | build-supported | header-defined, runtime untested | untested |

Other targets are unsupported. The platform crate fails compilation rather
than selecting an ABI outside the pinned upstream header.

This matrix covers the default facade. Optional `engine`, `detour`, `hook`,
and `full` features remain Linux x86_64-only; enabling them does not inherit
the broader core target matrix.

The xtask knows GMod's server/client filenames for Linux x86, Linux x86_64,
Windows x86, and Windows x86_64. Filename generation is packaging support,
not an ABI or E2E support claim. Linux x86_64 server E2E remains the native
runtime baseline, with Linux x86 also covered by a real server gate. No
Windows or client runtime gate is currently available. Win32 and Win64 have
not been tested in a real Garry's Mod process.
