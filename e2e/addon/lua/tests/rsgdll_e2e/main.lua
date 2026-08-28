local loaded, loadError = pcall(require, "rsgdll_e2e")
local module = _G.rsgdll_e2e
local defaultLoaded, defaultLoadError = pcall(require, "rsgdll_example")
local defaultModule = _G.rsgdll_example

if not loaded or not istable(module) then
    local message = loaded and "require succeeded but module global is missing" or tostring(loadError)
    file.CreateDir("rsgdll_e2e")
    file.Write("rsgdll_e2e/module_load_failure.txt", message)
    print("[rsgdll-e2e] MODULE_LOAD_FAILURE: " .. message)
end

if not defaultLoaded or not istable(defaultModule) then
    local message = defaultLoaded and "default module global is missing" or tostring(defaultLoadError)
    file.CreateDir("rsgdll_e2e")
    file.Write("rsgdll_e2e/module_load_failure.txt", message)
    print("[rsgdll-e2e] MODULE_LOAD_FAILURE: " .. message)
end

local function callError(name)
    local ok, message = pcall(module[name])

    return ok, tostring(message)
end

return {
    groupName = "rsgdll real module",
    cases = {
        {
            name = "require loads the binary module",
            func = function()
                expect(loaded).to.beTrue()
                expect(module).to.beA("table")
            end
        },
        {
            name = "loads a default-feature facade consumer",
            func = function()
                expect(defaultLoaded).to.beTrue()
                expect(defaultModule).to.beA("table")
                expect(defaultModule.hello("GMod")).to.equal("Hello GMod")

                local ok, message = pcall(defaultModule.get_user, 0)
                expect(ok).to.beFalse()
                expect(string.find(tostring(message), "user id must not be zero", 1, true)).to.exist()
            end
        },
        {
            name = "calls a plain Rust function",
            func = function()
                expect(module.plain()).to.equal("plain Rust call")
            end
        },
        {
            name = "attaches to the active Source engine",
            func = function()
                expect(module.engine_is_dedicated).to.beA("function")
                expect(module.engine_is_dedicated()).to.beTrue()
            end
        },
        {
            name = "converts primitive arguments",
            func = function()
                expect(module.add(20, 22)).to.equal(42)
            end
        },
        {
            name = "round trips every integer type",
            func = function()
                local u8Value, u16Value, u32Value, u64Value =
                    module.unsigned_integer_round_trip(255, 65535, 4294967295, 9007199254740992)
                local i8Value, i16Value, i32Value, i64Value =
                    module.signed_integer_round_trip(-128, -32768, -2147483648, -9007199254740992)

                expect(u8Value).to.equal(255)
                expect(u16Value).to.equal(65535)
                expect(u32Value).to.equal(4294967295)
                expect(u64Value).to.equal(9007199254740992)
                expect(i8Value).to.equal(-128)
                expect(i16Value).to.equal(-32768)
                expect(i32Value).to.equal(-2147483648)
                expect(i64Value).to.equal(-9007199254740992)
            end
        },
        {
            name = "converts primitive returns",
            func = function()
                local text, number, flag = module.primitives()

                expect(text).to.equal("converted")
                expect(number).to.equal(7)
                expect(flag).to.beTrue()
            end
        },
        {
            name = "creates and reads Lua tables without raw access",
            func = function()
                local value = module.make_table()

                expect(value.answer).to.equal(42)
                expect(value.label).to.equal("from Rust")
                expect(module.table_answer({ answer = 17 })).to.equal(17)
            end
        },
        {
            name = "continues after a caught Rust stack mutation error",
            func = function()
                expect(module.recover_table_set()).to.equal(42)
            end
        },
        {
            name = "exports callable Rust functions",
            func = function()
                local plusOne = module.export_plus_one()

                expect(isfunction(plusOne)).to.beTrue()
                expect(plusOne(8)).to.equal(9)
            end
        },
        {
            name = "protects Rust to Lua calls",
            func = function()
                expect(module.call_once(function(value)
                    return value * 2
                end, 21)).to.equal(42)

                local ok, message = pcall(module.call_once, function()
                    error("protected Lua failure")
                end, 21)

                expect(ok).to.beFalse()
                expect(string.find(tostring(message), "protected Lua failure", 1, true)).to.exist()
                expect(module.plain()).to.equal("plain Rust call")
            end
        },
        {
            name = "handles multiple protected and complex returns",
            func = function()
                local text, number, flag = module.call_multi(function()
                    return "one", 2, true
                end)

                expect(text).to.equal("one")
                expect(number).to.equal(2)
                expect(flag).to.beTrue()

                local value, sibling = module.table_and_value()
                expect(value.kind).to.equal("complex")
                expect(sibling).to.equal(9)
            end
        },
        {
            name = "round trips registry references with identity",
            func = function()
                local value = { identity = true }
                local returned = module.registry_roundtrip(value)

                expect(rawequal(value, returned)).to.beTrue()
            end
        },
        {
            name = "supports typed userdata methods and garbage collection",
            func = function()
                local dropsBefore = module.counter_drops()
                local counter = module.new_counter(4)

                expect(type(counter)).to.equal("rsgdll_e2e.Counter")
                expect(counter:value()).to.equal(4)
                expect(counter:add(3)).to.equal(7)
                expect(counter:value()).to.equal(7)

                local ok = pcall(counter.value, {})
                expect(ok).to.beFalse()

                counter = nil
                collectgarbage("collect")
                collectgarbage("collect")
                expect(module.counter_drops()).to.equal(dropsBefore + 1)
            end
        },
        {
            name = "preserves binary strings exactly",
            func = function()
                local bytes = string.char(0, 255, 65, 0)
                local echoed = module.binary_echo(bytes)

                expect(#echoed).to.equal(4)
                expect(string.byte(echoed, 1)).to.equal(0)
                expect(string.byte(echoed, 2)).to.equal(255)
                expect(string.byte(echoed, 3)).to.equal(65)
                expect(string.byte(echoed, 4)).to.equal(0)
            end
        },
        {
            name = "round trips serde structures",
            func = function()
                local value = module.serde_round_trip({
                    name = "Ada",
                    enabled = true,
                    scores = { 2, 7 }
                })

                expect(value.name).to.equal("Ada")
                expect(value.enabled).to.beTrue()
                expect(value.scores[1]).to.equal(2)
                expect(value.scores[2]).to.equal(7)

                local ok = pcall(module.serde_round_trip, {
                    name = 7,
                    enabled = "wrong",
                    scores = "not a sequence"
                })
                expect(ok).to.beFalse()

                ok = pcall(module.serde_round_trip, {
                    name = "sparse",
                    enabled = true,
                    scores = { [1000000000] = 1 }
                })
                expect(ok).to.beFalse()
                expect(module.plain()).to.equal("plain Rust call")
            end
        },
        {
            name = "completes background work on the GMod main thread",
            func = function()
                local completionRan = false
                module.start_background(41)

                local result = module.complete_background(function(value)
                    completionRan = true
                    return value
                end)

                expect(completionRan).to.beTrue()
                expect(result).to.equal(42)
            end
        },
        {
            name = "installs inspects and restores a real Lua debug hook",
            func = function()
                local previousEvents = 0
                local rsgdebug_upvalue_probe = 23
                local function probe()
                    local rsgdebug_local_probe = 19
                    return rsgdebug_local_probe + rsgdebug_upvalue_probe
                end

                debug.sethook(function()
                    previousEvents = previousEvents + 1
                end, "l")
                expect(module.debug_attach()).to.beTrue()
                expect(probe()).to.equal(42)

                local events, localValue, upvalueValue = module.debug_observation()
                expect(events > 0).to.beTrue()
                expect(localValue).to.equal(19)
                expect(upvalueValue).to.equal(23)
                expect(module.debug_detach()).to.beTrue()

                local before = previousEvents
                probe()
                debug.sethook()
                expect(previousEvents > before).to.beTrue()
            end
        },
        {
            name = "returns Result Ok values",
            func = function()
                expect(module.result_ok()).to.equal("ok")
            end
        },
        {
            name = "turns Result Err into a Lua error",
            func = function()
                local ok = pcall(module.result_err)

                expect(ok).to.beFalse()
            end
        },
        {
            name = "lets Lua pcall catch Rust errors",
            func = function()
                local ok, message = callError("result_err")

                expect(ok).to.beFalse()
                expect(message).to.beA("string")
            end
        },
        {
            name = "uses thiserror Display text",
            func = function()
                local _, message = callError("result_err")

                expect(string.find(message, "outer E2E failure", 1, true)).to.exist()
            end
        },
        {
            name = "includes the Rust error source chain",
            func = function()
                local _, message = callError("result_err")

                expect(string.find(message, "caused by: inner E2E cause", 1, true)).to.exist()
            end
        },
        {
            name = "catches Rust panics at the boundary",
            func = function()
                local ok, message = callError("panic_now")

                expect(ok).to.beFalse()
                expect(string.find(message, "panic in", 1, true)).to.exist()
                expect(string.find(message, "intentional E2E panic", 1, true)).to.exist()
            end
        },
        {
            name = "keeps the server alive after recoverable failures",
            func = function()
                pcall(module.result_err)
                pcall(module.panic_now)

                expect(module.plain()).to.equal("plain Rust call")
            end
        },
        {
            name = "restores the stack after reserve exhaustion",
            func = function()
                local ok = pcall(module.overflow_stack)

                expect(ok).to.beFalse()
                expect(module.plain()).to.equal("plain Rust call")
            end
        },
        {
            name = "finalizes userdata during a protected Lua call",
            func = function()
                local dropsBefore = module.counter_drops()
                local counter = module.new_counter(4)

                local returned = module.call_once(function(value)
                    getmetatable(counter).__gc(counter)
                    return value
                end, 9)

                expect(returned).to.equal(9)
                expect(module.counter_drops()).to.equal(dropsBefore + 1)
                expect(pcall(counter.value, counter)).to.beFalse()
            end
        },
        {
            name = "rejects cross-module Rust reentry",
            func = function()
                local ok, message = pcall(module.call_once, function()
                    return defaultModule.get_user(7)
                end, 0)

                expect(ok).to.beFalse()
                expect(string.find(tostring(message), "rsgdll callback re-entry", 1, true)).to.exist()
                expect(module.plain()).to.equal("plain Rust call")
                expect(defaultModule.get_user(7)).to.equal("user-7")
            end
        },
        {
            name = "registry strings cannot disable cross-module reentry guard",
            func = function()
                local ok, message = pcall(module.call_once, function()
                    debug.getregistry()["rsgdll.__private.rust_frame_active.v1"] = nil
                    return defaultModule.get_user(7)
                end, 0)

                expect(ok).to.beFalse()
                expect(string.find(tostring(message), "rsgdll callback re-entry", 1, true)).to.exist()
                expect(module.plain()).to.equal("plain Rust call")
            end
        },
        {
            name = "preserves diagnostics for a genuine native crash",
            func = function()
                if not isfunction(module.native_crash) then
                    return
                end

                file.CreateDir("rsgdll_e2e")
                file.Write(
                    "rsgdll_e2e/native_crash_reached.txt",
                    "rsgdll-e2e-native-crash-v1"
                )
                module.native_crash()
                error("native crash function returned")
            end
        }
    }
}
