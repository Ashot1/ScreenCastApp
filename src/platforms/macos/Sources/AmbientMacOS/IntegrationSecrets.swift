import Dispatch
import Foundation
import Security

/// Printable ASCII of 1...max bytes: the form of every token and key Ambient stores.
func graphic(_ s: String, _ max: Int) -> Bool {
    !s.isEmpty && s.utf8.count <= max && s.utf8.allSatisfy { $0 >= 33 && $0 <= 126 }
}
/// A memory database instance: exactly 32 lowercase hex characters.
func validInstance(_ s: String) -> Bool {
    s.utf8.count == 32 && s.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
}
/// The secret form of each adapter; the envelope kind must name the same adapter.
enum IntegrationSecret: Codable, Equatable, Sendable {
    case githubToken(token: String)
    case jiraBasic(email: String, token: String)
    case todoistToken(token: String)
    enum CodingKeys: String, CodingKey {
        case githubToken = "github_token"
        case jiraBasic = "jira_basic"
        case todoistToken = "todoist_token"
    }
    enum GithubTokenCodingKeys: String, CodingKey { case token }
    enum JiraBasicCodingKeys: String, CodingKey { case email, token }
    /// A Todoist personal token has GitHub's single-token form.
    typealias TodoistTokenCodingKeys = GithubTokenCodingKeys
    var kind: IntegrationKind {
        switch self {
        case .githubToken: .github
        case .jiraBasic: .jira
        case .todoistToken: .todoist
        }
    }
    /// The two borrowed dispatch parts; an absent second part crosses the ABI as `{NULL, 0}`.
    var parts: (first: String, second: String?) {
        switch self {
        case .githubToken(let token), .todoistToken(let token): (token, nil)
        case .jiraBasic(let email, let token): (email, token)
        }
    }
    func validate() throws {
        switch self {
        case .githubToken(let token), .todoistToken(let token):
            guard graphic(token, 16384) else { throw IntegrationFailure.invalid }
        case .jiraBasic(let email, let token):
            guard graphic(email, 254), !email.utf8.contains(58), graphic(token, 16384) else {
                throw IntegrationFailure.invalid
            }
        }
    }
}
/// Keychain payload bound to one memory database instance, profile and credential generation.
struct IntegrationEnvelope: Codable, Equatable, Sendable {
    let version: Int
    let instance: String
    let kind: IntegrationKind
    let profileID: UInt64
    let generation: UInt64
    let secret: IntegrationSecret
    enum CodingKeys: String, CodingKey {
        case version, instance, kind, generation, secret
        case profileID = "profile_id"
    }
    func validate() throws {
        let ids = 1...UInt64(Int64.max)
        guard version == 1, validInstance(instance), ids.contains(profileID),
            ids.contains(generation), secret.kind == kind
        else { throw IntegrationFailure.invalid }
        try secret.validate()
    }
}
protocol IntegrationSecretStore: Sendable {
    func load(instance: String, profileID: UInt64) throws -> IntegrationEnvelope?
    func save(_ envelope: IntegrationEnvelope) throws
    func delete(instance: String, profileID: UInt64) throws
}
/// A Keychain envelope bound to one memory database instance and one record suffix.
protocol KeychainEnvelope: Codable, Sendable {
    var instance: String { get }
    /// The account suffix after `<instance>/`.
    var account: String { get }
    var generation: UInt64 { get }
    func validate() throws
}
/// Security synchronizes these APIs. The only shared reference is immutable; each secret owner
/// serializes logical mutations. One service per owner; the optional keychain confines tests.
/// Data is at most 64 KiB on write and read; other instances' records are never read.
final class EnvelopeKeychain<Envelope: KeychainEnvelope>: @unchecked Sendable {
    private let service: String
    private let keychain: SecKeychain?
    init(service: String, keychain: SecKeychain? = nil) {
        self.service = service
        self.keychain = keychain
    }
    private func query(_ instance: String, _ account: String, add: Bool = false)
        -> [String: Any]
    {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: "\(instance)/\(account)",
            kSecAttrSynchronizable as String: false,
        ]
        if let keychain {
            q[(add ? kSecUseKeychain : kSecMatchSearchList) as String] =
                add ? keychain : [keychain] as Any
        }
        return q
    }
    func load(instance: String, account: String) throws -> Envelope? {
        var q = query(instance, account)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data, data.count <= 65536 else {
            throw IntegrationFailure.storage
        }
        do {
            let e = try JSONDecoder().decode(Envelope.self, from: data)
            try e.validate()
            guard e.instance == instance, e.account == account else {
                throw IntegrationFailure.storage
            }
            return e
        } catch { throw IntegrationFailure.storage }
    }
    func save(_ envelope: Envelope) throws {
        try envelope.validate()
        let data = try JSONEncoder().encode(envelope)
        guard data.count <= 65536 else { throw IntegrationFailure.invalid }
        let status = SecItemUpdate(
            query(envelope.instance, envelope.account) as CFDictionary,
            [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var q = query(envelope.instance, envelope.account, add: true)
            q[kSecValueData as String] = data
            guard SecItemAdd(q as CFDictionary, nil) == errSecSuccess else {
                throw IntegrationFailure.storage
            }
        } else if status != errSecSuccess {
            throw IntegrationFailure.storage
        }
    }
    func delete(instance: String, account: String) throws {
        let status = SecItemDelete(query(instance, account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw IntegrationFailure.storage
        }
    }
}
extension IntegrationEnvelope: KeychainEnvelope {
    var account: String { "\(profileID)" }
}
final class IntegrationKeychain: IntegrationSecretStore {
    private let items: EnvelopeKeychain<IntegrationEnvelope>
    init(keychain: SecKeychain? = nil) {
        items = EnvelopeKeychain(service: "dev.ambient.foundation.integrations", keychain: keychain)
    }
    func load(instance: String, profileID: UInt64) throws -> IntegrationEnvelope? {
        try items.load(instance: instance, account: "\(profileID)")
    }
    func save(_ envelope: IntegrationEnvelope) throws { try items.save(envelope) }
    func delete(instance: String, profileID: UInt64) throws {
        try items.delete(instance: instance, account: "\(profileID)")
    }
}
/// The single native secret owner. Core changes first; Keychain follows; a secret is read only
/// for the current database instance, kind, profile and credential generation.
actor IntegrationSecretOwner {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.integration.secrets")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    private let memory: MemorySession
    private let store: any IntegrationSecretStore
    private let transport: IntegrationTransportBridge
    private var mutating = false
    init(
        memory: MemorySession, store: any IntegrationSecretStore = IntegrationKeychain(),
        transport: IntegrationTransportBridge = IntegrationTransportBridge()
    ) {
        self.memory = memory
        self.store = store
        self.transport = transport
    }
    /// Returns the current profile and saved envelope only when every binding still matches.
    private func current(_ p: IntegrationProfile) async throws -> (
        IntegrationProfile, IntegrationEnvelope?
    ) {
        let settings = try await memory.integrationSettings(p.kind)
        guard let current = settings.profiles.first(where: { $0.id == p.id }) else {
            throw ModelCheckFailure.stale
        }
        let saved = try store.load(instance: settings.instance, profileID: p.id)
        guard let saved, saved.kind == current.kind,
            saved.generation == current.credentialGeneration
        else { return (current, nil) }
        return (current, saved)
    }
    func hasCredential(_ p: IntegrationProfile) async throws -> Bool {
        guard !mutating else { throw ModelCheckFailure.busy }
        let (current, saved) = try await current(p)
        guard !mutating else { throw ModelCheckFailure.busy }
        return current.credentialGeneration == p.credentialGeneration && saved != nil
    }
    func replace(_ p: IntegrationProfile, secret: IntegrationSecret) async throws
        -> IntegrationProfile
    {
        guard secret.kind == p.kind else { throw IntegrationFailure.invalid }
        try secret.validate()
        guard !mutating else { throw ModelCheckFailure.busy }
        mutating = true
        defer { mutating = false }
        let instance = try await memory.integrationSettings(p.kind).instance
        let revoked = try await memory.beginIntegrationCredentialReplacement(p)
        try store.save(
            IntegrationEnvelope(
                version: 1, instance: instance, kind: p.kind, profileID: revoked.id,
                generation: revoked.credentialGeneration, secret: secret))
        let current = try await memory.integrationProfile(revoked.id, kind: p.kind)
        guard current.settingsRevision == revoked.settingsRevision,
            current.credentialGeneration == revoked.credentialGeneration
        else { throw ModelCheckFailure.stale }
        return current
    }
    func delete(_ p: IntegrationProfile) async throws {
        guard !mutating else { throw ModelCheckFailure.busy }
        mutating = true
        defer { mutating = false }
        let instance = try await memory.integrationSettings(p.kind).instance
        let revoked = try await memory.beginIntegrationCredentialReplacement(p)
        do {
            try store.delete(instance: instance, profileID: revoked.id)
            try await memory.finishIntegrationProfileDeletion(revoked)
        } catch { throw IntegrationFailure.deleteIncomplete }
    }
    func read<Object: Decodable & Equatable & Sendable, Coverage: Decodable & Equatable & Sendable>(
        _ op: IntegrationOperation
    ) async throws -> IntegrationWindow<Object, Coverage> {
        let envelope: IntegrationEnvelope
        do {
            guard !mutating else { throw ModelCheckFailure.busy }
            let (current, saved) = try await current(op.profile)
            guard current.settingsRevision == op.profile.settingsRevision,
                current.credentialGeneration == op.profile.credentialGeneration, let saved
            else { throw IntegrationFailure.invalid }
            envelope = saved
        } catch {
            try op.abort()
            throw error
        }
        try await transport.dispatch(op, secret: envelope.secret)
        return try IntegrationBridge.window(op)
    }
}
