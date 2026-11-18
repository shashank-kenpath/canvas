#!/usr/bin/env bash
# Puts the last build on the local update feed (http://192.168.88.67:8790/).
# Run on the build machine after build.sh: rust/app-android/publish-lan.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
feed="$HOME/inkboard-ota"
version="$(grep -oP 'versionName = "\K[^"]+' "$here/app/build.gradle.kts")"
apk="inkboard-$version.apk"

mkdir -p "$feed"
cp "$root/out/canvas-studio-rust.apk" "$feed/$apk.tmp"
mv "$feed/$apk.tmp" "$feed/$apk"
printf '{"version":"%s","apk":"%s","sha256":"%s"}\n' "$version" "$apk" "$(sha256sum "$feed/$apk" | cut -d' ' -f1)" > "$feed/latest.json.tmp"
mv "$feed/latest.json.tmp" "$feed/latest.json"
# Keep the three newest builds.
ls -t "$feed"/inkboard-*.apk | tail -n +4 | xargs -r rm --
cat "$feed/latest.json"
