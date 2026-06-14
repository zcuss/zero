#!/usr/bin/env bash
# Setup Android SDK + NDK for Tauri 2.0 mobile builds.
# Idempotent — safe to re-run.
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "must run as root (sudo $0)" >&2
  exit 1
fi

export DEBIAN_FRONTEND=noninteractive

echo "[1/6] installing JDK 17 + tools..."
apt-get update -qq
apt-get install -y -qq openjdk-17-jdk unzip wget curl

export JAVA_HOME=$(dirname $(dirname $(readlink -f $(which javac))))
echo "JAVA_HOME=$JAVA_HOME"

ANDROID_HOME=${ANDROID_HOME:-/opt/android-sdk}
ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/$(ls $ANDROID_HOME/ndk 2>/dev/null | sort -V | tail -1)}
mkdir -p "$ANDROID_HOME/cmdline-tools"

echo "[2/6] downloading Android command-line tools (if missing)..."
if [[ ! -x "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" ]]; then
  cd /tmp
  if [[ ! -f commandlinetools.zip ]]; then
    wget -q https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip -O commandlinetools.zip
  fi
  unzip -q -o commandlinetools.zip
  mkdir -p "$ANDROID_HOME/cmdline-tools/latest"
  cp -r cmdline-tools/* "$ANDROID_HOME/cmdline-tools/latest/"
fi

echo "[3/6] installing platform + build-tools + NDK..."
export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
yes | sdkmanager --licenses >/dev/null
sdkmanager --install "platform-tools" "platforms;android-34" "build-tools;34.0.0" "ndk;26.1.10909125" 2>&1 | tail -3

# refresh NDK_HOME in case the directory name changed
ANDROID_NDK_HOME=$ANDROID_HOME/ndk/26.1.10909125

echo "[4/6] writing env file to /etc/profile.d/android.sh..."
cat > /etc/profile.d/android.sh <<EOF
export JAVA_HOME=$JAVA_HOME
export ANDROID_HOME=$ANDROID_HOME
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=$ANDROID_NDK_HOME
export PATH="\$PATH:\$ANDROID_HOME/cmdline-tools/latest/bin:\$ANDROID_HOME/platform-tools"
EOF

echo "[5/6] persisting env for non-login shells..."
cat > /root/.android_env <<EOF
export JAVA_HOME=$JAVA_HOME
export ANDROID_HOME=$ANDROID_HOME
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=$ANDROID_NDK_HOME
export PATH="\$PATH:\$ANDROID_HOME/cmdline-tools/latest/bin:\$ANDROID_HOME/platform-tools"
EOF

echo "[6/6] done. Activate with:  source /root/.android_env"
echo "Verify:  sdkmanager --list_installed  |  java --version"
