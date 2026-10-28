#!/usr/bin/env bash
# Build `libwordcraft_android.so` for Android (`arm64-v8a`) and assemble the APK.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ANDROID_DIR="${ROOT_DIR}/packaging/android"
JNI_ARM64_DIR="${ANDROID_DIR}/app/src/main/jniLibs/arm64-v8a"
DIST_DIR="${ROOT_DIR}/dist/release"

if [[ -z "${ANDROID_NDK_HOME:-}" && -n "${ANDROID_SDK_ROOT:-}" ]]; then
    ANDROID_NDK_HOME="$(ls -d "${ANDROID_SDK_ROOT}/ndk/"* 2>/dev/null | sort -V | tail -n 1 || true)"
fi

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    echo "error: ANDROID_NDK_HOME is not set" >&2
    exit 1
fi

HOST_TAG="linux-x86_64"
case "$(uname -s)" in
    Darwin*) HOST_TAG="darwin-x86_64" ;;
    MINGW*|MSYS*|CYGWIN*) HOST_TAG="windows-x86_64" ;;
esac

TOOLCHAIN_BIN="${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/${HOST_TAG}/bin"
export CC_aarch64_linux_android="${TOOLCHAIN_BIN}/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="${TOOLCHAIN_BIN}/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="${TOOLCHAIN_BIN}/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${CC_aarch64_linux_android}"

echo "==> Building wordcraft-android (aarch64-linux-android)..."
cargo build --manifest-path "${ROOT_DIR}/Cargo.toml" \
    --release \
    --target aarch64-linux-android \
    -p wordcraft-android

mkdir -p "${JNI_ARM64_DIR}"
cp -f "${ROOT_DIR}/target/aarch64-linux-android/release/libwordcraft_android.so" \
    "${JNI_ARM64_DIR}/libwordcraft_android.so"

echo "==> Assembling Android APK via Gradle..."
pushd "${ANDROID_DIR}" >/dev/null
gradle assembleRelease --no-daemon
popd >/dev/null

VERSION="$(grep -m1 '^version = ' "${ROOT_DIR}/Cargo.toml" | sed 's/version = "\(.*\)"/\1/')"
mkdir -p "${DIST_DIR}"
cp -f "${ANDROID_DIR}/app/build/outputs/apk/release/app-release.apk" \
    "${DIST_DIR}/wordcraft-${VERSION}-android-arm64-v8a.apk"

echo "==> Built APK: ${DIST_DIR}/wordcraft-${VERSION}-android-arm64-v8a.apk"
