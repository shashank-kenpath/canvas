#!/usr/bin/env bash
# Builds the Rust engine for Android and packages the app.
# Run on the build machine from anywhere: rust/app-android/build.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"

export PATH="$HOME/.cargo/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/28.2.13676358}"
export JAVA_HOME="${JAVA_HOME:-$HOME/jdk/jdk-21.0.13+9}"

cd "$root"
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 -o app-android/app/src/main/jniLibs build --release -p canvas_android

cd "$here"
echo "sdk.dir=$ANDROID_HOME" > local.properties
./gradlew --no-daemon assembleRelease

mkdir -p "$root/out"
cp app/build/outputs/apk/release/app-release.apk "$root/out/canvas-studio-rust.apk"
ls -la "$root/out/canvas-studio-rust.apk"
