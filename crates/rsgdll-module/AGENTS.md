# RSGDLL MODULE KNOWLEDGE BASE

## OVERVIEW
Module lifecycle, registration, callback dispatch, and return staging; score 8 as central distinct domain.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Registration API | `src/builder.rs` | `ModuleBuilder`, `Function`, method install |
| Callback registry | `src/dispatcher.rs` | IDs, registry, dispatcher installation |
| FFI dispatch | `src/dispatcher.rs` | `rust_dispatcher`, panic/error conversion |
| Return staging | `src/returns.rs` | Values prepared before bridge tail action |
| Public exports | `src/lib.rs` | Facade plus hidden ABI plumbing |
| Firewall tests | `tests/firewall.rs` | Stack, error, panic, overflow behavior |

## CONVENTIONS
- Registration emits bridge descriptors; generated macros must not manipulate raw vtables.
- Catch unwind-mode panics at Rust FFI dispatch and keep panic diagnostics distinct from `Result::Err`.
- Complete every Rust frame before C++ raises a Lua-visible error.
- Pass `Lua` and `MainThread` capabilities explicitly into callbacks.
- Preserve exported module/function context and standard error source chains.
- Treat aborting panic strategy as unrecoverable; do not claim graceful conversion there.

## ANTI-PATTERNS
- Never unwind a Rust panic into C++ or Garry's Mod.
- Never call throwing Lua APIs from dispatcher-owned Rust frames.
- Never make callback registries a route to globally retrieve Lua state.
- Never expose internal bridge crates as required consumer dependencies.
- Never weaken firewall tests around stack restoration or overflow cleanup.
- Never encode normal errors as Lua `(value, err)` tuples; failures become Lua errors after Rust returns.
