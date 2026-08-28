#include <atomic>
#include <cstring>

#include "firewall_abi.h"

template <typename Function, typename... Arguments>
static decltype(auto) call_lua(void *lua_base, std::size_t slot, Arguments... arguments) {
    void **vtable = nullptr;
    std::memcpy(&vtable, lua_base, sizeof(vtable));
    return reinterpret_cast<Function>(vtable[slot])(lua_base, arguments...);
}

static std::atomic<Dispatcher> dispatcher{nullptr};
#ifdef RSGDLL_DEBUG_NATIVE
static std::atomic<DebugDispatcher> debug_dispatcher{nullptr};
#endif
static std::atomic<const AbiLayout *> abi_layout{nullptr};
static constexpr std::uint32_t registration_capacity = 256;
static constexpr std::int32_t lua_multret = -1;
// ponytail: fixed bound keeps Rust-side setup non-allocating; raise only with
// an E2E case requiring wider Lua calls.
static constexpr std::int32_t operation_value_limit = 64;
static constexpr std::int32_t reserved_stack_slots = 256;
static const AbiLayout *current_abi_layout() {
    return abi_layout.load(std::memory_order_acquire);
}
static constexpr std::int32_t special_global = 0;
static constexpr std::int32_t special_registry = 2;
static constexpr std::int32_t lua_type_nil = 0;
static constexpr char callback_guard_key[] = "rsgdll.__private.rust_frame_active.v1";
enum : std::uint32_t {
    op_guard_check = op_set_user_type + 1, op_guard_set, op_guard_clear,
};

struct ProtectedContext {
    LuaState *state;
    void *lua_base;
    std::int32_t executor_reference, stack_limit;
    LuaOperation *operation;
};

static void *state_lua_base(LuaState *state) {
    const AbiLayout *layout = current_abi_layout();
    if (state == nullptr || layout == nullptr) {
        return nullptr;
    }
    void *result = nullptr;
    std::memcpy(&result,
        reinterpret_cast<std::uint8_t *>(state) + layout->lua_base_offset, sizeof(result));
    return result;
}

static thread_local ProtectedContext protected_context_storage{};
static thread_local ProtectedContext *protected_context = nullptr;
#ifdef RSGDLL_TEST_SUPPORT
static thread_local std::int32_t last_dispatch_status = status_success;
#endif

static std::int32_t execute_operation(LuaState *state);

static bool prepare_context(LuaState *state, void *lua_base) {
    if (protected_context != nullptr || state == nullptr ||
        lua_base == nullptr || state_lua_base(state) != lua_base) {
        return false;
    }
    call_lua<SetState>(lua_base, set_state_slot, state);
    for (std::int32_t index = 0; index < reserved_stack_slots; ++index) {
        call_lua<PushNil>(lua_base, push_nil_slot);
    }
    call_lua<Pop>(lua_base, pop_slot, reserved_stack_slots);
    call_lua<PushClosure>(lua_base, push_closure_slot, execute_operation, 0);
    const std::int32_t executor_reference =
        call_lua<ReferenceCreate>(lua_base, reference_create_slot);
    const std::int32_t top = call_lua<Top>(lua_base, top_slot);
    protected_context_storage = ProtectedContext{
        state, lua_base, executor_reference, top + reserved_stack_slots / 2, nullptr};
    protected_context = &protected_context_storage;
    return true;
}

static void finish_context(ProtectedContext *context) {
    protected_context = nullptr;
    call_lua<ReferenceFree>(
        context->lua_base, reference_free_slot, context->executor_reference);
}

static std::int32_t execute_operation(LuaState *state) {
    ProtectedContext *context = protected_context;
    void *lua_base = state_lua_base(state);
    if (context == nullptr || context->operation == nullptr ||
        context->state != state || context->lua_base != lua_base) {
        return 1;
    }
    LuaOperation *operation = context->operation;
    switch (operation->opcode) {
        case op_push: call_lua<Push>(lua_base, push_slot, 1); return 1;
        case op_create_table: call_lua<CreateTable>(lua_base, create_table_slot); return 1;
        case op_pcall:
            operation->result_integer =
                call_lua<PCall>(lua_base, pcall_slot, operation->a, operation->b, 0);
            return operation->result_integer == 0 ? operation->b : 1;
        case op_set_meta_table: call_lua<SetMetaTable>(lua_base, set_meta_table_slot, 1); return 0;
        case op_new_userdata:
            operation->result_pointer =
                call_lua<NewUserdata>(lua_base, new_userdata_slot, operation->length);
            return 1;
        case op_raw_get: call_lua<Push>(lua_base, raw_get_slot, 1); return 1;
        case op_raw_set:
            call_lua<Push>(lua_base, push_slot, 2);
            call_lua<Push>(lua_base, push_slot, 3);
            call_lua<Push>(lua_base, raw_set_slot, 1);
            return 0;
        case op_next:
            operation->result_integer = call_lua<Next>(lua_base, next_slot, 1);
            return operation->result_integer == 0 ? 0 : 2;
        case op_push_nil: call_lua<PushNil>(lua_base, push_nil_slot); return 1;
        case op_push_string:
            call_lua<PushString>(lua_base, push_string_slot,
                static_cast<const char *>(operation->pointer),
                operation->length);
            return 1;
        case op_push_number: call_lua<PushNumber>(lua_base, push_number_slot, operation->number); return 1;
        case op_push_bool: call_lua<PushBool>(lua_base, push_bool_slot, operation->a != 0); return 1;
        case op_push_c_closure:
            call_lua<PushClosure>(lua_base, push_closure_slot,
                reinterpret_cast<std::int32_t (*)(LuaState *)>(
                    const_cast<void *>(operation->pointer)),
                operation->a);
            return 1;
        case op_reference_create:
            operation->result_integer = call_lua<ReferenceCreate>(lua_base, reference_create_slot);
            return 0;
        case op_reference_free: call_lua<ReferenceFree>(lua_base, reference_free_slot, operation->a); return 0;
        case op_reference_push: call_lua<ReferencePush>(lua_base, reference_push_slot, operation->a); return 1;
        case op_push_special: call_lua<PushSpecial>(lua_base, push_special_slot, operation->a); return 1;
        case op_create_meta_table:
            operation->result_integer = call_lua<CreateMetaTable>(lua_base, create_meta_table_slot,
                static_cast<const char *>(operation->pointer));
            return 1;
        case op_push_meta_table:
            operation->result_integer =
                call_lua<PushMetaTable>(lua_base, push_meta_table_slot, operation->a);
            return operation->result_integer == 0 ? 0 : 1;
        case op_set_user_type:
            call_lua<SetUserType>(lua_base, set_user_type_slot, 1,
                const_cast<void *>(operation->pointer));
            return 0;
        case op_guard_check:
            call_lua<PushSpecial>(lua_base, push_special_slot, special_registry);
            call_lua<PushString>(lua_base, push_string_slot, callback_guard_key,
                sizeof(callback_guard_key) - 1);
            call_lua<Push>(lua_base, raw_get_slot, -2);
            operation->result_integer = call_lua<GetType>(lua_base, get_type_slot, -1) != lua_type_nil;
            call_lua<Pop>(lua_base, pop_slot, 2);
            return 0;
        case op_guard_set:
        case op_guard_clear:
            call_lua<PushSpecial>(lua_base, push_special_slot, special_registry);
            call_lua<PushString>(lua_base, push_string_slot, callback_guard_key,
                sizeof(callback_guard_key) - 1);
            if (operation->opcode == op_guard_set) {
                call_lua<PushBool>(lua_base, push_bool_slot, true);
            } else {
                call_lua<PushNil>(lua_base, push_nil_slot);
            }
            call_lua<RawSet>(lua_base, raw_set_slot, -3);
            call_lua<Pop>(lua_base, pop_slot, 1);
            return 0;
        default:
            operation->reserved = 1;
            return 0;
    }
}

static std::int32_t absolute_index(std::int32_t top, std::int32_t index) {
    return index >= 0 || index <= -10000 ? index : top + index + 1;
}

static void relocate_values(void *lua_base, std::int32_t first, std::int32_t last) {
    for (std::int32_t index = first; index <= last; ++index) {
        call_lua<Push>(lua_base, push_slot, index);
    }
    for (std::int32_t index = last; index >= first; --index) {
        call_lua<Remove>(lua_base, remove_slot, index);
    }
}

static std::int32_t execute_in_context(
    LuaState *state, void *lua_base, LuaOperation *operation) {
    ProtectedContext *context = protected_context;
    if (context == nullptr || context->operation != nullptr ||
        context->state != state || context->lua_base != lua_base ||
        state_lua_base(state) != lua_base || operation == nullptr) {
        return -1;
    }
    const std::int32_t top = call_lua<Top>(lua_base, top_slot);
    const bool cleanup =
        operation->opcode == op_pop ||
        operation->opcode == op_reference_free ||
        operation->opcode == op_guard_clear;
    if (top > context->stack_limit && !cleanup) {
        return -2;
    }
    operation->reserved = 0;
    context->operation = operation;
    if (operation->opcode == op_pop) {
        call_lua<Pop>(lua_base, pop_slot, operation->a);
        context->operation = nullptr;
        return 0;
    }

    std::int32_t argument_count = 0;
    std::int32_t result_count = 0;
    call_lua<ReferencePush>(
        lua_base, reference_push_slot, context->executor_reference);
    switch (operation->opcode) {
        case op_push: {
            const std::int32_t source = absolute_index(top, operation->a);
            call_lua<Push>(lua_base, push_slot, source);
            argument_count = 1;
            result_count = 1;
            break;
        }
        case op_create_table:
        case op_new_userdata:
        case op_push_nil:
        case op_push_string:
        case op_push_number:
        case op_push_bool:
        case op_reference_push:
        case op_push_special:
        case op_create_meta_table:
            result_count = 1;
            break;
        case op_reference_free:
        case op_guard_check:
        case op_guard_set:
        case op_guard_clear:
            break;
        case op_push_meta_table:
            result_count = lua_multret;
            break;
        case op_pcall: {
            if (operation->a < 0 || operation->b < 0 ||
                operation->a > operation_value_limit ||
                operation->b > operation_value_limit) {
                context->operation = nullptr;
                return -2;
            }
            const std::int32_t first = top - operation->a;
            relocate_values(lua_base, first, top);
            argument_count = operation->a + 1;
            result_count = lua_multret;
            break;
        }
        case op_set_meta_table: {
            const std::int32_t target = absolute_index(top, operation->a);
            call_lua<Push>(lua_base, push_slot, target);
            relocate_values(lua_base, top, top);
            argument_count = 2;
            break;
        }
        case op_raw_get:
        case op_next: {
            const std::int32_t table = absolute_index(top, operation->a);
            call_lua<Push>(lua_base, push_slot, table);
            relocate_values(lua_base, top, top);
            argument_count = 2;
            result_count =
                operation->opcode == op_raw_get ? 1 : lua_multret;
            break;
        }
        case op_raw_set: {
            const std::int32_t table = absolute_index(top, operation->a);
            call_lua<Push>(lua_base, push_slot, table);
            relocate_values(lua_base, top - 1, top);
            argument_count = 3;
            break;
        }
        case op_push_c_closure: {
            if (operation->a < 0 || operation->a > operation_value_limit) {
                context->operation = nullptr;
                return -2;
            }
            const std::int32_t first = top - operation->a + 1;
            relocate_values(lua_base, first, top);
            argument_count = operation->a;
            result_count = 1;
            break;
        }
        case op_reference_create: {
            relocate_values(lua_base, top, top);
            argument_count = 1;
            break;
        }
        case op_set_user_type: {
            const std::int32_t target = absolute_index(top, operation->a);
            call_lua<Push>(lua_base, push_slot, target);
            argument_count = 1;
            break;
        }
        default:
            context->operation = nullptr;
            return -2;
    }
    const std::int32_t status = call_lua<PCall>(
        lua_base, pcall_slot, argument_count, result_count, 0);
    context->operation = nullptr;
    if (status != 0) {
        return status;
    }
    return operation->reserved == 0 ? 0 : -3;
}

extern "C" std::int32_t rsgdll_bridge_execute(
    LuaState *state, void *lua_base, LuaOperation *operation) {
    if (protected_context != nullptr) {
        return execute_in_context(state, lua_base, operation);
    }
#ifdef RSGDLL_TEST_SUPPORT
    if (!prepare_context(state, lua_base)) {
        return -1;
    }
    const std::int32_t status = execute_in_context(state, lua_base, operation);
    finish_context(protected_context);
    return status;
#else
    return -1;
#endif
}

#ifdef RSGDLL_TEST_SUPPORT
extern "C" void rsgdll_bridge_enable_test_mode(const AbiLayout *layout) {
    abi_layout.store(layout, std::memory_order_release);
}

extern "C" std::int32_t rsgdll_bridge_test_last_dispatch_status() {
    return last_dispatch_status;
}
#endif

extern "C" int rsgdll_bridge_trampoline(LuaState *state);

extern "C" void rsgdll_bridge_set_dispatcher(Dispatcher value) {
    dispatcher.store(value, std::memory_order_release);
}

#ifdef RSGDLL_DEBUG_NATIVE
extern "C" void rsgdll_bridge_debug_set_dispatcher(DebugDispatcher value) {
    debug_dispatcher.store(value, std::memory_order_release); }

extern "C" void rsgdll_bridge_debug_hook(LuaState *state, void *record) {
    if (protected_context != nullptr || record == nullptr) {
        return;
    }
    void *base = state_lua_base(state);
    if (base == nullptr || !prepare_context(state, base)) {
        return;
    }
    const DebugDispatcher registered = debug_dispatcher.load(std::memory_order_acquire);
    if (registered != nullptr) {
        registered(state, record);
    }
    finish_context(protected_context);
}
#endif

static void raise_bridge_error(void *lua_base, const char *message) {
    if (lua_base == nullptr) {
        return;
    }
    call_lua<ThrowError>(lua_base, throw_error_slot, message);
}

static int fail_callback(
    void *lua_base, ProtectedContext *context, const char *message) {
    if (context != nullptr) {
        finish_context(context);
    }
    raise_bridge_error(lua_base, message);
    return 0;
}

static std::int32_t execute_guard(
    LuaState *state, void *lua_base, std::uint32_t opcode,
    std::int64_t *result = nullptr) {
    LuaOperation operation{};
    operation.opcode = opcode;
    const std::int32_t status = execute_in_context(state, lua_base, &operation);
    if (result != nullptr) {
        *result = operation.result_integer;
    }
    return status;
}

static bool accepts_dispatch_result(
    const DispatchResult &result, std::int32_t entry_top, std::int32_t exit_top) {
    if (entry_top < 0 || exit_top < entry_top ||
        result.return_count < 0) {
        return false;
    }
    switch (result.status) {
        case status_rust_error:
        case status_rust_panic:
        case status_internal_error:
            return result.return_count == 0 && exit_top == entry_top;
        case status_success:
            break;
        default:
            return false;
    }
    if (result.return_mode == return_mode_stack) {
        return result.return_count == exit_top - entry_top;
    }
    return result.return_mode == return_mode_staged &&
        result.return_count <= 16 && exit_top == entry_top;
}

extern "C" int rsgdll_bridge_trampoline(LuaState *state) {
    void *base = state_lua_base(state);
    if (protected_context != nullptr) {
        constexpr char reentrant[] =
            "rsgdll callback re-entry during protected Lua operation";
        raise_bridge_error(base, reentrant);
        return 0;
    }
    char error[error_capacity] = {};
    ReturnBuffer returns{};
    const bool context_ready = base != nullptr && prepare_context(state, base);
    if (!context_ready) {
        return fail_callback(
            base, nullptr, "rsgdll failed to prepare protected callback context");
    }

    std::int64_t guard_active = 0;
    if (execute_guard(state, base, op_guard_check, &guard_active) != 0) {
        return fail_callback(
            base, protected_context, "rsgdll failed to inspect callback re-entry guard");
    }
    if (guard_active != 0) {
        return fail_callback(
            base, protected_context, "rsgdll callback re-entry across loaded modules");
    }
    if (execute_guard(state, base, op_guard_set) != 0) {
        return fail_callback(
            base, protected_context, "rsgdll failed to install callback re-entry guard");
    }

    const Dispatcher registered = dispatcher.load(std::memory_order_acquire);
    const std::int32_t entry_top = call_lua<Top>(base, top_slot);
    const DispatchResult result =
        registered == nullptr
            ? DispatchResult{status_internal_error, 0, 0, return_mode_staged}
            : registered(state, error, error_capacity, &returns);
#ifdef RSGDLL_TEST_SUPPORT
    last_dispatch_status = result.status;
#endif
    const std::int32_t exit_top = call_lua<Top>(base, top_slot);

    const std::int32_t guard_clear_status = execute_guard(state, base, op_guard_clear);
    finish_context(protected_context);
    if (guard_clear_status != 0) {
        return fail_callback(
            base, nullptr, "rsgdll failed to clear callback re-entry guard");
    }

    if (!accepts_dispatch_result(result, entry_top, exit_top)) {
        return fail_callback(
            base, nullptr, "rsgdll dispatcher produced an invalid stack result");
    }

    if (result.status == status_success) {
        if (result.return_mode == return_mode_stack) {
            return result.return_count;
        }
        for (std::int32_t index = 0; index < result.return_count; ++index) {
            const ReturnSlot &value = returns.slots[index];
            switch (value.tag) {
            case return_nil:
                call_lua<PushNil>(base, push_nil_slot);
                break;
            case return_bool:
                call_lua<PushBool>(base, push_bool_slot, value.number != 0.0);
                break;
            case return_number:
                call_lua<PushNumber>(base, push_number_slot, value.number);
                break;
            case return_string: {
                const std::uint64_t end =
                    static_cast<std::uint64_t>(value.offset) + value.length;
                if (end > sizeof(returns.bytes)) {
                    constexpr char invalid[] = "rsgdll produced invalid string return data";
                    call_lua<ThrowError>(base, throw_error_slot, invalid);
                    return 0;
                }
                call_lua<PushString>(
                    base,
                    push_string_slot,
                    value.length == 0
                        ? ""
                        : reinterpret_cast<const char *>(returns.bytes + value.offset),
                    value.length);
                break;
            }
            default: {
                constexpr char invalid[] = "rsgdll produced an invalid return value tag";
                call_lua<ThrowError>(base, throw_error_slot, invalid);
                return 0;
            }
            }
        }
        return result.return_count;
    }

    if (result.error_length == 0) {
        constexpr char fallback[] = "rsgdll dispatcher failed without an error report";
        std::memcpy(error, fallback, sizeof(fallback));
    } else {
        const std::uint32_t end =
            result.error_length < error_capacity ? result.error_length : error_capacity - 1;
        error[end] = '\0';
    }

    raise_bridge_error(base, error);
    return 0;
}

extern "C" int rsgdll_bridge_gmod13_open(LuaState *state, ModuleInitializer initializer) {
    if (initializer == nullptr) {
        return 0;
    }

    ModuleRegistration registrations[registration_capacity]{};
    std::uint32_t count = 0;
    const std::uint8_t *module_name = nullptr;
    std::uint32_t module_name_length = 0;
    const AbiLayout *layout = nullptr;
    char error[error_capacity] = {};
    const std::uint8_t initialized = initializer(
        registrations, registration_capacity, &count, &module_name,
        &module_name_length, &layout, error, error_capacity);
    if (layout == nullptr) {
        return 0;
    }
    abi_layout.store(layout, std::memory_order_release);
    void *base = state_lua_base(state);
    if (base == nullptr) {
        return 0;
    }
    call_lua<SetState>(base, set_state_slot, state);
    if (initialized == 0) {
        raise_bridge_error(base, error[0] == '\0'
            ? "rsgdll module initializer failed without an error report" : error);
        return 0;
    }
    if (count > registration_capacity || module_name == nullptr ||
        module_name_length == 0) {
        raise_bridge_error(base, "rsgdll module initializer returned invalid metadata");
        return 0;
    }
    call_lua<PushSpecial>(base, push_special_slot, special_global);
    call_lua<PushString>(base, push_string_slot,
        reinterpret_cast<const char *>(module_name), module_name_length);
    call_lua<CreateTable>(base, create_table_slot);
    for (std::uint32_t index = 0; index < count; ++index) {
        const ModuleRegistration &registration = registrations[index];
        if (registration.name == nullptr && registration.name_length != 0) {
            return 0;
        }
        call_lua<PushString>(
            base,
            push_string_slot,
            registration.name_length == 0
                ? ""
                : reinterpret_cast<const char *>(registration.name),
            registration.name_length);
        call_lua<PushNumber>(
            base, push_number_slot, static_cast<double>(registration.callback_id));
        call_lua<PushClosure>(base, push_closure_slot, rsgdll_bridge_trampoline, 1);
        call_lua<RawSet>(base, raw_set_slot, -3);
    }
    call_lua<RawSet>(base, raw_set_slot, -3);
    call_lua<Pop>(base, pop_slot, 1);
    return 0;
}

extern "C" int rsgdll_bridge_gmod13_close(LuaState *) {
    // Dynamic unload/reload is unsupported. The host may call this only during
    // Lua-state/process teardown after native callbacks can no longer run.
    return 0;
}

static_assert(sizeof(DispatchResult) == 16);
static_assert(sizeof(ReturnSlot) == 24);
static_assert(sizeof(ReturnBuffer) == 4480);
static_assert(sizeof(ModuleRegistration) == sizeof(void *) + 8);
static_assert(
    sizeof(LuaOperation) ==
    (sizeof(void *) == 4 && alignof(std::int64_t) == 4 ? 48 : 56));
static_assert(sizeof(AbiLayout) == 27 * sizeof(std::size_t));

#ifdef RSGDLL_TEST_SUPPORT
extern "C" bool rsgdll_bridge_test_accepts_dispatch_result(
    DispatchResult result, std::int32_t entry_top, std::int32_t exit_top) {
    return accepts_dispatch_result(result, entry_top, exit_top);
}
#endif
