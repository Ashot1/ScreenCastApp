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
            Debug|"Debug Visual") cargo build -p ambient-bridge --locked ;;
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
    visual)
        # «Ambient Visual» (ADR-024): the fixture build and test data for mockup screenshots; it
        # never reads the user's memory, preferences or ChatGPT Keychain item.
        ambient_visual_build="$ambient_root/target/verification/design/build"
        ambient_visual_templates="$ambient_root/target/verification/design/templates"
        case "${1:-}" in
            build)
                shift
                xcodebuild -project src/platforms/macos/Ambient.xcodeproj -scheme Ambient \
                    -configuration "Debug Visual" -derivedDataPath "$ambient_visual_build" \
                    -clonedSourcePackagesDirPath "$ambient_root/target/xcode-packages" \
                    -destination 'generic/platform=macOS' \
                    build "$@"
                ;;
            seed)
                AMBIENT_VISUAL_SEED_DIR="$ambient_visual_templates" \
                    bash "$ambient_root/scripts/macos.sh" test --filter seedVisualFixtureTemplates
                ;;
            open)
                ambient_visual_template="${2:?Template required: empty, corrupt or cases}"
                case "$ambient_visual_template" in
                    empty|corrupt|cases) ;;
                    *) echo "Template must be empty, corrupt or cases" >&2; exit 2 ;;
                esac
                ambient_visual_session="${3:?Session folder name required}"
                [[ "$ambient_visual_session" =~ ^[A-Za-z0-9._-]+$ && "$ambient_visual_session" != .* ]] ||
                    { echo "Session folder name must be a plain name" >&2; exit 2; }
                ambient_visual_root="$ambient_root/target/verification/design/sessions/$ambient_visual_session"
                [[ -d "$ambient_visual_templates/$ambient_visual_template" ]] ||
                    { echo "Unknown template; run scripts/macos.sh visual seed" >&2; exit 2; }
                # Each session starts from a fresh copy of its template.
                rm -rf "$ambient_visual_root"
                ditto "$ambient_visual_templates/$ambient_visual_template" "$ambient_visual_root"
                open -n "$ambient_visual_build/Build/Products/Debug Visual/Ambient Visual.app" \
                    --env "AMBIENT_VISUAL_ROOT=$ambient_visual_root"
                echo "$ambient_visual_root"
                ;;
            fingerprint)
                # Read-only fingerprint of the user's real data before and after a session: memory
                # files, preferences and the ChatGPT Keychain item's attributes (never its secret).
                ambient_visual_data="$HOME/Library/Application Support/dev.ambient.foundation"
                echo "== ${2:?Label required} $(date -u +%FT%TZ)"
                for ambient_visual_file in memory.sqlite3 memory.sqlite3-wal; do
                    if [[ -f "$ambient_visual_data/$ambient_visual_file" ]]; then
                        echo "$ambient_visual_file sha256=$(shasum -a 256 "$ambient_visual_data/$ambient_visual_file" | cut -d' ' -f1) mtime=$(stat -f %m "$ambient_visual_data/$ambient_visual_file")"
                    else
                        echo "$ambient_visual_file absent"
                    fi
                done
                echo "defaults sha256=$(defaults export dev.ambient.foundation - 2>/dev/null | shasum -a 256 | cut -d' ' -f1)"
                echo "keychain-attributes sha256=$(security find-generic-password -s dev.ambient.foundation.chatgpt 2>&1 | shasum -a 256 | cut -d' ' -f1)"
                echo "real-ambient-running=$(pgrep -f '/Ambient.app/Contents/MacOS/Ambient$' >/dev/null && echo yes || echo no)"
                ;;
            *) echo "Usage: scripts/macos.sh visual {build|seed|open <empty|corrupt|cases> <session>|fingerprint <label>}" >&2; exit 2 ;;
        esac
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
    *) echo "Usage: scripts/macos.sh {bridge [Debug|Release]|check|test|app|visual} [build options]" >&2; exit 2 ;;
esac
