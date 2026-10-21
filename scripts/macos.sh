#!/bin/bash
set -euo pipefail

ambient_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ambient_root"
export MACOSX_DEPLOYMENT_TARGET=26.0

ambient_command="${1:-}"
if [[ $# -gt 0 ]]; then shift; fi

# The app embeds the built-in Google Desktop client at build time (providers/build.rs); the client
# JSON stays outside the repository. One value for every command, so builds do not alternate; set
# only when the file exists, so a machine without it does not rebuild on every run.
ambient_google_client="${AMBIENT_GOOGLE_OAUTH_CLIENT:-$HOME/.config/ambient/google-oauth-client.json}"
if [[ -f "$ambient_google_client" ]]; then export AMBIENT_GOOGLE_OAUTH_CLIENT="$ambient_google_client"; fi

case "$ambient_command" in
    bridge)
        case "${1:-Debug}" in
            Debug) cargo build -p ambient-bridge --locked ;;
            Release) cargo build -p ambient-bridge --release --locked ;;
            *) echo "Unsupported Xcode configuration" >&2; exit 2 ;;
        esac
        ;;
    check|test)
        if [[ "$ambient_command" == test ]]; then
            cargo build -p ambient-bridge --locked
        fi
        ambient_swift_command=build
        if [[ "$ambient_command" == test ]]; then ambient_swift_command=test; fi
        swift "$ambient_swift_command" --package-path src/platforms/macos \
            --cache-path "$ambient_root/target/swiftpm/cache" \
            --config-path "$ambient_root/target/swiftpm/configuration" \
            --security-path "$ambient_root/target/swiftpm/security" \
            -Xswiftc -warnings-as-errors -Xswiftc -strict-concurrency=complete \
            -Xlinker -L -Xlinker "$ambient_root/target/debug" "$@"
        ;;
    app)
        xcodebuild -project src/platforms/macos/Ambient.xcodeproj -scheme Ambient \
            -configuration Debug -derivedDataPath "$ambient_root/target/macos" \
            -clonedSourcePackagesDirPath "$ambient_root/target/xcode-packages" \
            -destination 'generic/platform=macOS' \
            build "$@"
        ;;
    voice)
        ambient_voice_arguments=(--package-path src/platforms/macos/VoiceHelper
            --scratch-path "$ambient_root/target/voice/build"
            --cache-path "$ambient_root/target/voice/cache"
            --config-path "$ambient_root/target/voice/configuration"
            --security-path "$ambient_root/target/voice/security" --configuration release)
        swift build "${ambient_voice_arguments[@]}" --product AmbientVoiceHelper
        ambient_voice_products="$(swift build "${ambient_voice_arguments[@]}" --show-bin-path)"
        ambient_voice_bundle="${1:?App bundle path required}"
        ambient_voice_identity="${2:--}"
        ambient_voice_helper_bundle="$ambient_voice_bundle/Contents/Helpers/AmbientVoiceHelper.app"
        mkdir -p "$ambient_voice_helper_bundle/Contents/MacOS" "$ambient_voice_helper_bundle/Contents/Resources/VoiceNotices"
        ditto "$ambient_voice_products/AmbientVoiceHelper" "$ambient_voice_helper_bundle/Contents/MacOS/AmbientVoiceHelper"
        ditto "$ambient_root/src/platforms/macos/VoiceHelper/Info.plist" "$ambient_voice_helper_bundle/Contents/Info.plist"
        for ambient_voice_resource in AmbientMacOS_VoiceSupport.bundle FluidAudio_FluidAudio.bundle; do
            ditto "$ambient_voice_products/$ambient_voice_resource" "$ambient_voice_helper_bundle/Contents/Resources/$ambient_voice_resource"
        done
        ditto "$ambient_root/src/platforms/macos/VoiceHelper/Notices" "$ambient_voice_helper_bundle/Contents/Resources/VoiceNotices"
        for ambient_voice_locale in en ru; do
            mkdir -p "$ambient_voice_bundle/Contents/Resources/$ambient_voice_locale.lproj"
            ditto "$ambient_root/src/platforms/macos/App/Resources/$ambient_voice_locale.lproj/InfoPlist.strings" \
                "$ambient_voice_bundle/Contents/Resources/$ambient_voice_locale.lproj/InfoPlist.strings"
        done
        /usr/bin/codesign --force --sign "$ambient_voice_identity" --options runtime \
            "$ambient_voice_helper_bundle"
        ;;
    *) echo "Usage: scripts/macos.sh {bridge [Debug|Release]|check|test|app} [build options]" >&2; exit 2 ;;
esac
