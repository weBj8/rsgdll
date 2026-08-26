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

struct ReturnSlot {
    std::uint32_t tag;
    std::uint32_t offset;
    std::uint32_t length;
    std::uint32_t reserved;
    double number;
};

struct ReturnBuffer {
    ReturnSlot slots[16];
    std::uint8_t bytes[4096];
};

struct ModuleRegistration {
    const std::uint8_t *name;
    std::uint32_t name_length;
    std::uint32_t callback_id;
};

using Dispatcher =
    DispatchResult (*)(LuaState *, char *, std::uint32_t, ReturnBuffer *);
using ThrowError = void (*)(void *, const char *);
using CreateTable = void (*)(void *);
using RawSet = void (*)(void *, std::int32_t);
using PushNil = void (*)(void *);
using PushString = void (*)(void *, const char *, std::uint32_t);
using PushNumber = void (*)(void *, double);
using PushBool = void (*)(void *, bool);
using PushClosure = void (*)(void *, std::int32_t (*)(LuaState *), std::int32_t);
using SetState = void (*)(void *, LuaState *);
using ModuleInitializer =
    std::uint8_t (*)(ModuleRegistration *, std::uint32_t, std::uint32_t *);

static std::atomic<Dispatcher> dispatcher{nullptr};
static constexpr std::uint32_t error_capacity = 4096;
static constexpr std::uint32_t registration_capacity = 256;
static constexpr std::size_t throw_error_slot = 18;
static constexpr std::size_t create_table_slot = 6;
static constexpr std::size_t raw_set_slot = 22;
static constexpr std::size_t push_nil_slot = 28;
static constexpr std::size_t push_string_slot = 29;
static constexpr std::size_t push_number_slot = 30;
static constexpr std::size_t push_bool_slot = 31;
static constexpr std::size_t push_closure_slot = 33;
static constexpr std::size_t set_state_slot = 50;
static constexpr std::uint32_t return_nil = 0;
static constexpr std::uint32_t return_bool = 1;
static constexpr std::uint32_t return_number = 2;
static constexpr std::uint32_t return_string = 3;

extern "C" int rsgdll_bridge_trampoline(LuaState *state);

extern "C" void rsgdll_bridge_set_dispatcher(Dispatcher value) {
    dispatcher.store(value, std::memory_order_release);
}

extern "C" int rsgdll_bridge_trampoline(LuaState *state) {
    char error[error_capacity] = {};
    ReturnBuffer returns{};
    if (state != nullptr && state->lua_base != nullptr) {
        auto **vtable = *reinterpret_cast<void ***>(state->lua_base);
        reinterpret_cast<SetState>(vtable[set_state_slot])(state->lua_base, state);
    }
    const Dispatcher registered = dispatcher.load(std::memory_order_acquire);
    const DispatchResult result =
        registered == nullptr
            ? DispatchResult{1, 0, 0}
            : registered(state, error, error_capacity, &returns);

    if (result.status == 0) {
        if (state == nullptr || state->lua_base == nullptr ||
            result.return_count < 0 || result.return_count > 16) {
            return 0;
        }
        auto **vtable = *reinterpret_cast<void ***>(state->lua_base);
        for (std::int32_t index = 0; index < result.return_count; ++index) {
            const ReturnSlot &value = returns.slots[index];
            switch (value.tag) {
            case return_nil:
                reinterpret_cast<PushNil>(vtable[push_nil_slot])(state->lua_base);
                break;
            case return_bool:
                reinterpret_cast<PushBool>(vtable[push_bool_slot])(
                    state->lua_base, value.number != 0.0);
                break;
            case return_number:
                reinterpret_cast<PushNumber>(vtable[push_number_slot])(
                    state->lua_base, value.number);
                break;
            case return_string: {
                const std::uint64_t end =
                    static_cast<std::uint64_t>(value.offset) + value.length;
                if (end > sizeof(returns.bytes)) {
                    constexpr char invalid[] = "rsgdll produced invalid string return data";
                    reinterpret_cast<ThrowError>(vtable[throw_error_slot])(
                        state->lua_base, invalid);
                    return 0;
                }
                reinterpret_cast<PushString>(vtable[push_string_slot])(
                    state->lua_base,
                    value.length == 0
                        ? ""
                        : reinterpret_cast<const char *>(returns.bytes + value.offset),
                    value.length);
                break;
            }
            default: {
                constexpr char invalid[] = "rsgdll produced an invalid return value tag";
                reinterpret_cast<ThrowError>(vtable[throw_error_slot])(
                    state->lua_base, invalid);
                return 0;
            }
            }
        }
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

extern "C" int rsgdll_bridge_gmod13_open(
    LuaState *state,
    ModuleInitializer initializer) {
    if (state == nullptr || state->lua_base == nullptr ||
        initializer == nullptr) {
        return 0;
    }
    auto **vtable = *reinterpret_cast<void ***>(state->lua_base);
    reinterpret_cast<SetState>(vtable[set_state_slot])(state->lua_base, state);

    ModuleRegistration registrations[registration_capacity]{};
    std::uint32_t count = 0;
    if (initializer(registrations, registration_capacity, &count) == 0 ||
        count > registration_capacity) {
        return 0;
    }

    reinterpret_cast<CreateTable>(vtable[create_table_slot])(state->lua_base);
    for (std::uint32_t index = 0; index < count; ++index) {
        const ModuleRegistration &registration = registrations[index];
        if (registration.name == nullptr && registration.name_length != 0) {
            return 0;
        }
        reinterpret_cast<PushString>(vtable[push_string_slot])(
            state->lua_base,
            registration.name_length == 0
                ? ""
                : reinterpret_cast<const char *>(registration.name),
            registration.name_length);
        reinterpret_cast<PushNumber>(vtable[push_number_slot])(
            state->lua_base, static_cast<double>(registration.callback_id));
        reinterpret_cast<PushClosure>(vtable[push_closure_slot])(
            state->lua_base, rsgdll_bridge_trampoline, 1);
        reinterpret_cast<RawSet>(vtable[raw_set_slot])(state->lua_base, -3);
    }
    return 1;
}

extern "C" int rsgdll_bridge_gmod13_close(LuaState *) {
    return 0;
}

static_assert(offsetof(LuaState, lua_base) == 120);
static_assert(sizeof(DispatchResult) == 12);
static_assert(sizeof(ReturnSlot) == 24);
static_assert(sizeof(ReturnBuffer) == 4480);
static_assert(sizeof(ModuleRegistration) == 16);
