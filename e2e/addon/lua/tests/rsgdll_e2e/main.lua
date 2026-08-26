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
            name = "creates and reads Lua tables without raw access",
            func = function()
                local value = module.make_table()

                expect( value.answer ).to.equal( 42 )
                expect( value.label ).to.equal( "from Rust" )
                expect( module.table_answer( { answer = 17 } ) ).to.equal( 17 )
            end
        },
        {
            name = "exports callable Rust functions",
            func = function()
                local plusOne = module.export_plus_one()

                expect( isfunction( plusOne ) ).to.beTrue()
                expect( plusOne( 8 ) ).to.equal( 9 )
            end
        },
        {
            name = "protects Rust to Lua calls",
            func = function()
                expect( module.call_once( function( value )
                    return value * 2
                end, 21 ) ).to.equal( 42 )

                local ok, message = pcall( module.call_once, function()
                    error( "protected Lua failure" )
                end, 21 )

                expect( ok ).to.beFalse()
                expect( string.find( tostring( message ), "protected Lua failure", 1, true ) ).to.exist()
                expect( module.plain() ).to.equal( "plain Rust call" )
            end
        },
        {
            name = "handles multiple protected and complex returns",
            func = function()
                local text, number, flag = module.call_multi( function()
                    return "one", 2, true
                end )

                expect( text ).to.equal( "one" )
                expect( number ).to.equal( 2 )
                expect( flag ).to.beTrue()

                local value, sibling = module.table_and_value()
                expect( value.kind ).to.equal( "complex" )
                expect( sibling ).to.equal( 9 )
            end
        },
        {
            name = "round trips registry references with identity",
            func = function()
                local value = { identity = true }
                local returned = module.registry_roundtrip( value )

                expect( rawequal( value, returned ) ).to.beTrue()
            end
        },
        {
            name = "supports typed userdata methods and garbage collection",
            func = function()
                local dropsBefore = module.counter_drops()
                local counter = module.new_counter( 4 )

                expect( type( counter ) ).to.equal( "rsgdll_e2e.Counter" )
                expect( counter:value() ).to.equal( 4 )
                expect( counter:add( 3 ) ).to.equal( 7 )
                expect( counter:value() ).to.equal( 7 )

                local ok = pcall( counter.value, {} )
                expect( ok ).to.beFalse()

                counter = nil
                collectgarbage( "collect" )
                collectgarbage( "collect" )
                expect( module.counter_drops() ).to.equal( dropsBefore + 1 )
            end
        },
        {
            name = "preserves binary strings exactly",
            func = function()
                local bytes = string.char( 0, 255, 65, 0 )
                local echoed = module.binary_echo( bytes )

                expect( #echoed ).to.equal( 4 )
                expect( string.byte( echoed, 1 ) ).to.equal( 0 )
                expect( string.byte( echoed, 2 ) ).to.equal( 255 )
                expect( string.byte( echoed, 3 ) ).to.equal( 65 )
                expect( string.byte( echoed, 4 ) ).to.equal( 0 )
            end
        },
        {
            name = "round trips serde structures",
            func = function()
                local value = module.serde_round_trip( {
                    name = "Ada",
                    enabled = true,
                    scores = { 2, 7 }
                } )

                expect( value.name ).to.equal( "Ada" )
                expect( value.enabled ).to.beTrue()
                expect( value.scores[1] ).to.equal( 2 )
                expect( value.scores[2] ).to.equal( 7 )

                local ok = pcall( module.serde_round_trip, {
                    name = 7,
                    enabled = "wrong",
                    scores = "not a sequence"
                } )
                expect( ok ).to.beFalse()
            end
        },
        {
            name = "completes background work on the GMod main thread",
            func = function()
                local completionRan = false
                module.start_background( 41 )

                local result = module.complete_background( function( value )
                    completionRan = true
                    return value
                end )

                expect( completionRan ).to.beTrue()
                expect( result ).to.equal( 42 )
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
