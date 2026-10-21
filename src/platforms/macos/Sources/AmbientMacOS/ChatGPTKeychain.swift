import Foundation
import Security

/// Security owns the immutable reference and synchronizes SecItem operations. The coordinator
/// serializes logical read/commit/logout; the optional reference confines integration tests.
final class ChatGPTKeychain: ChatGPTCredentialStore, @unchecked Sendable {
    private let service: String
    private let keychain: SecKeychain?
    init(service: String = "dev.ambient.foundation.chatgpt", keychain: SecKeychain? = nil) {
        self.service = service
        self.keychain = keychain
    }

    private func query(add: Bool = false) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: "installation",
            kSecAttrSynchronizable as String: false,
        ]
        if let keychain {
            if add {
                q[kSecUseKeychain as String] = keychain
            } else {
                q[kSecMatchSearchList as String] = [keychain]
            }
        }
        return q
    }
    func load() throws -> ChatGPTEnvelope? {
        var q = query()
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw ChatGPTFailure.storage }
        guard let data = result as? Data else { throw ChatGPTFailure.corrupt }
        return try ChatGPTEnvelope.decode(data)
    }
    func save(_ envelope: ChatGPTEnvelope) throws {
        let data = try envelope.encoded()
        let status = SecItemUpdate(
            query() as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var q = query(add: true)
            q[kSecValueData as String] = data
            guard SecItemAdd(q as CFDictionary, nil) == errSecSuccess else {
                throw ChatGPTFailure.storage
            }
        } else if status != errSecSuccess {
            throw ChatGPTFailure.storage
        }
    }
}
