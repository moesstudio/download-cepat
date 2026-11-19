#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export NDK_HOME="${NDK_HOME:-$ANDROID_HOME/ndk/27.2.12479018}"

if [[ -z "${JAVA_HOME:-}" ]]; then
  if [[ -d "/Applications/Android Studio.app/Contents/jbr/Contents/Home" ]]; then
    export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
  elif command -v /usr/libexec/java_home >/dev/null 2>&1; then
    export JAVA_HOME="$(/usr/libexec/java_home)"
  fi
fi

export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/build-tools/35.0.0:$PATH"

APKSIGNER="$ANDROID_HOME/build-tools/35.0.0/apksigner"
AAPT2="$ANDROID_HOME/build-tools/35.0.0/aapt2"
KEYSTORE_PROPS="$ROOT_DIR/src-tauri/gen/android/keystore.properties"

if [[ ! -d "$ANDROID_HOME" ]]; then
  echo "ERROR: ANDROID_HOME not found at $ANDROID_HOME" >&2
  exit 1
fi
if [[ ! -d "$NDK_HOME" ]]; then
  echo "ERROR: NDK_HOME not found at $NDK_HOME" >&2
  exit 1
fi
if [[ ! -x "$APKSIGNER" ]]; then
  echo "ERROR: apksigner not found at $APKSIGNER" >&2
  exit 1
fi
if [[ ! -f "$KEYSTORE_PROPS" && -z "${ANDROID_KEYSTORE_PATH:-}" ]]; then
  echo "ERROR: Signing config missing ($KEYSTORE_PROPS or ANDROID_KEYSTORE_PATH)." >&2
  exit 1
fi

APP_VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
INSTALLER_DIR="$ROOT_DIR/installer"
mkdir -p "$INSTALLER_DIR"

find_signed_release_apk() {
  local apk_root="$ROOT_DIR/src-tauri/gen/android/app/build/outputs/apk"
  local candidate
  candidate="$(find "$apk_root" -type f -name '*release*.apk' ! -name '*-unsigned.apk' 2>/dev/null | head -n 1 || true)"
  if [[ -z "$candidate" ]]; then
    local unsigned
    unsigned="$(find "$apk_root" -type f -name '*unsigned*.apk' 2>/dev/null | head -n 1 || true)"
    if [[ -n "$unsigned" ]]; then
      echo "ERROR: Found only unsigned APK ($unsigned). Check keystore.properties / release signingConfig." >&2
    else
      echo "ERROR: No release APK found under $apk_root" >&2
    fi
    return 1
  fi
  echo "$candidate"
}

echo "==> Building Download Cepat v${APP_VERSION} Android (arm64-v8a)..."
rm -rf "$ROOT_DIR/src-tauri/gen/android/app/build/outputs/apk"
npx tauri android build --apk --target aarch64 --ci
ARM64_APK_SRC="$(find_signed_release_apk)"
ARM64_APK_OUT="$INSTALLER_DIR/Download Cepat_${APP_VERSION}_arm64-v8a.apk"
cp -f "$ARM64_APK_SRC" "$ARM64_APK_OUT"

echo "==> Building Download Cepat v${APP_VERSION} Android (universal: arm64-v8a + armeabi-v7a)..."
rm -rf "$ROOT_DIR/src-tauri/gen/android/app/build/outputs/apk"
npx tauri android build --apk --target aarch64 armv7 --ci
UNIV_APK_SRC="$(find_signed_release_apk)"
UNIV_APK_OUT="$INSTALLER_DIR/Download Cepat_${APP_VERSION}_universal.apk"
cp -f "$UNIV_APK_SRC" "$UNIV_APK_OUT"

for apk in "$ARM64_APK_OUT" "$UNIV_APK_OUT"; do
  echo "==> Verifying APK signature: $apk"
  "$APKSIGNER" verify --verbose "$apk"
  if [[ -x "$AAPT2" ]]; then
    echo "==> Badging summary: $(basename "$apk")"
    "$AAPT2" dump badging "$apk" | grep -E "package:|sdkVersion:|targetSdkVersion:|native-code:" || true
  fi
  ls -lh "$apk"
done

echo "✅ Android APKs ready in $INSTALLER_DIR"
