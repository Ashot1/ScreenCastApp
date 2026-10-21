// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "AmbientVoiceHelper",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "AmbientVoiceHelper", targets: ["AmbientVoiceHelper"])],
    dependencies: [
        .package(path: ".."),
        .package(
            url: "https://github.com/FluidInference/FluidAudio.git",
            revision: "503b4bd1bbf7220882de39fe8ae6716aae4132da", traits: []),
    ],
    targets: [
        .executableTarget(
            name: "AmbientVoiceHelper",
            dependencies: [
                .product(name: "VoiceSupport", package: "macos"),
                .product(name: "FluidAudio", package: "FluidAudio"),
            ],
            swiftSettings: [.unsafeFlags(["-warnings-as-errors", "-strict-concurrency=complete"])])
    ],
    swiftLanguageModes: [.v6]
)
