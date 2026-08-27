# RSGDLL LUA KNOWLEDGE BASE

## OVERVIEW
Checked, main-thread-bound Lua abstractions; score 10 from size, exports, and distinct safety domain.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Callback-scoped access | `src/lua.rs` | Checked `Lua<'lua>` capability |
| Stack lifetime | `src/stack.rs` | `StackFrame` restores or commits stack state |
| Rust/Lua conversion | `src/convert/` | Inspect first; return Rust `Result` errors |
| Tables and functions | `src/table.rs`, `src/function.rs` | Protected operations only |
| Lua references | `src/reference.rs` | Main-thread-bound registry handles |
| Userdata | `src/userdata.rs` | Metatable and method integration |
| Behavior tests | `tests/core.rs` | Primary Lua API regression surface |
| Test ABI | `tests/support/mock_lua.rs` | Mock must preserve stack/error behavior |

## CONVENTIONS
- Safe `Lua`, stack, function, table, reference, and userdata values remain `!Send` and `!Sync`.
- Decode by inspecting values and validating in Rust; do not use throwing Lua `Check*` calls.
- Calls into arbitrary Lua code require protected bridge operations.
- Keep raw-state construction and C-closure plumbing under `__private`.
- Stack mutations belong inside a `StackFrame`; commit only intentional return values.
- Conversion failures use typed Rust errors and preserve `Display`/`source` chains.

## ANTI-PATTERNS
- Never permit Lua `longjmp` to remove an active Rust frame.
- Never add manual `Send` or `Sync` implementations for Lua-bound types.
- Never expose raw ABI/vtable manipulation as checked API convenience.
- Never replace stack-lifecycle tests with mocks that cannot detect imbalance.
- Never move engine, detour, signature-scan, or async policy into this crate.
