import Foundation
import XCTest

@testable import KeychainStore

final class KeychainStoreTests: XCTestCase {
    func testHelperSessionUsesBoundedVersionedJSONFieldNames() throws {
        let registration = HelperRegistration(
            clientID: "synthetic-client", issuer: "https://auth.openai.com",
            subject: "synthetic-subject")
        let session = HelperSession(
            accessToken: "SYNTHETIC_ACCESS", refreshToken: "SYNTHETIC_REFRESH",
            expiresAtMilliseconds: 123_456, grantedScopes: ["openid", "resource.invoke"],
            nonce: "SYNTHETIC_NONCE",
            registration: registration)
        let data = try JSONEncoder().encode(session)
        let value = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(value["access_token"] as? String, "SYNTHETIC_ACCESS")
        XCTAssertEqual(value["refresh_token"] as? String, "SYNTHETIC_REFRESH")
        XCTAssertEqual(value["expires_at_ms"] as? Int, 123_456)
        XCTAssertEqual(value["nonce"] as? String, "SYNTHETIC_NONCE")
        XCTAssertNotNil(value["granted_scopes"] as? [String])
        XCTAssertNotNil(value["registration"] as? [String: String])
    }

    func testHelperInputRejectsOneByteOverMaximumWithoutReadingFurther() throws {
        let zero = try FileHandle(forReadingFrom: URL(fileURLWithPath: "/dev/zero"))
        defer { try? zero.close() }
        XCTAssertThrowsError(try HelperInput.read(from: zero)) { error in
            XCTAssertTrue(error is HelperInputError)
        }
    }

    func testHelperInputReadsFragmentedJSONUntilEOF() throws {
        let pipe = Pipe()
        let expected = Data(("{\"value\":\"" + String(repeating: "x", count: 4096) + "\"}").utf8)
        let writer = pipe.fileHandleForWriting
        let finished = DispatchGroup()
        finished.enter()
        DispatchQueue.global().async {
            writer.write(expected.prefix(1))
            Thread.sleep(forTimeInterval: 0.02)
            writer.write(expected.dropFirst())
            writer.closeFile()
            finished.leave()
        }
        let result = try HelperInput.read(from: pipe.fileHandleForReading)
        finished.wait()
        try pipe.fileHandleForReading.close()
        XCTAssertEqual(result, expected)
    }
}
