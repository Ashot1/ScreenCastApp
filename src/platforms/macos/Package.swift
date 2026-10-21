// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "AmbientMacOS",
    defaultLocalization: "en",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "AmbientMacOS", targets: ["AmbientMacOS"]),
        .library(name: "VoiceSupport", targets: ["VoiceSupport"]),
    ],
    targets: [
        .systemLibrary(name: "CAmbient"),
        .systemLibrary(name: "CLocalPermissions"),
        .target(
            name: "CVoiceProcess", publicHeadersPath: "include",
            cSettings: [.unsafeFlags(["-Wall", "-Wextra", "-Werror"])]),
        .target(
            name: "VoiceSupport", dependencies: ["CVoiceProcess"],
            resources: [.process("Resources")]),
        .target(
            name: "AmbientMacOS", dependencies: ["CAmbient", "CLocalPermissions", "VoiceSupport"],
            resources: [.process("Resources")]
        ),
        .target(
            name: "TestKeychainSupport", path: "Tests/TestKeychainSupport",
            publicHeadersPath: "include", linkerSettings: [.linkedFramework("Security")]),
        .executableTarget(
            name: "VoiceTestChild", dependencies: ["VoiceSupport", "CVoiceProcess"],
            path: "Tests/VoiceTestChild"),
        .testTarget(
            name: "AmbientMacOSTests",
            dependencies: [
                "AmbientMacOS", "CAmbient", "TestKeychainSupport", "VoiceSupport", "CVoiceProcess",
            ]),
    ],
    swiftLanguageModes: [.v6]
)
