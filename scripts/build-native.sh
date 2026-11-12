#!/usr/bin/env bash
#
# Builds the Android native libraries.
#
# The Oboe shim is C++, so it is built with CMake and the Rust libraries are
# built against it. Both end up in the same directory, which is the layout
# the APK expects.
#
# Usage:
#   scripts/build-native.sh <output directory> [abi ...]
#
# The ABIs default to arm64-v8a. The NDK is taken from ANDROID_NDK (or
# ANDROID_NDK_HOME), and the NDK's own CMake is used, because the CMake on a
# build machine is often newer than the projects that are being built.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:?usage: build-native.sh <output directory> [abi ...]}"
shift || true

ABIS=("$@")
if [ ${#ABIS[@]} -eq 0 ]; then
  ABIS=(arm64-v8a)
fi

NDK="${ANDROID_NDK:-${ANDROID_NDK_HOME:-}}"
if [ -z "$NDK" ] || [ ! -d "$NDK" ]; then
  echo "The Android NDK was not found. Set ANDROID_NDK." >&2
  exit 1
fi

# The CMake of the Android SDK is used when it is installed; otherwise the
# one on the machine is, with the policy floor lowered for the old projects.
CMAKE=""
for candidate in \
  "${ANDROID_HOME:-}"/cmake/*/bin/cmake \
  "${ANDROID_SDK_ROOT:-}"/cmake/*/bin/cmake \
  "$NDK"/build/cmake/*/bin/cmake; do
  if [ -x "$candidate" ]; then
    CMAKE="$candidate"
    break
  fi
done
if [ -z "$CMAKE" ]; then
  CMAKE="$(command -v cmake || true)"
fi
if [ -z "$CMAKE" ]; then
  echo "No CMake was found. Install the SDK's CMake with: sdkmanager cmake;3.22.1" >&2
  exit 1
fi

echo "Using $($CMAKE --version | head -1) from the NDK"

JNI_LIBS="$OUT/jniLibs"
mkdir -p "$JNI_LIBS"

for ABI in "${ABIS[@]}"; do
  echo "Building the Oboe shim for $ABI"
  "$CMAKE" -S "$ROOT/cpp" -B "$OUT/oboe/$ABI" \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$ABI" \
    -DANDROID_PLATFORM=android-26 \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -DCMAKE_LIBRARY_OUTPUT_DIRECTORY="$JNI_LIBS/$ABI" \
    >/dev/null
  "$CMAKE" --build "$OUT/oboe/$ABI" --config Release -j2 >/dev/null
done

echo "Building the Rust libraries"
export VIMO_NATIVE_LIB_DIR="$JNI_LIBS"
CARGO_NDK_ARGS=()
for ABI in "${ABIS[@]}"; do
  CARGO_NDK_ARGS+=(-t "$ABI")
done

cargo ndk "${CARGO_NDK_ARGS[@]}" \
  -o "$JNI_LIBS" \
  build --release \
  -p vimo-jni \
  --manifest-path "$ROOT/Cargo.toml"

echo "The libraries are in $JNI_LIBS"
find "$JNI_LIBS" -name '*.so' | sort
