#!/bin/bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
app_path="$PWD/.scratch/NativeContentProbeV2.app"
if [ -e "$app_path" ]; then
    echo 'Refusing to replace the frozen v2 app.' >&2
    exit 73
fi
mkdir -p .scratch
staging_dir="$(mktemp -d "$PWD/.scratch/staging.XXXXXX")"
trap 'rm -rf "$staging_dir"' EXIT
staging_app="$staging_dir/NativeContentProbeV2.app"
mkdir -p "$staging_app/Contents/MacOS"
cp Info.plist "$staging_app/Contents/Info.plist"
plutil -lint "$staging_app/Contents/Info.plist"
swiftc -swift-version 6 -strict-concurrency=complete -warnings-as-errors \
    -target "$(uname -m)-apple-macosx14.0" -module-cache-path "$PWD/.scratch/ModuleCache" \
    ProbeContracts.swift ProbePolicy.swift AXBoundary.swift SearchController.swift Sources/main.swift \
    -o "$staging_app/Contents/MacOS/NativeContentProbeV2"
codesign --sign - --timestamp=none --identifier dev.ambient.feasibility.native-content-v2 "$staging_app"
codesign --verify --strict --verbose=2 "$staging_app"
mv "$staging_app" "$app_path"
shasum -a 256 "$app_path/Contents/MacOS/NativeContentProbeV2"
codesign -d -r- --verbose=2 "$app_path"
