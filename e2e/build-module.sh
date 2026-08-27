#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
target="${RSGDLL_E2E_TARGET:-x86_64-unknown-linux-gnu}"
stage_root="${1:-$repo_root/e2e/stage}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/e2e/module/target}"
build_image="${RSGDLL_E2E_BUILD_IMAGE:-rust:1.97.1-bookworm}"
crash_test="${RSGDLL_E2E_CRASH_TEST:-0}"

if [[ "$target" != "x86_64-unknown-linux-gnu" ]]; then
    printf 'unsupported E2E target: %s\n' "$target" >&2
    exit 2
fi

mkdir -p "$target_dir"
container_user=(--user "$(id -u):$(id -g)")
if [[ "$(docker --version)" == podman* ]]; then
    container_user=(--userns=keep-id)
fi
docker run --rm \
    "${container_user[@]}" \
    --env CARGO_HOME=/tmp/cargo \
    --env CARGO_TARGET_DIR=/target \
    --env RSGDLL_E2E_CRASH_TEST="$crash_test" \
    --volume "$repo_root:/work:ro" \
    --volume "$target_dir:/target" \
    --workdir /work/e2e/module \
    "$build_image" \
    /bin/bash -c '
        set -euo pipefail
        args=(cargo build --locked --target "'"$target"'")
        if [[ "$RSGDLL_E2E_CRASH_TEST" == 1 ]]; then
            args+=(--features crash-test)
        fi
        "${args[@]}"
        cd /work
        cargo build --locked -p rsgdll-example --target "'"$target"'"
    '

check_exports() {
    local binary="$1"
    local found_open=0
    local found_close=0

    while read -r symbol _; do
        case "$symbol" in
            gmod13_open)
                found_open=1
                ;;
            gmod13_close)
                found_close=1
                ;;
            *)
                printf 'unexpected exported symbol in %s: %s\n' "$binary" "$symbol" >&2
                return 1
                ;;
        esac
    done < <(nm -D --defined-only --format=posix "$binary")

    if [[ "$found_open" != 1 || "$found_close" != 1 ]]; then
        printf 'missing GMod entrypoint export in %s\n' "$binary" >&2
        return 1
    fi
}

check_exports "$target_dir/$target/debug/librsgdll_e2e.so"
check_exports "$target_dir/$target/debug/librsgdll_example.so"

install -Dm755 \
    "$target_dir/$target/debug/librsgdll_e2e.so" \
    "$stage_root/garrysmod/lua/bin/gmsv_rsgdll_e2e_linux64.dll"
install -Dm755 \
    "$target_dir/$target/debug/librsgdll_example.so" \
    "$stage_root/garrysmod/lua/bin/gmsv_rsgdll_example_linux64.dll"
mkdir -p "$stage_root/garrysmod/addons/rsgdll-e2e"
cp -R "$repo_root/e2e/addon/." "$stage_root/garrysmod/addons/rsgdll-e2e/"
