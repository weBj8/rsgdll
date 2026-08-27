# RSGDLL E2E KNOWLEDGE BASE

## OVERVIEW
Real Garry's Mod/GLuaTest consumer harness; score 8 from isolated workspace and distinct runtime/crash domain.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Full run | `run.sh` | Pins GLuaTest, builds, launches Docker |
| Module build | `build-module.sh` | Locked external-style `cdylib` build |
| Runtime entry | `runner-entrypoint.sh` | Server execution and outcome classification |
| Consumer crate | `module/` | Separate workspace using public `rsgdll` only |
| Lua tests | `addon/lua/tests/rsgdll_e2e/` | GLuaTest behavior surface |
| Container image | `Dockerfile` | GMod runtime dependencies |
| Crash artifacts | `artifacts/` | Logs, cores, backtraces when available |

## CONVENTIONS
- `module/Cargo.toml` keeps its own empty `[workspace]`; do not add it to root workspace.
- Consumer code depends on `rsgdll`, never internal `rsgdll-*` crates.
- Built modules must export `gmod13_open` and `gmod13_close` through normal loader naming.
- Keep pinned GLuaTest/runtime metadata reproducible and builds locked.
- Classify `TEST_FAILURE`, `MODULE_LOAD_FAILURE`, `SERVER_CRASH`, and `TIMEOUT` separately.
- Preserve Linux crash logs, core dumps, GDB backtraces, and exact binary metadata when available.

## ANTI-PATTERNS
- Never replace real module loading with direct internal-crate tests.
- Never silently convert crashes or load failures into ordinary test failures.
- Never depend on host-only artifacts when Docker build/staging is required.
- Never treat compile success as ABI or end-to-end runtime verification.
- Never remove expected-crash coverage merely to keep the harness green.
- Never make tests pass through fixed sleeps; wait on observable server/test state.
