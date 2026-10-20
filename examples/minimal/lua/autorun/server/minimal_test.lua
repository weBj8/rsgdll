-- rsgdll minimal integration test (spec §13.1). Prints RSGDLL_TEST lines for xtask.
local function report(name, ok, detail)
    print(string.format("RSGDLL_TEST %s %s %s", name, ok and "PASS" or "FAIL", tostring(detail or "")))
end
local function check(name, f)
    local ok, err = pcall(f)
    report(name, ok and err ~= false, ok and "" or err)
end

check("require", function() require("minimal") return minimal ~= nil end)
check("add", function() return minimal.Add(1, 2) == 3 end)
check("divide_ok", function() return minimal.Divide(4, 2) == 2 end)
check("divide_err", function()
    local ok, e = pcall(minimal.Divide, 1, 0)
    assert(not ok, "expected error")
    assert(e:find("division by zero", 1, true), e)
    assert(e:find("rust stack traceback", 1, true), e)
end)
check("bad_argument", function()
    local ok, e = pcall(minimal.Add, "x")
    assert(not ok and e:find("bad argument #1", 1, true), tostring(e))
end)
check("greet", function() return type(minimal.Greet("a")) == "string" end)
print("RSGDLL_TEST_DONE minimal")
