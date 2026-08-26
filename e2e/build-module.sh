#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
target="${RSGDLL_E2E_TARGET:-x86_64-unknown-linux-gnu}"
stage_root="${1:-$repo_root/e2e/stage}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/e2e/module/target}"
build_image="${RSGDLL_E2E_BUILD_IMAGE:-rust:1.97.1-bookworm}"

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
    --volume "$repo_root:/work:ro" \
    --volume "$target_dir:/target" \
    --workdir /work/e2e/module \
    "$build_image" \
    cargo build --locked --target "$target"

install -Dm755 \
    "$target_dir/$target/debug/librsgdll_e2e.so" \
    "$stage_root/garrysmod/lua/bin/gmsv_rsgdll_e2e_linux64.dll"
mkdir -p "$stage_root/garrysmod/addons/rsgdll-e2e"
cp -R "$repo_root/e2e/addon/." "$stage_root/garrysmod/addons/rsgdll-e2e/"
