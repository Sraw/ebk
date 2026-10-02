#!/bin/sh
# Usage: tools/dist.sh
# Builds into dist/: the converter for Windows, macOS (both processors in one file) and Linux, and the KOReader
# plug-in with its helper programs (Linux and macOS, x86-64 and ARM) and libraries (Android).
# Needs the Rust targets named below, tools/zig (the linker for the other systems), tools/cargo-tools
# (cargo-zigbuild) and, for Android, the Android NDK; see the README.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"
PATH="$here/tools/zig:$here/tools/cargo-tools/bin:$HOME/.cargo/bin:$PATH"
export PATH MACOSX_DEPLOYMENT_TARGET=11.0
log=$(mktemp)
trap 'rm -f "$log"' EXIT
build() { # <cargo arguments>: quiet unless it fails
    cargo "$@" > "$log" 2>&1 || { grep -E "^error" -A12 "$log" >&2 || tail -5 "$log" >&2; echo "failed: cargo $*" >&2; exit 1; }
}

for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf x86_64-pc-windows-gnu universal2-apple-darwin; do
    build zigbuild --quiet --release -p ebk-cli --target "$target"
done
# ARMv6 without floating-point hardware (the first Kindles): zig's C library lacks functions Rust calls there,
# so this one is linked with the linker and the C library that come with Rust
CARGO_TARGET_ARM_UNKNOWN_LINUX_MUSLEABI_LINKER=rust-lld CARGO_TARGET_ARM_UNKNOWN_LINUX_MUSLEABI_RUSTFLAGS="-C strip=symbols" \
    build build --quiet --release -p ebk-cli --target arm-unknown-linux-musleabi

# the Android NDK: the one named in the environment, or the newest in the Android SDK, or one in tools/ndk
ndk=${ANDROID_NDK_HOME:-}
[ -d "$ndk" ] || [ -z "${ANDROID_HOME:-}" ] || ndk=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)
[ -d "$ndk" ] || ndk=$(ls -d "$here"/tools/ndk/android-ndk-* 2>/dev/null | sort -V | tail -1)
ndk=${ndk:+$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin}
if [ -n "$ndk" ]; then
    # API level 21 is Android 5; pages of 16 KiB are what newer devices have
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk/aarch64-linux-android21-clang"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
    export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$ndk/armv7a-linux-androideabi21-clang"
    for target in aarch64-linux-android armv7-linux-androideabi; do
        build build --quiet --release -p ebk-ffi --target "$target"
    done
else
    echo "no Android NDK (ANDROID_NDK_HOME, ANDROID_HOME/ndk, tools/ndk): the plug-in is built without the Android libraries" >&2
fi

rm -rf dist && mkdir -p dist
pack() { # <name of the package> <the program> <its name in the package>
    mkdir -p "dist/$1" && cp "$2" "dist/$1/$3" && cp tools/dist/README.txt "dist/$1/" && (cd dist && zip -q -r "$1.zip" "$1")
}
pack ebk-windows target/x86_64-pc-windows-gnu/release/ebk.exe ebk.exe
pack ebk-macos target/universal2-apple-darwin/release/ebk ebk
pack ebk-linux target/x86_64-unknown-linux-musl/release/ebk ebk

plugin=dist/ebk.koplugin
mkdir -p $plugin/bin $plugin/lib
cp koreader/ebk.koplugin/*.lua koreader/README.txt $plugin/
cp target/x86_64-unknown-linux-musl/release/ebk $plugin/bin/ebk-x86_64
cp target/aarch64-unknown-linux-musl/release/ebk $plugin/bin/ebk-aarch64
cp target/armv7-unknown-linux-musleabihf/release/ebk $plugin/bin/ebk-armv7
cp target/arm-unknown-linux-musleabi/release/ebk $plugin/bin/ebk-armv6
cp target/universal2-apple-darwin/release/ebk $plugin/bin/ebk-macos
if [ -n "$ndk" ]; then
    "$ndk/llvm-strip" -o $plugin/lib/libebkffi-aarch64.so target/aarch64-linux-android/release/libebkffi.so
    "$ndk/llvm-strip" -o $plugin/lib/libebkffi-armv7.so target/armv7-linux-androideabi/release/libebkffi.so
fi
(cd dist && zip -q -r ebk.koplugin.zip ebk.koplugin)
ls -l dist/*.zip
