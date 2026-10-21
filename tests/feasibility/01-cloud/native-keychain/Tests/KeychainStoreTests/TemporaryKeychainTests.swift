import Foundation
import Security
import XCTest

@testable import KeychainStore

// Only the fixture needs the legacy API to create an isolated file; storage uses SecItem throughout.
@available(macOS, deprecated: 10.10)
final class TemporaryKeychainTests: XCTestCase {
    func testNativeRoundTripInAnExplicitTemporaryKeychain() throws {
        var originalSearchList: CFArray?
        XCTAssertEqual(SecKeychainCopySearchList(&originalSearchList), errSecSuccess)
        let originalList = try XCTUnwrap(originalSearchList)
        var originalDefault: SecKeychain?
        let originalDefaultStatus = SecKeychainCopyDefault(&originalDefault)
        func assertConfigurationUnchanged() throws {
            var currentSearchList: CFArray?
            XCTAssertEqual(SecKeychainCopySearchList(&currentSearchList), errSecSuccess)
            // Compare metadata in memory without printing any user Keychain names or paths.
            XCTAssertTrue(CFEqual(originalList, try XCTUnwrap(currentSearchList)))
            var currentDefault: SecKeychain?
            XCTAssertEqual(SecKeychainCopyDefault(&currentDefault), originalDefaultStatus)
            XCTAssertEqual(originalDefault == nil, currentDefault == nil)
            if let originalDefault, let currentDefault {
                XCTAssertTrue(CFEqual(originalDefault, currentDefault))
            }
        }
        defer { XCTAssertNoThrow(try assertConfigurationUnchanged()) }
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ambient-keychain-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = directory.appendingPathComponent("isolated.keychain").path
        let password = UUID().uuidString
        var created: SecKeychain?
        let status = password.withCString {
            SecKeychainCreate(path, UInt32(password.utf8.count), $0, false, nil, &created)
        }
        XCTAssertEqual(status, errSecSuccess)
        let keychain = try XCTUnwrap(created)
        defer { XCTAssertEqual(SecKeychainDelete(keychain), errSecSuccess) }
        try assertConfigurationUnchanged()
        let store = KeychainStore(testKeychain: keychain)
        let account = "v1:synthetic:\(UUID().uuidString)"
        let binding = HelperRegistration(
            clientID: "synthetic-client", issuer: "https://auth.openai.com",
            subject: "synthetic-subject")
        let session = HelperSession(
            accessToken: "SYNTHETIC_ACCESS", refreshToken: "SYNTHETIC_REFRESH",
            expiresAtMilliseconds: 123_456, grantedScopes: ["openid", "resource.invoke"],
            nonce: "synthetic-nonce",
            registration: binding)
        XCTAssertNil(try store.read(account: account, as: HelperSession.self))
        try store.add(account: account, session: session)
        XCTAssertEqual(try store.read(account: account, as: HelperSession.self), session)
        XCTAssertThrowsError(try store.add(account: account, session: session))
        let rotated = HelperSession(
            accessToken: "ROTATED_ACCESS", refreshToken: "ROTATED_REFRESH",
            expiresAtMilliseconds: 234_567, grantedScopes: session.grantedScopes,
            nonce: session.nonce,
            registration: binding)
        try store.update(account: account, session: rotated)
        XCTAssertEqual(try store.read(account: account, as: HelperSession.self), rotated)
        XCTAssertNil(try store.read(account: account + "-other", as: HelperSession.self))
        try store.delete(account: account)
        XCTAssertNil(try store.read(account: account, as: HelperSession.self))
        try assertConfigurationUnchanged()
    }
}
