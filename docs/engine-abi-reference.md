# Raw Source engine ABI reference

Part 9's `CreateInterface` ABI is handwritten against Valve's Source SDK 2013
commit
[`9cbe7d5d3c847fb2e2be6049c3e2a72853bd6b85`](https://github.com/ValveSoftware/source-sdk-2013/commit/9cbe7d5d3c847fb2e2be6049c3e2a72853bd6b85):

- [`src/public/tier1/interface.h`](https://github.com/ValveSoftware/source-sdk-2013/blob/9cbe7d5d3c847fb2e2be6049c3e2a72853bd6b85/src/public/tier1/interface.h)

The checked engine wrapper uses the first three slots and slot 36
(`ServerCommand`) of
`IVEngineServer021`, pinned to `danielga/sourcesdk-minimal` commit
[`6ddf32f8728ac63ab471865cfa4b9b08b94cb5fa`](https://github.com/danielga/sourcesdk-minimal/commit/6ddf32f8728ac63ab471865cfa4b9b08b94cb5fa):

- [`public/eiface.h`](https://github.com/danielga/sourcesdk-minimal/blob/6ddf32f8728ac63ab471865cfa4b9b08b94cb5fa/public/eiface.h#L78-L100)

Those slots are `ChangeLevel`, `IsMapValid`, `IsDedicatedServer`, and
`ServerCommand`.

The x86-64 logging listener ABI and C-exported registration functions are
pinned to `danielga/sourcesdk-minimal` commit
[`99cafecf352ef5e6af6b1dade2566a8680039152`](https://github.com/danielga/sourcesdk-minimal/commit/99cafecf352ef5e6af6b1dade2566a8680039152):

- [`public/tier0/logging.h`](https://github.com/danielga/sourcesdk-minimal/blob/99cafecf352ef5e6af6b1dade2566a8680039152/public/tier0/logging.h)

Only `ILoggingListener::Log`, the `LoggingContext_t` prefix, severity values,
`LCF_DO_NOT_ECHO`, and the register/unregister exports are represented.
GMod's Linux x86-64 `LoggingSystem_UnregisterLoggingListener` export is a
no-op. The checked wrapper therefore registers one process-lifetime listener
and only swaps its Rust callback while a guard is active.
GMod's Linux x86-64 `LoggingSystem_UnregisterLoggingListener` export is a
no-op. The checked wrapper therefore registers one process-lifetime listener
and only swaps its Rust callback while a guard is active.

That header defines:

```cpp
typedef void* (*CreateInterfaceFn)(const char *pName, int *pReturnCode);

enum
{
    IFACE_OK = 0,
    IFACE_FAILED
};
```

Only Linux x86_64 is currently header-defined. `engine.so` is the library
name shipped at `bin/linux64/engine.so` by the pinned real Garry's Mod E2E
image. Runtime verification remains part of that gate.
