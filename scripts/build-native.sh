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
# ANDROID_NDK_HOME), and the NDK's own CMake is used when it is installed,
# because the CMake on a build machine is often newer than the projects that
# are being built.
#
# The output directory only ever holds what is meant to be packaged:
#
#   <output>/jniLibs/<abi>/libvimo_jni.so     the JNI library
#   <output>/jniLibs/<abi>/libvimo_oboe.so   the Oboe shim
#
# The CMake build tree is kept outside of it.

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

echo "Using $($CMAKE --version | head -1)"

JNI_LIBS="$OUT/jniLibs"
mkdir -p "$JNI_LIBS"

# The CMake build tree is scratch space: it holds the object files and must
# never end up in the package.
BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vimo-oboe-build.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT

for ABI in "${ABIS[@]}"; do
  echo "Building the Oboe shim for $ABI"
  "$CMAKE" -S "$ROOT/cpp" -B "$BUILD_DIR/$ABI" \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$ABI" \
    -DANDROID_PLATFORM=android-26 \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -DCMAKE_LIBRARY_OUTPUT_DIRECTORY="$JNI_LIBS/$ABI" \
    >/dev/null
  "$CMAKE" --build "$BUILD_DIR/$ABI" --config Release -j2 >/dev/null
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

# The C ABI library is not used on Android: the JNI library is the entry
# point there. It is dropped so that it does not end up in the package.
find "$JNI_LIBS" -name 'libvimo_core.so' -delete

# The symbols are stripped so that the package stays small.
STRIP="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
if [ -x "$STRIP" ]; then
  find "$JNI_LIBS" -name '*.so' -exec "$STRIP" --strip-unneeded {} +
fi

echo "The libraries are in $JNI_LIBS"
find "$JNI_LIBS" -name '*.so' -printf '%p %k KB\n' | sort
