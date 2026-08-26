# Raw Source engine ABI reference

Part 9's `CreateInterface` ABI is handwritten against Valve's Source SDK 2013
commit
[`9cbe7d5d3c847fb2e2be6049c3e2a72853bd6b85`](https://github.com/ValveSoftware/source-sdk-2013/commit/9cbe7d5d3c847fb2e2be6049c3e2a72853bd6b85):

- [`src/public/tier1/interface.h`](https://github.com/ValveSoftware/source-sdk-2013/blob/9cbe7d5d3c847fb2e2be6049c3e2a72853bd6b85/src/public/tier1/interface.h)

The checked engine wrapper uses only the first three slots of
`IVEngineServer021`, pinned to `danielga/sourcesdk-minimal` commit
[`6ddf32f8728ac63ab471865cfa4b9b08b94cb5fa`](https://github.com/danielga/sourcesdk-minimal/commit/6ddf32f8728ac63ab471865cfa4b9b08b94cb5fa):

- [`public/eiface.h`](https://github.com/danielga/sourcesdk-minimal/blob/6ddf32f8728ac63ab471865cfa4b9b08b94cb5fa/public/eiface.h#L78-L100)

Those slots are `ChangeLevel`, `IsMapValid`, and `IsDedicatedServer`.

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
