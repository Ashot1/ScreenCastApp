// swift-tools-version: 6.2
import Foundation
import PackageDescription

// The experiment consumes the verified v0.17.7 runtime source; see the test-only manifest patch.
let source = ProcessInfo.processInfo.environment["PARAKEET_FLUIDAUDIO_SOURCE"]
let package = Package(
    name: "ParakeetFeasibility",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(
            name: "FluidAudio",
            path: source ?? "/private/tmp/ambient-04-parakeet/FluidInference-FluidAudio-2437bf2",
            traits: [])
    ],
    targets: [
        .executableTarget(
            name: "LocalParakeet",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")])
    ]
)
