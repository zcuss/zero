#!/usr/bin/env bash
# Build + sign Android APK for Zero.
# Requires: ANDROID_HOME set, JDK 17, rustc, npm, this project at $1 (or current dir).
set -euo pipefail

PROJECT_ROOT="${1:-$PWD}"
cd "$PROJECT_ROOT"

# load env if present
[[ -f /root/.android_env ]] && source /root/.android_env
export PATH="/usr/local/bin:$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

echo "[1/4] building rust + android assets..."
npm run tauri -- android build --ci

APK="src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
[[ -f "$APK" ]] || { echo "no APK at $APK"; exit 1; }

KEYSTORE="/root/.android/debug.keystore"
if [[ ! -f "$KEYSTORE" ]]; then
  mkdir -p "$(dirname "$KEYSTORE")"
  keytool -genkey -v -keystore "$KEYSTORE" -alias androiddebugkey \
    -keyalg RSA -keysize 2048 -validity 10000 \
    -dname "CN=Zero Debug,O=Zero,C=US" \
    -storepass android -keypass android >/dev/null
fi

OUT="${APK/unsigned/signed}"
ALIGNED="${APK%.apk}-aligned.apk"
echo "[2/4] zipalign..."
"$ANDROID_HOME/build-tools/34.0.0/zipalign" -f 4 "$APK" "$ALIGNED"
echo "[3/4] apksigner..."
"$ANDROID_HOME/build-tools/34.0.0/apksigner" sign \
  --ks "$KEYSTORE" --ks-pass pass:android \
  --ks-key-alias androiddebugkey --key-pass pass:android \
  --out "$OUT" "$ALIGNED"
"$ANDROID_HOME/build-tools/34.0.0/apksigner" verify "$OUT"
echo "[4/4] artifacts:"
ls -lh "$OUT" \
  src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab
echo
echo "install on connected device:  adb install -r $OUT"
echo "install aab to play store:    upload $(realpath src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab)"
