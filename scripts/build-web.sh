#!/usr/bin/env bash
# Builds the browser version into dist/ (index.html + .js + .wasm).
#
# The game's script and WebAssembly (and the page script leaderboard.js)
# get a content hash in their file names, written into the pages: a browser
# that still has one of them cached from an earlier deploy can never pair
# it with the other from a newer one.
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
cp web/index.html web/showroom.html web/credits.html web/story.html web/leaderboard.js web/README-local.txt dist/
cp -r web/img dist/img
cp -r web/fonts dist/fonts
cp -r assets dist/assets

if command -v wasm-opt > /dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o dist/timefighter_bg.wasm dist/timefighter_bg.wasm
fi

hash=$(cat dist/timefighter.js dist/timefighter_bg.wasm dist/leaderboard.js | sha256sum | cut -c1-12)
mv dist/timefighter_bg.wasm "dist/timefighter_bg.$hash.wasm"
# The script's own fallback URL of the WebAssembly (used by showroom.html).
sed "s/timefighter_bg\.wasm/timefighter_bg.$hash.wasm/g" dist/timefighter.js > "dist/timefighter.$hash.js"
rm dist/timefighter.js
mv dist/leaderboard.js "dist/leaderboard.$hash.js"
for page in dist/index.html dist/showroom.html; do
  sed -i -e "s#\./timefighter\.js#./timefighter.$hash.js#g" \
    -e "s#\./timefighter_bg\.wasm#./timefighter_bg.$hash.wasm#g" \
    -e "s#\./leaderboard\.js#./leaderboard.$hash.js#g" "$page"
done
for file in dist/index.html dist/showroom.html "dist/timefighter.$hash.js"; do
  if grep -qE "timefighter\.js|timefighter_bg\.wasm|leaderboard\.js" "$file"; then
    echo "$file still names an unhashed file" >&2
    exit 1
  fi
done
grep -q "timefighter_bg.$hash.wasm" "dist/timefighter.$hash.js" \
  || { echo "timefighter.$hash.js does not name its WebAssembly" >&2; exit 1; }

ls -l dist
