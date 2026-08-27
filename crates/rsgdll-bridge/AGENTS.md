# RSGDLL BRIDGE KNOWLEDGE BASE

## OVERVIEW
Minimal C++/Rust Lua error firewall; score 9 from dense ABI symbols and unique native build constraints.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Shared ABI layouts | `src/lib.rs` | Rust-owned `#[repr(C)]` contract |
| Generated C header | `abi_header.rs` | Emitted into `OUT_DIR` |
| Native build | `build.rs` | C++17, exceptions disabled, line-budget gate |
| Throwing Lua calls | `src/firewall.cpp` | Only bounded longjmp-safe boundary |
| Trampoline dispatch | `src/firewall.cpp` | Stores and invokes Rust dispatcher |
| Bridge behavior | `../rsgdll-module/tests/firewall.rs` | End-to-end error/panic/stack cases |

## CONVENTIONS
- Rust formats errors into caller-provided fixed buffers, destroys temporaries, then returns.
- Shared fields use stable C representation and explicit-width integer types.
- C++ receives POD status/results and raises Lua errors only after Rust fully returns.
- `firewall_abi.h` is generated from Rust declarations; edit Rust source, not output.
- Non-trivial unsafe blocks require nearby `SAFETY:` invariants.
- Keep `#![deny(unsafe_op_in_unsafe_fn)]` effective at ABI boundaries.

## ANTI-PATTERNS
- Never move potentially throwing Lua calls into Rust frames.
- Never exceed 600 pure lines in `src/firewall.cpp`; build enforces this ceiling.
- Never use STL containers, C++ exceptions, or implicit cross-language ownership.
- Never return Rust-owned allocations whose cleanup depends on C++ returning after `lua_error`.
- Never duplicate ABI declarations manually across Rust and C++.
- Never move normal Lua abstractions, conversions, runtime logic, or user code into C++.
