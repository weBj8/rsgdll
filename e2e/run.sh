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
run_root="${RUNNER_TEMP:-/tmp}/rsgdll-e2e"
gluatest_dir="${GLUATEST_DIR:-$run_root/GLuaTest-$gluatest_ref}"
stage_dir="$run_root/stage"
artifact_dir="${E2E_ARTIFACT_DIR:-$repo_root/e2e-artifacts}"
gmod_branch="${GMOD_BRANCH:-x86-64}"
build_image="${RSGDLL_E2E_BUILD_IMAGE:-rust:1.97.1-bookworm}"

mkdir -p "$run_root" "$artifact_dir"
chmod a+rwx "$artifact_dir"
rm -f \
    "$artifact_dir/backtrace.txt" \
    "$artifact_dir/console.log" \
    "$artifact_dir/core" \
    "$artifact_dir/debug.log" \
    "$artifact_dir/gluatest.log" \
    "$artifact_dir/module-load-failure.txt" \
    "$artifact_dir/outcome.txt" \
    "$artifact_dir/tested-module.dll"
rm -rf "$stage_dir"
mkdir -p "$stage_dir/artifacts-input"

if [[ ! -d "$gluatest_dir/.git" ]]; then
    git clone --filter=blob:none https://github.com/CFC-Servers/GLuaTest.git "$gluatest_dir"
    git -C "$gluatest_dir" checkout --detach "$gluatest_ref"
fi

bash "$repo_root/e2e/build-module.sh" "$stage_dir"
install -m755 \
    "$stage_dir/garrysmod/lua/bin/gmsv_rsgdll_e2e_linux64.dll" \
    "$artifact_dir/tested-module.dll"

{
    printf 'git commit: %s\n' "$(git -C "$repo_root" rev-parse HEAD)"
    printf 'GMod branch: %s\n' "$gmod_branch"
    printf 'target architecture: x86_64-unknown-linux-gnu\n'
    printf 'enabled rsgdll features: default (none)\n'
    printf 'build image: %s\n' "$build_image"
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

compose=(
    docker compose
    --file "$gluatest_dir/docker/docker-compose.yml"
    --file "$repo_root/e2e/docker-compose.override.yml"
)

docker rm --force gluatest_runner >/dev/null 2>&1 || true
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
docker rm --force gluatest_runner >/dev/null 2>&1 || true

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
if [[ "$outcome" == PASS ]]; then
    exit 0
fi
exit 1
