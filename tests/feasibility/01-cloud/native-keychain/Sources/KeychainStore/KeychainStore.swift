import Foundation
import Security

public struct KeychainStore {
    private let service = "com.ambientassistant.feasibility.session.v1"
    private let maximumSessionBytes = 48 * 1024
    private let testKeychain: CFTypeRef?

    public init() {
        testKeychain = nil
    }

    init(testKeychain: CFTypeRef) {
        self.testKeychain = testKeychain
    }

    public func read<Session: Decodable>(account: String, as type: Session.Type) throws -> Session?
    {
        let query = try query(account: account).merging([
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]) { _, new in new }
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data, data.count <= maximumSessionBytes
        else {
            throw KeychainStoreError(status: status)
        }
        return try JSONDecoder().decode(type, from: data)
    }

    public func add<Session: Encodable>(account: String, session: Session) throws {
        let data = try encoded(session)
        var item = try query(account: account, adding: true)
        item[kSecValueData as String] = data
        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeychainStoreError(status: status) }
    }

    public func update<Session: Encodable>(account: String, session: Session) throws {
        let query = try query(account: account)
        let attributes = [kSecValueData as String: try encoded(session)] as CFDictionary
        let status = SecItemUpdate(query as CFDictionary, attributes)
        guard status == errSecSuccess else { throw KeychainStoreError(status: status) }
    }

    public func delete(account: String) throws {
        let status = SecItemDelete(try query(account: account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainStoreError(status: status)
        }
    }

    private func encoded<Session: Encodable>(_ session: Session) throws -> Data {
        let data = try JSONEncoder().encode(session)
        guard data.count <= maximumSessionBytes else {
            throw KeychainStoreError(status: errSecDataTooLarge)
        }
        return data
    }

    private func query(account: String, adding: Bool = false) throws -> [String: Any] {
        guard !account.isEmpty, account.utf8.count <= 512 else {
            throw KeychainStoreError(status: errSecParam)
        }
        var result: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecUseDataProtectionKeychain as String: false,
        ]
        if let testKeychain {
            // Add selects a destination; queries select a search scope without changing the user's search list.
            if adding {
                result[kSecUseKeychain as String] = testKeychain
            } else {
                result[kSecMatchSearchList as String] = [testKeychain]
            }
        }
        return result
    }
}

public struct KeychainStoreError: Error {
    public let status: OSStatus

    public init(status: OSStatus) {
        self.status = status
    }
}

public enum HelperInputError: Error {
    case tooLarge
}

public enum HelperInput {
    public static let maximumBytes = 64 * 1024

    public static func read(from handle: FileHandle) throws -> Data {
        var data = Data()
        // Pipes can deliver partial chunks; never accumulate more than the limit plus one byte.
        while data.count <= maximumBytes {
            let count = min(8192, maximumBytes + 1 - data.count)
            guard let chunk = try handle.read(upToCount: count), !chunk.isEmpty else { return data }
            data.append(chunk)
        }
        throw HelperInputError.tooLarge
    }
}

public struct HelperRegistration: Codable, Equatable {
    public let clientID: String
    public let issuer: String
    public let subject: String

    public init(clientID: String, issuer: String, subject: String) {
        self.clientID = clientID
        self.issuer = issuer
        self.subject = subject
    }

    enum CodingKeys: String, CodingKey {
        case clientID = "client_id"
        case issuer
        case subject
    }
}

public struct HelperSession: Codable, Equatable {
    public let accessToken: String
    public let refreshToken: String
    public let expiresAtMilliseconds: Int64
    public let grantedScopes: [String]
    public let nonce: String
    public let registration: HelperRegistration

    public init(
        accessToken: String, refreshToken: String, expiresAtMilliseconds: Int64,
        grantedScopes: [String],
        nonce: String, registration: HelperRegistration
    ) {
        self.accessToken = accessToken
        self.refreshToken = refreshToken
        self.expiresAtMilliseconds = expiresAtMilliseconds
        self.grantedScopes = grantedScopes
        self.nonce = nonce
        self.registration = registration
    }

    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case expiresAtMilliseconds = "expires_at_ms"
        case grantedScopes = "granted_scopes"
        case nonce
        case registration
    }
}
