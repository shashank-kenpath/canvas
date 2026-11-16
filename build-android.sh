#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/app"
export PATH="${FLUTTER_HOME:-/home/ubuntu/tools/flutter}/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-/home/ubuntu/Android/sdk}"
export JAVA_HOME="${JAVA_HOME:-/home/ubuntu/jdk/jdk-21.0.13+9}"
flutter pub get
flutter analyze
flutter test --timeout 60s
UPDATE_MANIFEST_URL="${UPDATE_MANIFEST_URL:-https://annotated-remedies-property-binding.trycloudflare.com/update.json}"
flutter build apk --release --dart-define="UPDATE_MANIFEST_URL=$UPDATE_MANIFEST_URL"
flutter build apk --release --split-per-abi --dart-define="UPDATE_MANIFEST_URL=$UPDATE_MANIFEST_URL"
