#!/bin/sh
# Usage: tools/dist.sh
# Builds the converter for Windows, macOS (both processors in one file) and Linux, and the reader of the KOReader
# plug-in for ARM devices, into dist/. Needs the Rust targets named below, tools/zig (the linker for the other
# systems) and tools/cargo-tools (cargo-zigbuild); see the README.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"
PATH="$here/tools/zig:$here/tools/cargo-tools/bin:$PATH"
export PATH MACOSX_DEPLOYMENT_TARGET=11.0
build() { cargo zigbuild --quiet --release -p ebk-cli --target "$1" 2>&1 | grep -E "^error" -A8 || true; }
for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf x86_64-pc-windows-gnu universal2-apple-darwin; do
    build "$target"
done
rm -rf dist && mkdir -p dist
pack() { # <name of the package> <the program> <its name in the package>
    mkdir -p "dist/$1" && cp "$2" "dist/$1/$3" && cp tools/dist/README.txt "dist/$1/" && (cd dist && zip -q -r "$1.zip" "$1")
}
pack ebk-windows target/x86_64-pc-windows-gnu/release/ebk.exe ebk.exe
pack ebk-macos target/universal2-apple-darwin/release/ebk ebk
pack ebk-linux target/x86_64-unknown-linux-musl/release/ebk ebk
ls -l dist/*.zip
