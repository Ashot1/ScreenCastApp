import Dispatch
import Foundation
import Security

/// The secret of one plugin server: a personal token of a remote server, the variable values of
/// a local one (ADR-022 `env(map)`) or the Google sign-in of the managed server (ADR-021 `oauth`;
/// times are Unix seconds).
enum PluginSecret: Codable, Equatable, Sendable {
    case bearer(token: String)
    case env(values: [String: String])
    case oauth(access: String, refresh: String, expiresAt: Int64, refreshIssuedAt: Int64)
    enum CodingKeys: String, CodingKey { case bearer, env, oauth }
    enum BearerCodingKeys: String, CodingKey { case token }
    enum EnvCodingKeys: String, CodingKey { case values }
    enum OauthCodingKeys: String, CodingKey {
        case access, refresh
        case expiresAt = "expires_at"
        case refreshIssuedAt = "refresh_issued_at"
    }
    /// The text dispatch and discovery borrow: the token, or the compact JSON of the values with
    /// sorted names (printable ASCII only, so it passes the bridge's key check unchanged).
    func dispatchText() throws -> String {
        switch self {
        case .bearer(let token): return token
        case .oauth(let access, _, _, _): return access
        case .env(let values):
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
            guard let text = String(data: try encoder.encode(values), encoding: .utf8) else {
                throw IntegrationFailure.invalid
            }
            return text
        }
    }
    func validate() throws {
        switch self {
        case .bearer(let token):
            guard graphic(token, 16384) else { throw IntegrationFailure.invalid }
        case .oauth(let access, let refresh, let expiresAt, let issuedAt):
            guard graphic(access, 16384), graphic(refresh, 16384), expiresAt > 0, issuedAt > 0
            else { throw IntegrationFailure.invalid }
        case .env(let values):
            // Providers re-check after decoding; names and values are bounded here already.
            guard (1...16).contains(values.count),
                values.allSatisfy({ validEnvName($0.key) && graphic($0.value, 16384) }),
                try dispatchText().utf8.count <= 16384
            else { throw IntegrationFailure.invalid }
        }
    }
}
/// `^[A-Z_][A-Z0-9_]{0,63}$`, the package rule Core applied to the declared names.
private func validEnvName(_ name: String) -> Bool {
    let b = Array(name.utf8)
    guard let first = b.first, (1...64).contains(b.count), first == 95 || (65...90).contains(first)
    else { return false }
    return b.allSatisfy { $0 == 95 || (65...90).contains($0) || (48...57).contains($0) }
}
/// Keychain payload bound to one memory database instance, plugin, server and generation.
struct PluginEnvelope: KeychainEnvelope, Equatable {
    let version: Int
    let instance: String
    let pluginID: UInt64
    let server: String
    let generation: UInt64
    let secret: PluginSecret
    enum CodingKeys: String, CodingKey {
        case version, instance, server, generation, secret
        case pluginID = "plugin_id"
    }
    var account: String { Self.account(plugin: pluginID, server: server) }
    static func account(plugin: UInt64, server: String) -> String { "\(plugin)/\(server)" }
    func validate() throws {
        let ids = 1...UInt64(Int64.max)
        guard version == 1, validInstance(instance), ids.contains(pluginID),
            ids.contains(generation), (1...32).contains(server.utf8.count),
            server.utf8.allSatisfy({
                (97...122).contains($0) || (48...57).contains($0) || $0 == 45
            })
        else { throw IntegrationFailure.invalid }
        try secret.validate()
    }
}
extension EnvelopeKeychain where Envelope == PluginEnvelope {
    /// Plugin secrets: service `dev.ambient.foundation.plugins`, `<instance>/<plugin>/<server>`.
    static func plugins(keychain: SecKeychain? = nil) -> EnvelopeKeychain<PluginEnvelope> {
        EnvelopeKeychain(service: "dev.ambient.foundation.plugins", keychain: keychain)
    }
}
/// One revoke at Google through the bridge (up to 15 s); a failure is `false`, never a throw.
@Sendable private func revokeGoogle(_ id: String, _ token: String) async -> Bool {
    (try? await ManagedServers().revoke(id, token: token)) ?? false
}
/// One token refresh through the bridge, off the caller's actor (up to 15 s).
@Sendable private func refreshGoogle(_ id: String, _ token: String) async throws -> OAuthAnswer {
    try await ManagedServers().refresh(id, token: token)
}
/// The single native owner of plugin secrets. Core changes first, Keychain follows; a secret is
/// used only for the current database instance, plugin, server and generation.
actor PluginSecretOwner {
    /// One refresh of a managed server's sign-in: (server id, refresh token) → the answer.
    typealias Refresher = @Sendable (String, String) async throws -> OAuthAnswer
    /// One revoke of a managed server's refresh token at the provider: (server id, token).
    typealias Revoker = @Sendable (String, String) async -> Bool
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.plugin.secrets")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    private let memory: MemorySession
    private let store: EnvelopeKeychain<PluginEnvelope>
    private let refresher: Refresher
    private let revoker: Revoker
    private let now: @Sendable () -> Int64
    private var mutating = false
    /// Servers whose Google grant ended (`invalid_grant`, e.g. the 7-day testing mode); the key
    /// is revoked and the settings ask to sign in again.
    private(set) var expired: Set<String> = []
    init(
        memory: MemorySession, store: EnvelopeKeychain<PluginEnvelope> = .plugins(),
        refresher: @escaping Refresher = refreshGoogle,
        revoker: @escaping Revoker = revokeGoogle,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970) }
    ) {
        self.memory = memory
        self.store = store
        self.refresher = refresher
        self.revoker = revoker
        self.now = now
    }
    /// A Google access token expiring within 300 s (more than one request) is refreshed first.
    /// A refresh keeps the generation; it is refused while a revoke or replacement runs, and
    /// itself holds `mutating`, so a replacement never meets a refresh writing an older key.
    private func fresh(_ list: PluginList, _ p: PluginRecord, _ s: PluginServer) async throws
        -> PluginEnvelope?
    {
        guard let e = try saved(list, p, s) else { return nil }
        guard case .oauth(_, let refresh, let expiresAt, let issuedAt) = e.secret,
            let id = s.kind.managedID, expiresAt - now() < 300
        else { return e }
        guard !mutating else { throw ModelCheckFailure.busy }
        mutating = true
        defer { mutating = false }
        let answer: OAuthAnswer
        // A failed refresh keeps the current token (the server's 401 then revokes it); only a
        // cancellation ends the caller.
        do { answer = try await refresher(id, refresh) } catch is CancellationError {
            throw CancellationError()
        } catch { return e }
        switch answer.status {
        case .ok:
            guard let access = answer.accessToken, let expiresIn = answer.expiresIn else {
                return e
            }
            // Google does not rotate; a rotating server's new refresh token restarts its age.
            let secret = PluginSecret.oauth(
                access: access, refresh: answer.refreshToken ?? refresh,
                expiresAt: now() + expiresIn,
                refreshIssuedAt: answer.refreshToken == nil ? issuedAt : now())
            let next = PluginEnvelope(
                version: 1, instance: e.instance, pluginID: e.pluginID, server: e.server,
                generation: e.generation, secret: secret)
            try store.save(next)
            return next
        case .invalidGrant:
            // The grant is gone: the same revoke as sign-out, then «sign in again».
            _ = try await memory.revokePluginCredential(p, server: s.key)
            try store.delete(instance: list.instance, account: e.account)
            expired.insert(PluginEnvelope.account(plugin: p.id, server: s.key))
            return nil
        case .denied, .invalid, .failed, .unavailable: return e
        }
    }
    private func saved(_ list: PluginList, _ p: PluginRecord, _ s: PluginServer) throws
        -> PluginEnvelope?
    {
        let account = PluginEnvelope.account(plugin: p.id, server: s.key)
        guard let e = try store.load(instance: list.instance, account: account),
            e.generation == s.generation
        else { return nil }
        return e
    }
    /// Servers of this plugin that have a key of their current generation.
    func signedIn(_ p: PluginRecord) async throws -> Set<String> {
        guard !mutating else { throw ModelCheckFailure.busy }
        let list = try await memory.plugins()
        guard let current = list.plugins.first(where: { $0.id == p.id }) else { return [] }
        var out = Set<String>()
        for s in current.servers {
            if try saved(list, current, s) != nil { out.insert(s.key) }
        }
        return out
    }
    /// The key of exactly the given generation, for a blocking discovery.
    func key(_ p: PluginRecord, server: PluginServer) async throws -> String? {
        guard !mutating else { throw ModelCheckFailure.busy }
        let list = try await memory.plugins()
        return try await fresh(list, p, server)?.secret.dispatchText()
    }
    /// The Google sign-in of a managed server: Core moves the generation, then the Keychain holds
    /// the tokens for it.
    func signIn(_ p: PluginRecord, server: String, tokens: OAuthAnswer) async throws
        -> PluginRecord
    {
        guard tokens.status == .ok, let access = tokens.accessToken,
            let refresh = tokens.refreshToken, let expiresIn = tokens.expiresIn
        else { throw IntegrationFailure.invalid }
        let at = now()
        let current = try await replace(
            p, server: server,
            secret: .oauth(
                access: access, refresh: refresh, expiresAt: at + expiresIn, refreshIssuedAt: at))
        expired.remove(PluginEnvelope.account(plugin: p.id, server: server))
        return current
    }
    /// When the Google refresh token was issued, for the testing-mode notice (7 days).
    func signedInAt(_ p: PluginRecord, server: PluginServer) async throws -> Int64? {
        let list = try await memory.plugins()
        guard case .oauth(_, _, _, let issuedAt) = try saved(list, p, server)?.secret else {
            return nil
        }
        return issuedAt
    }
    func replace(_ p: PluginRecord, server: String, token: String) async throws -> PluginRecord {
        try await replace(p, server: server, secret: .bearer(token: token))
    }
    /// The values of a local server's variables: exactly the names Core declared, none extra.
    func replace(_ p: PluginRecord, server: String, values: [String: String]) async throws
        -> PluginRecord
    {
        guard let s = p.servers.first(where: { $0.key == server }),
            Set(values.keys) == Set(s.kind.envNames)
        else { throw IntegrationFailure.invalid }
        return try await replace(p, server: server, secret: .env(values: values))
    }
    /// Replacement: Core moves the generation first, then the Keychain holds the new key for it.
    private func replace(_ p: PluginRecord, server: String, secret: PluginSecret) async throws
        -> PluginRecord
    {
        try secret.validate()
        guard !mutating else { throw ModelCheckFailure.busy }
        mutating = true
        defer { mutating = false }
        let instance = try await memory.plugins().instance
        let generation = try await memory.revokePluginCredential(p, server: server)
        try store.save(
            PluginEnvelope(
                version: 1, instance: instance, pluginID: p.id, server: server,
                generation: generation, secret: secret))
        guard let current = try await memory.plugins().plugins.first(where: { $0.id == p.id }),
            current.servers.contains(where: { $0.key == server && $0.generation == generation })
        else { throw ModelCheckFailure.stale }
        return current
    }
    /// Revokes the Google grant of each managed server of `p` that holds a sign-in, before the
    /// local key goes (ADR-021: revoke first); `false` when any revoke was not confirmed.
    private func revokeAtProvider(_ list: PluginList, _ p: PluginRecord, servers: [String]) async
        -> Bool
    {
        var all = true
        for s in p.servers where servers.contains(s.key) {
            guard let id = s.kind.managedID else { continue }
            // An unreadable secret cannot be revoked: that is «not confirmed», not «nothing».
            let envelope: PluginEnvelope?
            do { envelope = try saved(list, p, s) } catch {
                all = false
                continue
            }
            guard case .oauth(_, let refresh, _, _) = envelope?.secret else { continue }
            if await !revoker(id, refresh) { all = false }
        }
        return all
    }
    /// Sign-out: the grant is revoked at Google when there is one (best effort), then Core and
    /// the Keychain drop the key whatever the network said. `revoked` is false when Google did
    /// not confirm.
    func signOut(_ p: PluginRecord, server: String) async throws -> (
        plugin: PluginRecord, revoked: Bool
    ) {
        guard !mutating else { throw ModelCheckFailure.busy }
        mutating = true
        defer { mutating = false }
        let before = try await memory.plugins()
        let revoked = await revokeAtProvider(before, p, servers: [server])
        let instance = before.instance
        _ = try await memory.revokePluginCredential(p, server: server)
        try store.delete(
            instance: instance, account: PluginEnvelope.account(plugin: p.id, server: server))
        let list = try await memory.plugins()
        guard let current = list.plugins.first(where: { $0.id == p.id }) else {
            throw ModelCheckFailure.stale
        }
        return (current, revoked)
    }
    /// Removal: Google grants are revoked (best effort), Core revokes every server, Keychain
    /// entries go, then Core deletes the plugin. Returns whether every Google revoke was confirmed.
    func delete(_ p: PluginRecord) async throws -> Bool {
        guard !mutating else { throw ModelCheckFailure.busy }
        mutating = true
        defer { mutating = false }
        let before = try await memory.plugins()
        let revoked = await revokeAtProvider(before, p, servers: p.servers.map(\.key))
        let instance = before.instance
        let revision = try await memory.beginPluginRemoval(p)
        do {
            for s in p.servers {
                try store.delete(
                    instance: instance, account: PluginEnvelope.account(plugin: p.id, server: s.key)
                )
            }
            try await memory.finishPluginRemoval(p.id, revision: revision)
            // After Core: the private folder of its local servers (caches, HOME) goes too.
            let folder = try await memory.pluginServersRoot().appending(
                path: String(p.id), directoryHint: .isDirectory)
            do { try FileManager.default.removeItem(at: folder) } catch CocoaError.fileNoSuchFile {}
        } catch { throw IntegrationFailure.deleteIncomplete }
        return revoked
    }
    /// Keys of enabled plugins' servers with a current-generation secret, for one dispatch.
    func dispatchKeys() async throws -> [PluginDispatchKey] {
        guard !mutating else { throw ModelCheckFailure.busy }
        let list = try await memory.plugins()
        var keys: [PluginDispatchKey] = []
        for p in list.plugins where p.enabled && !p.tools.isEmpty {
            for s in p.servers {
                if let e = try await fresh(list, p, s) {
                    keys.append(
                        PluginDispatchKey(
                            plugin: p.id, server: s.key, generation: s.generation,
                            key: try e.secret.dispatchText()))
                }
            }
        }
        // At most 16 plugins with 4 servers each: within the 64 keys of one dispatch.
        return keys
    }
}
