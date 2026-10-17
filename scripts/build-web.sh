#!/usr/bin/env bash
# Builds the browser version into dist/ (index.html + .js + .wasm).
#
# Needs: rustup target wasm32-unknown-unknown, and wasm-bindgen-cli in exactly
# the wasm-bindgen version from Cargo.lock.
# Set CARGO to a wrapper command to control how cargo is invoked
# (default: "cargo"). The target directory is asked from cargo itself, so a
# wrapper that sets CARGO_TARGET_DIR internally works too.
set -euo pipefail

cd "$(dirname "$0")/.."
read -r -a cargo_cmd <<< "${CARGO:-cargo}"
target_dir=$("${cargo_cmd[@]}" metadata --format-version 1 --no-deps \
  | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
if [[ -z "$target_dir" ]]; then
  echo "could not determine cargo's target directory" >&2
  exit 1
fi

want=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')
have=$(wasm-bindgen --version | awk '{print $2}')
if [[ "$want" != "$have" ]]; then
  echo "wasm-bindgen-cli $have installed, Cargo.lock needs $want" >&2
  exit 1
fi

"${cargo_cmd[@]}" build --profile wasm-release --target wasm32-unknown-unknown --bin timefighter

rm -rf dist
mkdir -p dist
wasm-bindgen --target web --no-typescript --out-dir dist \
  "$target_dir/wasm32-unknown-unknown/wasm-release/timefighter.wasm"
cp web/index.html web/showroom.html web/credits.html web/README-local.txt dist/
cp -r web/img dist/img
cp -r assets dist/assets

if command -v wasm-opt > /dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o dist/timefighter_bg.wasm dist/timefighter_bg.wasm
fi

ls -l dist
