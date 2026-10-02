#!/bin/sh
# Usage (on a Mac): tools/dist/macos/build-app.sh <the universal program ebk> <folder to put EBK.app into>
# Builds EBK.app: an AppleScript application with the program inside it, signed ad hoc (there is no developer
# certificate: macOS asks once before the first start, see tools/dist/README.txt).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/../../../Cargo.toml" | head -1)
app="$2/EBK.app"
rm -rf "$app"
osacompile -o "$app" "$here/EBK.applescript"
cp "$1" "$app/Contents/Resources/ebk"
chmod 755 "$app/Contents/Resources/ebk"
python3 "$here/info_plist.py" "$app/Contents/Info.plist" "$version"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
echo "$app"
