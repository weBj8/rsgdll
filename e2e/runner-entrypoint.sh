#!/usr/bin/env bash
set -u
shopt -s globstar nullglob

gmod_root=/home/steam/gmodserver
server="$gmod_root/garrysmod"
artifacts=/e2e-artifacts

: > "$artifacts/backtrace.txt"

for stale_core in "$gmod_root"/**/core "$gmod_root"/**/core.*; do
    rm -f "$stale_core"
done

if ! ulimit -c unlimited; then
    printf 'failed to set RLIMIT_CORE=unlimited\n' >> "$artifacts/backtrace.txt"
fi

"$gmod_root/entrypoint.sh"
server_status=$?

copy_first() {
    local destination="$1"
    local source
    for source in "$gmod_root"/**/"$destination"; do
        if [[ -f "$source" ]]; then
            cp "$source" "$artifacts/$destination"
            return
        fi
    done
}

copy_first console.log
copy_first debug.log

core_file=
for candidate in "$gmod_root"/**/core "$gmod_root"/**/core.*; do
    if [[ -f "$candidate" && ( -z "$core_file" || "$candidate" -nt "$core_file" ) ]]; then
        core_file="$candidate"
    fi
done
{
    if [[ -n "$core_file" ]]; then
        printf '%s\n' "$core_file"
    fi
} > "$artifacts/core-path.txt"
if [[ -n "$core_file" ]]; then
    cp --no-preserve=mode "$core_file" "$artifacts/core"
    server_binary="$gmod_root/bin/linux64/srcds"
    if [[ -x "$server_binary" ]]; then
        gdb --batch \
            -ex "set pagination off" \
            -ex "thread apply all bt full" \
            "$server_binary" \
            "$core_file" > "$artifacts/backtrace.txt" 2>&1 || true
    else
        printf 'srcds_linux64 not found; gdb backtrace unavailable\n' \
            >> "$artifacts/backtrace.txt"
    fi
elif [[ ! -s "$artifacts/backtrace.txt" ]]; then
    printf 'no core file produced\n' > "$artifacts/backtrace.txt"
fi

module_path="$server/lua/bin/gmsv_rsgdll_e2e_linux64.dll"
if [[ -f "$module_path" && ! -f "$artifacts/tested-module.dll" ]]; then
    cp "$module_path" "$artifacts/tested-module.dll"
fi
default_module_path="$server/lua/bin/gmsv_rsgdll_example_linux64.dll"
if [[ -f "$default_module_path" &&
    ! -f "$artifacts/tested-default-module.dll" ]]; then
    cp "$default_module_path" "$artifacts/tested-default-module.dll"
fi

module_failure="$server/data/rsgdll_e2e/module_load_failure.txt"
if [[ -f "$module_failure" ]]; then
    cp "$module_failure" "$artifacts/module-load-failure.txt"
fi
native_crash_sentinel="$server/data/rsgdll_e2e/native_crash_reached.txt"
if [[ -f "$native_crash_sentinel" ]]; then
    cp "$native_crash_sentinel" "$artifacts/native-crash-reached.txt"
fi
clean_exit="$server/data/gluatest_clean_exit.txt"
failures="$server/data/gluatest_failures.json"

if [[ -n "$core_file" || -f "$artifacts/debug.log" || "$server_status" -gt 128 ]]; then
    outcome=SERVER_CRASH
elif [[ "$server_status" -eq 124 ]]; then
    outcome=TIMEOUT
elif [[ -f "$module_failure" ]]; then
    outcome=MODULE_LOAD_FAILURE
elif [[ -s "$failures" ]]; then
    outcome=TEST_FAILURE
elif [[ "$server_status" -eq 0 && -f "$clean_exit" ]] &&
    [[ "$(cat "$clean_exit")" == "true" ]]; then
    outcome=PASS
else
    outcome=TEST_FAILURE
fi

printf '%s\n' "$outcome" > "$artifacts/outcome.txt"
exit "$server_status"
