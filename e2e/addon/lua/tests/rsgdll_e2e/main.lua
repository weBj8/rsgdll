local loaded, loadError = pcall( require, "rsgdll_e2e" )
local module = _G.rsgdll_e2e

if not loaded or not istable( module ) then
    local message = loaded and "require succeeded but module global is missing" or tostring( loadError )
    file.CreateDir( "rsgdll_e2e" )
    file.Write( "rsgdll_e2e/module_load_failure.txt", message )
    print( "[rsgdll-e2e] MODULE_LOAD_FAILURE: " .. message )
end

local function callError( name )
    local ok, message = pcall( module[name] )

    return ok, tostring( message )
end

return {
    groupName = "rsgdll real module",
    cases = {
        {
            name = "require loads the binary module",
            func = function()
                expect( loaded ).to.beTrue()
                expect( module ).to.beA( "table" )
            end
        },
        {
            name = "calls a plain Rust function",
            func = function()
                expect( module.plain() ).to.equal( "plain Rust call" )
            end
        },
        {
            name = "converts primitive arguments",
            func = function()
                expect( module.add( 20, 22 ) ).to.equal( 42 )
            end
        },
        {
            name = "converts primitive returns",
            func = function()
                local text, number, flag = module.primitives()

                expect( text ).to.equal( "converted" )
                expect( number ).to.equal( 7 )
                expect( flag ).to.beTrue()
            end
        },
        {
            name = "returns Result Ok values",
            func = function()
                expect( module.result_ok() ).to.equal( "ok" )
            end
        },
        {
            name = "turns Result Err into a Lua error",
            func = function()
                local ok = pcall( module.result_err )

                expect( ok ).to.beFalse()
            end
        },
        {
            name = "lets Lua pcall catch Rust errors",
            func = function()
                local ok, message = callError( "result_err" )

                expect( ok ).to.beFalse()
                expect( message ).to.beA( "string" )
            end
        },
        {
            name = "uses thiserror Display text",
            func = function()
                local _, message = callError( "result_err" )

                expect( string.find( message, "outer E2E failure", 1, true ) ).to.exist()
            end
        },
        {
            name = "includes the Rust error source chain",
            func = function()
                local _, message = callError( "result_err" )

                expect( string.find( message, "caused by: inner E2E cause", 1, true ) ).to.exist()
            end
        },
        {
            name = "catches Rust panics at the boundary",
            func = function()
                local ok, message = callError( "panic_now" )

                expect( ok ).to.beFalse()
                expect( string.find( message, "panic in", 1, true ) ).to.exist()
                expect( string.find( message, "intentional E2E panic", 1, true ) ).to.exist()
            end
        },
        {
            name = "keeps the server alive after recoverable failures",
            func = function()
                pcall( module.result_err )
                pcall( module.panic_now )

                expect( module.plain() ).to.equal( "plain Rust call" )
            end
        }
    }
}
