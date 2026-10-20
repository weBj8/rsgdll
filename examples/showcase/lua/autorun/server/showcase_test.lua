-- rsgdll showcase integration tests (spec §13.2). Phase 1 runs on the first map, then
-- `changelevel` reloads the state; phase 2 checks the reload and prints RSGDLL_TEST_DONE.
local PHASE_FILE = "rsgdll_showcase_phase.txt"
local function report(name, ok, detail)
    print(string.format("RSGDLL_TEST %s %s %s", name, ok and "PASS" or "FAIL", tostring(detail or ""):gsub("\n", " | ")))
end
local function check(name, f)
    local ok, err = pcall(f)
    report(name, ok and err ~= false, ok and (err == false and "returned false" or "") or err)
end
local function later(seconds, f) timer.Simple(seconds, f) end

-- Empty servers hibernate (no Think/timers, spec §10.2); keep ticking for the tests.
RunConsoleCommand("sv_hibernate_think", "1")
local phase2 = file.Exists(PHASE_FILE, "DATA")
require("showcase")

if phase2 then
    file.Delete(PHASE_FILE)
    check("reload_required_again", function() return showcase.Add(1, 1) == 2 end)
    check("reload_lifecycle", function()
        local l = table.concat(showcase.Lifecycle(), ",")
        assert(l:find("preclose,postclose,preopen,postopen", 1, true), l)
    end)
    check("reload_no_leftover_hooks", function()
        local n = 0
        for id in pairs(hook.GetTable().Think or {}) do if tostring(id):find("rsgdll.showcase", 1, true) then n = n + 1 end end
        assert(n == 2, "Think hooks: " .. n) -- pump + #[hook(Think)] of the new state only
    end)
    check("async_state_closed_after_close", function() return showcase.StaleHandle() == "closed" end)
    print("RSGDLL_TEST_DONE showcase")
    timer.Simple(0.5, function() RunConsoleCommand("quit") end)
    return
end

-- Lifecycle
check("lifecycle_open_order", function() return table.concat(showcase.Lifecycle(), ",") == "preopen,postopen" end)
-- Exports
check("export_default", function() return showcase.Add(2, 3) == 5 end)
check("export_global", function() return ShowcaseGlobal() == "global" end)
check("export_nested", function() return showcase.math.Clamp(5, 0, 3) == 3 end)
check("export_realm_server", function() return showcase.ServerOnly() == true end)
check("export_varargs", function() return showcase.Sum(1, 2, 3, 4) == 10 end)
check("export_optional", function() return showcase.GreetOpt() == "hi anon" and showcase.GreetOpt("x") == "hi x" end)
check("export_multireturn", function() local q, r = showcase.DivMod(7, 2) return q == 3 and r == 1 end)
-- Conversion
check("conv_derive_struct", function()
    local c = showcase.EchoConfig({ Name = "n", Tags = { "a", "b" } })
    return c.Name == "n" and c.Retries == 1 and #c.Tags == 2
end)
check("conv_derive_enum", function() local a = showcase.EchoAction({ kind = "Ban", minutes = 5 }) return a.kind == "Ban" and a.minutes == 5 end)
check("conv_serde", function() local p = showcase.SerdePoint({ x = 1, y = 2 }) return p.x == 2 and p.y == 1 end)
check("conv_vector", function() local v = showcase.VecAdd(Vector(1, 2, 3), Vector(1, 1, 1)) return v == Vector(2, 3, 4) end)
check("conv_angle", function() return showcase.AngFlip(Angle(1, 2, 3)) == Angle(-1, -2, -3) end)
check("conv_color", function() local c = showcase.Red() return IsColor(c) and c.r == 255 and c.g == 0 end)
check("conv_entity", function()
    local e = ents.Create("info_target")
    local ok = showcase.EntClass(e) == "info_target"
    e:Remove()
    return ok
end)
check("conv_null_player", function() return showcase.MaybePlayer(NULL) == false end)
check("conv_i64_overflow", function() local ok, e = pcall(showcase.TooBig) return not ok and e:find("2^53", 1, true) ~= nil end)
check("conv_error_path", function()
    local ok, e = pcall(showcase.EchoConfig, { Name = "n", Tags = { "a", {} } })
    assert(not ok and e:find("bad argument #1", 1, true) and e:find("Tags", 1, true), e)
end)
-- Errors
check("error_backtrace", function()
    local ok, e = pcall(showcase.DivMod, 1, 0)
    assert(not ok and e:find("division by zero", 1, true) and e:find("rust stack traceback", 1, true), e)
end)
check("error_panic_survives", function()
    local ok, e = pcall(showcase.Boom)
    assert(not ok and e:find("rust panic", 1, true) and e:find("showcase panic", 1, true), e)
    return showcase.Add(1, 1) == 2
end)
check("error_lua_in_lua_macro", function()
    local t = showcase.LuaErrorText()
    assert(t:find("inner", 1, true) and t:find("stack traceback", 1, true), t)
end)
-- Userdata
check("userdata_methods", function()
    local c = showcase.NewCounter()
    c:Inc() c:Inc(4)
    return tostring(c) == "Counter(5)"
end)
-- lua!
check("lua_macro_forms", function() local n, s, k = showcase.LuaForms(5) return n == 6 and s == "x5" and k == 12 end)
-- Interfaces
check("interfaces", function()
    local srv, cross, path = showcase.Interfaces()
    assert(srv and cross and SHOWCASE_RUNSTRING == 42, tostring(path))
end)
-- hook / timer
check("hook_typed_run_remove", function() return showcase.HooksAndTimers() == "pong" and hook.GetTable().PlayerSay["showcase.say"] == nil end)
check("hook_custom_attr", function() hook.Run("Showcase.Event", 9) return SHOWCASE_EVENT == 9 end)
-- Async
showcase.ThreadCall()
showcase.LocalSleep()
showcase.Fetch(4, function(err, v) SHOWCASE_FETCH = tostring(err) .. ":" .. tostring(v) end)
check("coroutine_vector", function()
    local co = coroutine.create(function() return showcase.VecAdd(Vector(1, 0, 0), Vector(0, 1, 0)), showcase.AngFlip(Angle(1, 1, 1)) end)
    local ok, v, a = coroutine.resume(co)
    assert(ok, v)
    return v == Vector(1, 1, 0) and a == Angle(-1, -1, -1)
end)

later(2, function()
    local thinks, ticks = showcase.Counts()
    check("hook_think_attr", function() return thinks > 0 end)
    check("timer_attr", function() return ticks > 0 end)
    check("timer_simple", function() return SHOWCASE_SIMPLE == true end)
    check("timer_repeat_adjust", function() return SHOWCASE_REP == 2 end)
    check("async_handle_call_thread", function() return SHOWCASE_THREAD == "ok" end)
    check("async_spawn_local_sleep_worker", function() return SHOWCASE_LOCAL == 42 end)
    check("async_export_callback", function() return SHOWCASE_FETCH == "nil:v8" end)
    file.Write(PHASE_FILE, "2")
    RunConsoleCommand("changelevel", game.GetMap())
end)
