# PROJECT KNOWLEDGE BASE

**Generated:** 2026-08-27T11:12:39Z
**Commit:** a91819d
**Branch:** master

## OVERVIEW
`rsgdll` is a Rust workspace for sound Garry's Mod binary Lua modules. Normal consumers depend only on public facade crate `rsgdll`; internal `rsgdll-*` crates remain implementation details.

## STRUCTURE
```text
rsgdll/
├── crates/                    # facade plus layered implementation crates
├── docs/                      # architecture and authoritative ABI references
├── examples/hello-module/     # public-facade `cdylib` example
├── e2e/                       # isolated real GMod/GLuaTest consumer harness
└── xtask/                     # artifact naming and staging
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Architecture/invariants | `docs/architecture.md` | Read before architectural changes |
| Public API | `crates/rsgdll/src/lib.rs` | Facade, prelude, optional re-exports |
| Raw Lua/GMod ABI | `crates/rsgdll-abi/` | Handwritten ABI only |
| Platform selection | `crates/rsgdll-platform/` | Target-specific behavior via `cfg` |
| Checked Lua API | `crates/rsgdll-lua/` | See child `AGENTS.md` |
| Lifecycle/dispatch | `crates/rsgdll-module/` | See child `AGENTS.md` |
| Main-thread runtime | `crates/rsgdll-runtime/` | Explicit capability and completion queue |
| Error firewall | `crates/rsgdll-bridge/` | See child `AGENTS.md` |
| Proc macros | `crates/rsgdll-macros/` | Generates stable facade plumbing |
| Source engine ABI/API | `crates/rsgdll-engine-sys/`, `crates/rsgdll-engine/` | Optional subsystem |
| Scanning/detouring | `crates/rsgdll-sigscan/`, `crates/rsgdll-detour/` | Optional low-level subsystem |
| Real runtime validation | `e2e/` | See child `AGENTS.md` |

## CODE MAP
Reference counts are unmeasured: workspace-wide LSP references were not reliable.

| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `Lua<'lua>` | struct | `crates/rsgdll-lua/src/lua.rs` | n/a | Checked callback-scoped Lua access |
| `StackFrame` | struct | `crates/rsgdll-lua/src/stack.rs` | n/a | Stack restoration/commit guard |
| `ModuleBuilder` | struct | `crates/rsgdll-module/src/builder.rs` | n/a | Registration lifecycle |
| `rust_dispatcher` | function | `crates/rsgdll-module/src/dispatcher.rs` | n/a | Rust FFI dispatch/panic boundary |
| `gmod13_open` | export | `crates/rsgdll-macros/src/lib.rs` | n/a | Generated GMod module entry |
| `MainThread` | capability | `crates/rsgdll-runtime/` | n/a | Explicit main-thread authority |
| `Engine::attach` | function | `crates/rsgdll-engine/src/lib.rs` | n/a | Optional engine entry |

## CONVENTIONS
- Dependency flow descends from `rsgdll` through module/Lua/runtime toward platform and raw ABI; low-level crates never depend upward.
- Public docs/examples prefer `use rsgdll::prelude::*`; macros resolve internals through `rsgdll::__private`.
- `rsgdll::raw` is an explicit escape hatch behind feature `raw`; do not broadly re-export ABI internals.
- Facade `default = []`; `detour -> sigscan`, `hook -> sigscan + detour`, and `full` excludes `raw`.
- OS/architecture selection uses Rust target `cfg`, never Cargo features.
- Safe Lua/context/reference types are main-thread-bound and receive capabilities explicitly.
- `#[rsgdll::module(close = hook)]` accepts only a safe `fn()` for final
  Lua-state/process teardown; it has no Lua access and does not make dynamic
  unload or reload supported.
- Rust-facing failures use `Result<T, E>` and preserve `Display` plus `Error::source()` chains.
- Unsafe code stays at real ABI/FFI boundaries, denies `unsafe_op_in_unsafe_fn`, and carries nearby `SAFETY:` invariants.
- Shared C/C++ ABI declarations originate in Rust stable-layout types and generate headers into `OUT_DIR`.
- Distinguish build-supported, ABI-verified, and E2E-verified targets.

## ANTI-PATTERNS (THIS PROJECT)
- Never introduce `gmod-rs`.
- Never let Lua error/`longjmp` remove a Rust stack frame.
- Never unwind Rust panic into C++ or Garry's Mod.
- Never use throwing Lua `Check*` functions as normal Rust conversion.
- Never manually implement `Send`/`Sync` for Lua-bound types without independently defensible proof.
- Never expose Rust allocations across FFI without explicit ownership and destruction.
- Never move Source engine ABI into `rsgdll-abi` or optional subsystems into core Lua abstractions.
- Never make normal consumers depend directly on internal crates.
- Never guess vtable order, calling conventions, state layouts, or platform offsets; cite authoritative upstream definitions.
- Never create speculative empty crates or features.

## UNIQUE STYLES
- C++ exists only as bounded Lua longjmp/error firewall; `crates/rsgdll-bridge/src/firewall.cpp` has enforced 600 pure-line ceiling.
- Rust returns fully before C++ raises Lua-visible failure; errors cross boundary as POD status plus caller-provided buffers.
- Generated module glue exports `gmod13_open`/`gmod13_close` and routes callbacks through one bridge trampoline.
- E2E consumer is intentionally outside root workspace and uses public facade exactly like external developers.

## COMMANDS
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --manifest-path examples/hello-module/Cargo.toml --locked
cargo package --workspace --allow-dirty --no-verify
cargo xtask stage --help
bash e2e/run.sh
```

## NOTES
- Current MSRV CI is Rust 1.88.
- `crates/rsgdll-lua/tests/core.rs`, module firewall tests, and bridge firewall are primary hotspots.
- Numbered implementation Parts stop exactly at requested Part boundary; do not pre-implement later Parts.
- Unsupported ABI behavior must be explicit, never a silent fallback.
