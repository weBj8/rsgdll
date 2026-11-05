# Feasibility: uncaught engine Lua errors before unwind

**Result: not supportable as a stable rsgdll API.** No supported integration point exists. The
only pre-unwind point is GMod's private native error handler, which can be reached only with a
version-specific detour. rsgdll therefore exposes **no** uncaught-error observer. A
post-unwind callback would not be equivalent.

## Evidence (srcds Linux x86-64, LuaJIT 2.1.0-beta3, GMod 2026.04.29)

The probe was a server autorun script. Nested `outer -> inner` functions held a local `secret`
and an upvalue, then raised errors from `timer.Simple` and `hook` (`Think`) callbacks and from a
coroutine. A `pcall` case and a caught-then-rethrown case were also included.

| Question | Observation |
|---|---|
| `GM:OnLuaError` pre-unwind? | **No.** It fired 6 times, once per uncaught error and not for the `pcall` case. A full `debug.getinfo`/`getlocal` walk from inside the hook never finds the faulting frame (`NOT live` in all 6 cases). |
| Original error object preserved? | **No.** `error({code=1})` reaches `OnLuaError` as `CallFunctionProtected was passed a nil as the error message!`. |
| Caught vs uncaught distinguishable? | Yes, at the engine level: `pcall`-caught errors produce no report, and a genuine rethrow produces one report. |
| Pre-unwind handler visible to Lua? | **No.** The `[ERROR]` traceback lists the live frames, so the engine captures it before unwind. But a `"c"` (call) debug hook sees no C function invoked with the error value: the handler runs natively through LuaJIT's errfunc path, which does not fire hooks. |
| Coroutine errors | `coroutine.resume` returns `false, msg`. The engine reports nothing, so these are not engine-boundary errors. |
| `lua_atpanic` | Only covers unprotected paths. Engine callbacks run under `lua_pcall`. |
| `ILuaInterface::ErrorFromLua` | It is a vtable slot (`vtables.rs`), not a callback registration. Observing it would require patching the vtable or detouring code, and nothing shows that it runs before unwind. |

Line-, call- and count-hooks can inspect live frames safely (`rsgdll::debug`, issue 2), but Lua
5.1 has no error hook event. The `debug_hook_live_frame` srcds test covers that path.

## What intercepting it would take (not implemented)

There are two options:

- Detour GMod's private errfunc, the native `CallFunctionProtected` handler in the server/client
  binary.
- Replace the `errfunc` argument at the exported `lua_pcall` and forward to the original handler.

Both depend on private engine code, per-build symbols/offsets and the calling convention of
each of the eight targets: {Linux, Windows} × {x86, x86-64} × {server, client}. GMod's
`lua_shared` exports `lua_pcall`, but other GMod modules also call it. Telling engine dispatch
apart from script `pcall` would need return-address heuristics. Issue 3 rules this out:
"Do not add a broad generic detouring framework merely for this issue."

## Not verified

- Windows and client realms: no binaries/client were available. The Linux x86-64 result above
  is a server-only result.
- `net.Receive` needs a connected client.

The negative result rests on documented `OnLuaError` semantics plus the Linux x86-64 server
probe. It is not proof for the other targets.
