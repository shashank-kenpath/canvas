#!/usr/bin/env bash
# Builds Inkboard for the web into cloud/public/app, which the Worker serves
# at /app. Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at
# the version in rust/Cargo.lock.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
rust="$(dirname "$here")"
repo="$(dirname "$rust")"
out="$repo/cloud/public/app"
export PATH="$HOME/.cargo/bin:$PATH"

cd "$rust"
cargo build --release -p inkboard_web --target wasm32-unknown-unknown
rm -rf "$out"
mkdir -p "$out/fonts"
wasm-bindgen --target web --no-typescript --out-dir "$out" target/wasm32-unknown-unknown/release/inkboard_web.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int -o "$out/inkboard_web_bg.wasm" "$out/inkboard_web_bg.wasm"
fi

version=$(grep '^version' web/Cargo.toml | head -1 | cut -d'"' -f2)
cp web/www/index.html "$out/index.html"
sed "s/__VERSION__/$version/" web/www/main.js > "$out/main.js"
cp "$repo/branding/mark.svg" "$out/icon.svg"
cp app-android/app/src/main/assets/fonts/*.ttf "$out/fonts/"
cat > "$out/manifest.webmanifest" <<JSON
{ "name": "Inkboard", "short_name": "Inkboard", "start_url": "/app/", "scope": "/app/", "display": "standalone",
  "background_color": "#f7f6fb", "theme_color": "#2748e8", "icons": [{ "src": "icon.svg", "sizes": "any", "type": "image/svg+xml" }] }
JSON
du -sh "$out" "$out/inkboard_web_bg.wasm"
