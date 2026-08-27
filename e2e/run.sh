#!/usr/bin/env bash
set -u
set -o pipefail

usage() {
    printf 'Usage: bash e2e/run.sh\n'
}

if [[ "${1:-}" == "--help" ]]; then
    usage
    exit 0
elif [[ "$#" -ne 0 ]]; then
    usage >&2
    exit 2
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
gluatest_ref=735e52d20c69d14c9a2a8d42f3d6428d1af0936b
run_id="${RSGDLL_E2E_RUN_ID:-$$}"
if [[ ! "$run_id" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
    printf 'invalid RSGDLL_E2E_RUN_ID: %s\n' "$run_id" >&2
    exit 2
fi
run_root="${RUNNER_TEMP:-/tmp}/rsgdll-e2e-$run_id"
gluatest_dir="${GLUATEST_DIR:-$run_root/GLuaTest-$gluatest_ref}"
stage_dir="$run_root/stage"
artifact_dir="${E2E_ARTIFACT_DIR:-$repo_root/e2e-artifacts-$run_id}"
gmod_branch="${GMOD_BRANCH:-x86-64}"
build_image="${RSGDLL_E2E_BUILD_IMAGE:-rust:1.97.1-bookworm}"
expected_outcome="${RSGDLL_E2E_EXPECTED_OUTCOME:-PASS}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$run_root/cargo-target}"

mkdir -p "$run_root" "$artifact_dir"
chmod a+rwx "$artifact_dir"
rm -f \
    "$artifact_dir/backtrace.txt" \
    "$artifact_dir/console.log" \
    "$artifact_dir/core" \
    "$artifact_dir/debug.log" \
    "$artifact_dir/gluatest.log" \
    "$artifact_dir/checksums.sha256" \
    "$artifact_dir/close-hook.txt" \
    "$artifact_dir/core-original" \
    "$artifact_dir/core-path.txt" \
    "$artifact_dir/loaded-default-module.dll" \
    "$artifact_dir/loaded-module.dll" \
    "$artifact_dir/metadata.txt" \
    "$artifact_dir/module-load-failure.txt" \
    "$artifact_dir/native-crash-reached.txt" \
    "$artifact_dir/outcome.txt" \
    "$artifact_dir/runtime-provenance.txt" \
    "$artifact_dir/source-files.sha256" \
    "$artifact_dir/source-status.txt" \
    "$artifact_dir/tested-srcds" \
    "$artifact_dir/tested-default-module.dll" \
    "$artifact_dir/tested-module.dll"
rm -rf "$stage_dir"
mkdir -p "$stage_dir/artifacts-input"

if [[ ! -d "$gluatest_dir/.git" ]]; then
    git clone --filter=blob:none https://github.com/CFC-Servers/GLuaTest.git "$gluatest_dir"
    git -C "$gluatest_dir" checkout --detach "$gluatest_ref"
fi

git -C "$repo_root" status --porcelain=v1 --untracked-files=all \
    > "$artifact_dir/source-status.txt"
if [[ -s "$artifact_dir/source-status.txt" ]]; then
    tree_state=dirty
else
    tree_state=clean
fi
(
    cd "$repo_root" || exit 1
    git ls-files --cached --others --exclude-standard -z -- \
        .github Cargo.lock Cargo.toml README.md crates docs e2e examples xtask |
        sort -z |
        while IFS= read -r -d '' path; do
            if [[ -L "$path" ]]; then
                printf '%s  %s\n' "$(readlink "$path" | sha256sum | cut -d' ' -f1)" "$path"
            elif [[ -f "$path" ]]; then
                sha256sum "$path"
            else
                printf 'missing  %s\n' "$path"
            fi
        done
) > "$artifact_dir/source-files.sha256"
source_tree_sha256="$(sha256sum "$artifact_dir/source-files.sha256" | cut -d' ' -f1)"

if [[ "$expected_outcome" == SERVER_CRASH ]]; then
    export RSGDLL_E2E_CRASH_TEST=1
    enabled_features='async, backtrace, engine, serde, crash-test'
else
    export RSGDLL_E2E_CRASH_TEST=0
    enabled_features='async, backtrace, engine, serde'
fi
if ! bash "$repo_root/e2e/build-module.sh" "$stage_dir"; then
    printf 'E2E module build failed\n' >&2
    exit 1
fi
install -m755 \
    "$stage_dir/garrysmod/lua/bin/gmsv_rsgdll_e2e_linux64.dll" \
    "$artifact_dir/tested-module.dll" || exit 1
install -m755 \
    "$stage_dir/garrysmod/lua/bin/gmsv_rsgdll_example_linux64.dll" \
    "$artifact_dir/tested-default-module.dll" || exit 1

{
    printf 'git commit: %s\n' "$(git -C "$repo_root" rev-parse HEAD)"
    printf 'git tree state: %s\n' "$tree_state"
    printf 'source tree sha256: %s\n' "$source_tree_sha256"
    printf 'GMod branch: %s\n' "$gmod_branch"
    printf 'target architecture: x86_64-unknown-linux-gnu\n'
    printf 'enabled rsgdll features: %s\n' "$enabled_features"
    printf 'default consumer rsgdll features: none\n'
    printf 'expected outcome: %s\n' "$expected_outcome"
    printf 'build image: %s\n' "$build_image"
    printf 'tested module sha256: %s\n' \
        "$(sha256sum "$artifact_dir/tested-module.dll" | cut -d' ' -f1)"
    printf 'tested default module sha256: %s\n' \
        "$(sha256sum "$artifact_dir/tested-default-module.dll" | cut -d' ' -f1)"
    docker run --rm --entrypoint /bin/bash "$build_image" \
        -c 'rustc --version --verbose; cargo --version'
} > "$artifact_dir/metadata.txt"

core_pattern=core.%e.%p.%t
if [[ -w /proc/sys/kernel/core_pattern ]]; then
    printf '%s\n' "$core_pattern" > /proc/sys/kernel/core_pattern
elif command -v sudo >/dev/null && sudo -n true 2>/dev/null; then
    printf '%s\n' "$core_pattern" | sudo tee /proc/sys/kernel/core_pattern >/dev/null
else
    printf 'warning: unable to configure kernel.core_pattern\n' >&2
fi

export REQUIREMENTS=/dev/null
export CUSTOM_SERVER_CONFIG=/dev/null
export PROJECT_DIR="$stage_dir/garrysmod"
export GMOD_ARTIFACT_DIR="$stage_dir/artifacts-input"
export GMOD_BRANCH="$gmod_branch"
export GAMEMODE=sandbox
export MAP=gm_construct
export COLLECTION_ID=0
export TIMEOUT="${GLUATEST_TIMEOUT_MINUTES:-5}"
export EXTRA_STARTUP_ARGS=
export SSH_PRIVATE_KEY=
export GITHUB_TOKEN=
export RSGDLL_E2E_ENTRYPOINT="$repo_root/e2e/runner-entrypoint.sh"
export E2E_ARTIFACT_DIR="$artifact_dir"
export RSGDLL_E2E_CONTAINER_NAME="gluatest_runner_$run_id"

compose=(
    docker compose
    --project-name "rsgdll-e2e-$run_id"
    --file "$gluatest_dir/docker/docker-compose.yml"
    --file "$repo_root/e2e/docker-compose.override.yml"
)

docker rm --force "$RSGDLL_E2E_CONTAINER_NAME" >/dev/null 2>&1 || true
"${compose[@]}" pull
pull_status=$?
if [[ "$pull_status" -eq 0 ]]; then
    "${compose[@]}" up --pull never --exit-code-from runner
    compose_status=$?
else
    compose_status=$pull_status
fi

"${compose[@]}" logs --no-color runner \
    > "$artifact_dir/gluatest.log" 2>&1 || true
docker cp \
    "$RSGDLL_E2E_CONTAINER_NAME:/home/steam/gmodserver/garrysmod/lua/bin/gmsv_rsgdll_e2e_linux64.dll" \
    "$artifact_dir/loaded-module.dll" || true
docker cp \
    "$RSGDLL_E2E_CONTAINER_NAME:/home/steam/gmodserver/garrysmod/lua/bin/gmsv_rsgdll_example_linux64.dll" \
    "$artifact_dir/loaded-default-module.dll" || true
docker cp "$RSGDLL_E2E_CONTAINER_NAME:/home/steam/gmodserver/bin/linux64/srcds" \
    "$artifact_dir/tested-srcds" || true
if [[ -s "$artifact_dir/core-path.txt" ]]; then
    core_path="$(<"$artifact_dir/core-path.txt")"
    docker cp "$RSGDLL_E2E_CONTAINER_NAME:$core_path" "$artifact_dir/core-original" || true
fi
"${compose[@]}" down --remove-orphans >/dev/null 2>&1 || true

if [[ ! -f "$artifact_dir/console.log" ]]; then
    printf 'console.log was not produced\n' > "$artifact_dir/console.log"
fi

if [[ -f "$artifact_dir/outcome.txt" ]]; then
    outcome="$(cat "$artifact_dir/outcome.txt")"
elif [[ "$pull_status" -ne 0 ]]; then
    outcome=TEST_FAILURE
elif [[ "$compose_status" -eq 124 ]]; then
    outcome=TIMEOUT
else
    outcome=SERVER_CRASH
fi

printf 'RSGDLL_E2E_OUTCOME=%s\n' "$outcome"
if [[ "$outcome" != "$expected_outcome" ]]; then
    exit 1
fi
for required_artifact in \
    loaded-default-module.dll \
    loaded-module.dll \
    tested-default-module.dll \
    tested-module.dll \
    tested-srcds; do
    if [[ ! -s "$artifact_dir/$required_artifact" ]]; then
        printf 'required provenance artifact missing: %s\n' "$required_artifact" >&2
        exit 1
    fi
done
if [[ "$expected_outcome" == PASS ]]; then
    close_hook_marker=
    if [[ -f "$artifact_dir/close-hook.txt" ]]; then
        close_hook_marker="$(<"$artifact_dir/close-hook.txt")"
    fi
    if [[ "$close_hook_marker" != "rsgdll-e2e-close-v1" ]]; then
        printf 'gmod13_close hook marker missing or invalid\n' >&2
        exit 1
    fi
fi
{
    printf 'loaded module sha256: %s\n' \
        "$(sha256sum "$artifact_dir/loaded-module.dll" | cut -d' ' -f1)"
    printf 'loaded default module sha256: %s\n' \
        "$(sha256sum "$artifact_dir/loaded-default-module.dll" | cut -d' ' -f1)"
    printf 'srcds sha256: %s\n' \
        "$(sha256sum "$artifact_dir/tested-srcds" | cut -d' ' -f1)"
    if [[ -f "$artifact_dir/core-original" ]]; then
        printf 'core sha256: %s\n' \
            "$(sha256sum "$artifact_dir/core-original" | cut -d' ' -f1)"
    fi
} > "$artifact_dir/runtime-provenance.txt"
tested_module_sha256="$(sha256sum "$artifact_dir/tested-module.dll" | cut -d' ' -f1)"
tested_default_sha256="$(sha256sum "$artifact_dir/tested-default-module.dll" | cut -d' ' -f1)"
loaded_module_sha256="$(grep '^loaded module sha256: ' "$artifact_dir/runtime-provenance.txt" | cut -d' ' -f4)"
loaded_default_sha256="$(grep '^loaded default module sha256: ' "$artifact_dir/runtime-provenance.txt" | cut -d' ' -f5)"
if [[ "$loaded_module_sha256" != "$tested_module_sha256" ||
      "$loaded_default_sha256" != "$tested_default_sha256" ]]; then
    printf 'runtime module hashes do not match tested binaries\n' >&2
    exit 1
fi
if [[ "$expected_outcome" == SERVER_CRASH ]]; then
    if [[ ! -s "$artifact_dir/core" || ! -s "$artifact_dir/backtrace.txt" ]]; then
        printf 'native crash did not preserve both core and backtrace\n' >&2
        exit 1
    fi
    if [[ ! -f "$artifact_dir/native-crash-reached.txt" ]]; then
        printf 'native crash test case was not reached\n' >&2
        exit 1
    fi
    native_crash_marker=$(<"$artifact_dir/native-crash-reached.txt")
    if [[ "$native_crash_marker" != "rsgdll-e2e-native-crash-v1" ]]; then
        printf 'native crash test marker was invalid\n' >&2
        exit 1
    fi
    if ! grep --fixed-strings --quiet \
        'Program terminated with signal SIGABRT' \
        "$artifact_dir/backtrace.txt"; then
        printf 'native crash core was not terminated by SIGABRT\n' >&2
        exit 1
    fi
    if ! grep --fixed-strings --quiet \
        ' in rsgdll_e2e::__rsgdll_impl_native_crash (' \
        "$artifact_dir/backtrace.txt"; then
        printf 'native crash backtrace does not contain the intentional crash frame\n' >&2
        exit 1
    fi
    copied_core_sha256="$(sha256sum "$artifact_dir/core" | cut -d' ' -f1)"
    original_core_sha256="$(sha256sum "$artifact_dir/core-original" | cut -d' ' -f1)"
    if [[ "$copied_core_sha256" != "$original_core_sha256" ]]; then
        printf 'copied core hash does not match captured core\n' >&2
        exit 1
    fi
    printf 'core sha256: %s\n' "$copied_core_sha256" >> "$artifact_dir/metadata.txt"
fi
if ! (
    cd "$artifact_dir" || exit 1
    checksum_files=(
        metadata.txt
        runtime-provenance.txt
        source-files.sha256
        source-status.txt
        loaded-default-module.dll
        loaded-module.dll
        tested-default-module.dll
        tested-module.dll
        tested-srcds
    )
    if [[ -f core ]]; then
        checksum_files+=(core core-original core-path.txt)
    fi
    if [[ -f close-hook.txt ]]; then
        checksum_files+=(close-hook.txt)
    fi
    sha256sum "${checksum_files[@]}" > checksums.sha256
    sha256sum --check checksums.sha256
); then
    printf 'artifact checksum validation failed\n' >&2
    exit 1
fi
if [[ "$outcome" == "$expected_outcome" ]]; then
    exit 0
fi
exit 1
