// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AmbientFeasibilityKeychain",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "keychain-helper", targets: ["KeychainHelper"]),
        .library(name: "KeychainStore", targets: ["KeychainStore"]),
    ],
    targets: [
        .target(name: "KeychainStore"),
        .executableTarget(name: "KeychainHelper", dependencies: ["KeychainStore"]),
        .testTarget(name: "KeychainStoreTests", dependencies: ["KeychainStore"]),
    ],
    swiftLanguageModes: [.v6]
)
