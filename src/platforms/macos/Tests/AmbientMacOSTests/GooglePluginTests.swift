import CryptoKit
import Foundation
import Testing

@testable import AmbientMacOS

private let google = "google-workspace"
/// The canonical pinned definition Core accepts for one declared read-only tool, and its hash.
private func pinnedTool(_ name: String) -> DiscoveredTool? {
    let definition =
        #"{"description":"\#(name)","inputSchema":{"type":"object"},"name":"\#(name)","readOnlyHint":true}"#
    let hash = SHA256.hash(data: Data(definition.utf8)).map { String(format: "%02x", $0) }
        .joined()
    let json: [String: Any] = [
        "name": name, "hash": hash, "read_only": true, "approvable": true, "definition": definition,
    ]
    return (try? JSONSerialization.data(withJSONObject: json)).flatMap {
        try? JSONDecoder().decode(DiscoveredTool.self, from: $0)
    }
}
/// The built-in Google plugin installed through the review, one tool pinned, enabled.
@MainActor private func installedGoogle(_ memory: MemorySession, _ plugins: PluginsModel)
    async throws -> PluginRecord
{
    await plugins.addGoogle()
    await plugins.installDraft()
    let p = try #require(plugins.draft?.plugin)
    let tool = try #require(pinnedTool("search_docs"))
    let revision = try await memory.approvePluginTool(p, server: "google", tool: tool)
    #expect(revision == p.revision)
    try await memory.setPluginEnabled(p.id, enabled: true)
    plugins.draft = nil
    return try #require(try await memory.plugins().plugins.first { $0.id == p.id })
}
private func tokens(_ access: String, refresh: String?, expiresIn: Int64) throws -> OAuthAnswer {
    var json: [String: Any] = ["status": "ok", "access_token": access, "expires_in": expiresIn]
    if let refresh { json["refresh_token"] = refresh }
    return try JSONDecoder().decode(
        OAuthAnswer.self, from: JSONSerialization.data(withJSONObject: json))
}
/// No ChatGPT session: these tests never dispatch a model request.
private struct NoChatGPT: ChatGPTCredentialStore {
    func load() -> ChatGPTEnvelope? { nil }
    func save(_ envelope: ChatGPTEnvelope) {}
}
/// The refresh answers a test hands out, one per call.
private actor Answers {
    private var queue: [OAuthAnswer]
    private(set) var calls: [String] = []
    init(_ queue: [OAuthAnswer]) { self.queue = queue }
    func next(_ token: String) throws -> OAuthAnswer {
        calls.append(token)
        guard !queue.isEmpty else { throw IntegrationFailure.invalid }
        return queue.removeFirst()
    }
}

@Test func theGoogleSignInIsAnOauthSecretWithExplicitKeys() throws {
    let secret = PluginSecret.oauth(
        access: "ya29.a", refresh: "1//r", expiresAt: 1_800_000_000, refreshIssuedAt: 1_799_996_400)
    try secret.validate()
    #expect(try secret.dispatchText() == "ya29.a")
    let e = PluginEnvelope(
        version: 1, instance: String(repeating: "a", count: 32), pluginID: 3, server: "google",
        generation: 4, secret: secret)
    #expect(try JSONDecoder().decode(PluginEnvelope.self, from: JSONEncoder().encode(e)) == e)
    let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(e)) as? [String: Any]
    let oauth = (json?["secret"] as? [String: Any])?["oauth"] as? [String: Any]
    let names = Set(oauth?.keys.map { $0 } ?? [])
    #expect(names == ["access", "refresh", "expires_at", "refresh_issued_at"])
    for bad in [
        PluginSecret.oauth(access: "", refresh: "r", expiresAt: 1, refreshIssuedAt: 1),
        .oauth(access: "a b", refresh: "r", expiresAt: 1, refreshIssuedAt: 1),
        .oauth(access: "a", refresh: "r", expiresAt: 0, refreshIssuedAt: 1),
    ] {
        #expect(throws: IntegrationFailure.invalid) { try bad.validate() }
    }
}

@Test @MainActor func addGoogleReviewsTheBuiltInPackageAndInstallStartsNothing() async throws {
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let memory = MemorySession(databaseURL: fixture.databaseURL())
    let secrets = PluginSecretOwner(
        memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain))
    let plugins = PluginsModel(memory: memory, secrets: secrets)
    await plugins.addGoogle()
    #expect(plugins.failureKey == nil)
    let spec = try #require(plugins.draft?.review.servers.first)
    #expect(spec.kind == .managed(id: google))
    #expect(spec.needsKey && !spec.downloads && spec.kind.startsProcess)
    #expect(plugins.draft?.review.warnings.isEmpty == true)
    // A fresh plugins root: either uv is missing or the server is not installed yet.
    let status = try #require(plugins.managedStatus[google])
    #expect(status == .notInstalled || status == .uvMissing)
    await plugins.installDraft()
    let p = try #require(plugins.draft?.plugin)
    // Installing the plugin neither installs nor starts the server, and lists nothing.
    #expect(plugins.draft?.discoveries.isEmpty == true)
    let root = try await memory.pluginServersRoot()
    #expect(!FileManager.default.fileExists(atPath: root.appending(path: "\(p.id)").path))
    let server = root.appending(path: "google-workspace-mcp")
    #expect(!FileManager.default.fileExists(atPath: server.path))
    await plugins.load()
    await plugins.rediscover(try #require(plugins.plugins.first))
    #expect(plugins.live[p.id]?.isEmpty != false)
    try await memory.close()
}

@Test @MainActor func aTokenNearExpiryIsRefreshedForTheDispatchWithoutANewGeneration()
    async throws
{
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let memory = MemorySession(databaseURL: fixture.databaseURL())
    let answers = Answers([
        try tokens("ya29.second", refresh: nil, expiresIn: 3599),
        try JSONDecoder().decode(OAuthAnswer.self, from: Data(#"{"status":"invalid_grant"}"#.utf8)),
    ])
    let now: Int64 = 1_800_000_000
    let secrets = PluginSecretOwner(
        memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain),
        refresher: { id, token in
            #expect(id == google)
            return try await answers.next(token)
        }, now: { now })
    let plugins = PluginsModel(memory: memory, secrets: secrets)
    let p = try await installedGoogle(memory, plugins)
    // Signed in with a token that expires within the 300 s margin of a request.
    let signed = try await secrets.signIn(
        p, server: "google",
        tokens: try tokens("ya29.first", refresh: "1//refresh", expiresIn: 200))
    let generation = try #require(signed.servers.first?.generation)
    #expect(generation != p.servers.first?.generation)
    let keys = try await secrets.dispatchKeys()
    #expect(keys.map(\.key) == ["ya29.second"])
    #expect(keys.map(\.generation) == [generation], "a refresh keeps the generation")
    #expect(await answers.calls == ["1//refresh"])
    // Fresh now: no second refresh for the next request.
    #expect(try await secrets.dispatchKeys().map(\.key) == ["ya29.second"])
    #expect(await answers.calls.count == 1)
    #expect(
        try await secrets.signedInAt(signed, server: try #require(signed.servers.first)) == now)
    // Tokens never reach SQLite.
    let bytes = try Data(contentsOf: fixture.databaseURL())
    for t in ["ya29.first", "ya29.second", "1//refresh"] {
        #expect(bytes.range(of: Data(t.utf8)) == nil)
    }
    // An hour later the testing grant has ended: the key is revoked, «sign in again».
    let later = PluginSecretOwner(
        memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain),
        refresher: { _, token in try await answers.next(token) }, now: { now + 3600 })
    #expect(try await later.dispatchKeys().isEmpty)
    #expect(await later.expired == [PluginEnvelope.account(plugin: p.id, server: "google")])
    let after = try #require(try await memory.plugins().plugins.first { $0.id == p.id })
    #expect(after.servers.first?.generation != generation)
    #expect(try await later.signedIn(after).isEmpty)
    try await memory.close()
}

@Test @MainActor func removingTheGooglePluginRemovesTheServerFolderOnlyWhenIdle() async throws {
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let memory = MemorySession(databaseURL: fixture.databaseURL())
    let secrets = PluginSecretOwner(
        memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain))
    let chat = ChatGPTModel(
        coordinator: ChatGPTCoordinator(store: NoChatGPT(), memory: memory, plugins: secrets))
    let plugins = chat.plugins
    let p = try await installedGoogle(memory, plugins)
    let folder = try await memory.pluginServersRoot().appending(path: "google-workspace-mcp")
    try FileManager.default.createDirectory(
        at: folder.appending(path: "python"), withIntermediateDirectories: true)
    await plugins.load()
    await plugins.removeManaged(google, chatGPT: chat)
    #expect(plugins.failureKey == nil)
    #expect(!FileManager.default.fileExists(atPath: folder.path))
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    await plugins.delete(p, chatGPT: chat)
    #expect(plugins.failureKey == nil)
    #expect(try await memory.plugins().plugins.isEmpty)
    #expect(!FileManager.default.fileExists(atPath: folder.path))
    try await memory.close()
}

#if AMBIENT_MODEL_TEST_SUPPORT
    /// The browser step is simulated by calling the loopback redirect with a callback whose
    /// state is not this sign-in's: Rust refuses it before any token request.
    @Test @MainActor func aGoogleCallbackWithAnotherStateIsRefusedWithoutNetworkOrKeychain()
        async throws
    {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let secrets = PluginSecretOwner(
            memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain))
        let opened = Opened()
        let plugins = PluginsModel(
            memory: memory, secrets: secrets,
            openBrowser: { url in
                let redirect = URLComponents(url: url, resolvingAgainstBaseURL: false)?
                    .queryItems?.first { $0.name == "redirect_uri" }?.value
                Task { await opened.call(url, redirect) }
                return true
            })
        let p = try await installedGoogle(memory, plugins)
        await plugins.signInWithGoogle(p, server: "google")
        #expect(plugins.failureKey == "plugins.google.signInInvalid")
        let url = try #require(await opened.url)
        #expect(url.host == "accounts.google.com")
        #expect(await opened.redirect?.hasPrefix("http://127.0.0.1:") == true)
        #expect(try await secrets.signedIn(p).isEmpty)
        try await memory.close()
    }
    private actor Opened {
        private(set) var url: URL?
        private(set) var redirect: String?
        func call(_ url: URL, _ redirect: String?) async {
            self.url = url
            self.redirect = redirect
            let query = "state=not-this-one&code=c&iss=https%3A%2F%2Faccounts.google.com"
            guard let redirect, let callback = URL(string: "\(redirect)?\(query)") else { return }
            _ = try? await URLSession.shared.data(from: callback)
        }
    }
#endif

/// Records each revoke at Google and answers with `confirm`.
private actor Revokes {
    private(set) var tokens: [String] = []
    let confirm: Bool
    init(confirm: Bool) { self.confirm = confirm }
    func revoke(_ token: String) -> Bool {
        tokens.append(token)
        return confirm
    }
}

@Test @MainActor func signOutAndRemovalRevokeAtGoogleAndAlwaysDropTheLocalKey() async throws {
    for confirm in [true, false] {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let revokes = Revokes(confirm: confirm)
        let secrets = PluginSecretOwner(
            memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain),
            revoker: { id, token in
                #expect(id == google)
                return await revokes.revoke(token)
            })
        let chat = ChatGPTModel(
            coordinator: ChatGPTCoordinator(store: NoChatGPT(), memory: memory, plugins: secrets))
        let plugins = PluginsModel(memory: memory, secrets: secrets)
        let p = try await installedGoogle(memory, plugins)
        let signed = try await secrets.signIn(
            p, server: "google",
            tokens: try tokens("ya29.a", refresh: "1//to-revoke", expiresIn: 3599))
        // Sign-out: Google first, then Core and the Keychain whatever Google answered.
        await plugins.signOut(signed, server: "google", chatGPT: chat)
        #expect(await revokes.tokens == ["1//to-revoke"])
        #expect(plugins.failureKey == (confirm ? nil : "plugins.google.revokeFailed"))
        let out = try #require(try await memory.plugins().plugins.first { $0.id == p.id })
        #expect(try await secrets.signedIn(out).isEmpty)
        #expect(out.servers.first?.generation != signed.servers.first?.generation)
        // Removal of a signed-in plugin revokes too; a plugin without a sign-in revokes nothing.
        let again = try await secrets.signIn(
            out, server: "google",
            tokens: try tokens("ya29.b", refresh: "1//second", expiresIn: 3599))
        await plugins.load()
        await plugins.delete(again, chatGPT: chat)
        #expect(await revokes.tokens == ["1//to-revoke", "1//second"])
        #expect(plugins.failureKey == (confirm ? nil : "plugins.google.revokeFailed"))
        #expect(try await memory.plugins().plugins.isEmpty)
        try await memory.close()
    }
}
