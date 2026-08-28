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

The optional `engine` feature is build-supported on all four targets and uses
the selected target's C++ calling convention and loaded-library API. Its real
GMod runtime gate currently covers Linux x86_64. Optional `detour`, `hook`,
and therefore `full` retain their narrower support independently.

The xtask knows GMod's server/client filenames for Linux x86, Linux x86_64,
Windows x86, and Windows x86_64. Filename generation is packaging support,
not an ABI or E2E support claim. Linux x86_64 server E2E remains the native
runtime baseline, with Linux x86 also covered by a real server gate. No
Windows or client runtime gate is currently available. Win32 and Win64 have
not been tested in a real Garry's Mod process.
