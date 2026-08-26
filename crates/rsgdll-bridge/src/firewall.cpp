#include <atomic>
#include <cstddef>
#include <cstdint>

struct LuaState {
    std::uint8_t opaque[120];
    void *lua_base;
};

struct DispatchResult {
    std::int32_t status;
    std::int32_t return_count;
    std::uint32_t error_length;
};

using Dispatcher = DispatchResult (*)(LuaState *, char *, std::uint32_t);
using ThrowError = void (*)(void *, const char *);

static std::atomic<Dispatcher> dispatcher{nullptr};
static constexpr std::uint32_t error_capacity = 4096;
static constexpr std::size_t throw_error_slot = 18;

extern "C" void rsgdll_bridge_set_dispatcher(Dispatcher value) {
    dispatcher.store(value, std::memory_order_release);
}

extern "C" int rsgdll_bridge_trampoline(LuaState *state) {
    char error[error_capacity] = {};
    const Dispatcher registered = dispatcher.load(std::memory_order_acquire);
    const DispatchResult result =
        registered == nullptr
            ? DispatchResult{1, 0, 0}
            : registered(state, error, error_capacity);

    if (result.status == 0) {
        return result.return_count;
    }

    if (result.error_length == 0) {
        constexpr char fallback[] = "rsgdll dispatcher failed without an error report";
        for (std::size_t index = 0; index < sizeof(fallback); ++index) {
            error[index] = fallback[index];
        }
    } else {
        const std::uint32_t end =
            result.error_length < error_capacity ? result.error_length : error_capacity - 1;
        error[end] = '\0';
    }

    if (state != nullptr && state->lua_base != nullptr) {
        auto **vtable = *reinterpret_cast<void ***>(state->lua_base);
        reinterpret_cast<ThrowError>(vtable[throw_error_slot])(state->lua_base, error);
    }
    return 0;
}

static_assert(offsetof(LuaState, lua_base) == 120);
static_assert(sizeof(DispatchResult) == 12);
