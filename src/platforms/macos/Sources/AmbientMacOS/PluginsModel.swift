import AppKit
import Observation
import SwiftUI

extension MemorySession {
    /// Reads and reviews a plugin folder off the UI actor; nothing is stored.
    func reviewPluginFolder(_ folder: URL) throws -> ([PluginPackageFile], PluginReview) {
        let files = try PluginPackageReader.read(folder: folder)
        return (files, try PluginBridge.review(files))
    }
}

/// Why a Google sign-in ended without tokens; each names its settings text.
enum GoogleSignIn: Error {
    case unavailable, denied, invalid, failed
    var key: String {
        switch self {
        case .unavailable: "plugins.google.signInUnavailable"
        case .denied: "plugins.google.signInDenied"
        case .invalid: "plugins.google.signInInvalid"
        case .failed: "plugins.google.signInFailed"
        }
    }
}
/// Native view state of user-installed plugins. Core owns every plugin record and decision; this
/// model only shows them and forwards user actions. Late answers of an older load are dropped.
@MainActor @Observable final class PluginsModel {
    struct Draft: Equatable {
        let files: [PluginPackageFile]
        let review: PluginReview
        var tokens: [String: String] = [:]
        /// Entered variable values of local servers, by server and name.
        var values: [String: [String: String]] = [:]
        var plugin: PluginRecord?
        /// The latest discovery of each server, as Core answered it.
        var discoveries: [String: Discovery] = [:]
        /// Checked tools as "server/name"; only approvable ones can be checked.
        var selected: Set<String> = []
    }
    private let memory: MemorySession
    let secrets: PluginSecretOwner
    private let discovery = PluginDiscovery()
    private let managed = ManagedServers()
    private let openBrowser: @MainActor @Sendable (URL) -> Bool
    private let codexHome: URL
    /// Install state of each managed server id, read from disk when plugins load.
    private(set) var managedStatus: [String: ManagedStatus] = [:]
    /// The running install, which «Cancel» stops; «installing» is UI state only (ADR-022).
    private(set) var installing: (id: String, operation: PluginOperation)?
    private(set) var installResult: ManagedInstall?
    /// The Google sign-in waiting for the browser; «Cancel» closes its loopback.
    private var signIn: ChatGPTLoopback?
    private(set) var signingIn = false
    /// When each managed server's sign-in was issued, by plugin and server (testing mode notice).
    private(set) var signedInAt: [UInt64: [String: Int64]] = [:]
    private(set) var expired: Set<String> = []
    /// The discovery that «Cancel» stops: a local program may run for up to 120 s.
    private(set) var running: (plugin: UInt64, server: String, operation: PluginOperation)?
    private var generation: UInt64 = 0
    private(set) var plugins: [PluginRecord] = []
    private(set) var signedIn: [UInt64: Set<String>] = [:]
    private(set) var calls: [PluginCall] = []
    /// Live definitions per installed plugin and server, rediscovered when settings open.
    private(set) var live: [UInt64: [String: Discovery]] = [:]
    private(set) var failureKey: String?
    private(set) var working = false
    var draft: Draft?
    init(
        memory: MemorySession, secrets: PluginSecretOwner,
        codexHome: URL = FileManager.default.homeDirectoryForCurrentUser,
        openBrowser: @escaping @MainActor @Sendable (URL) -> Bool = {
            NSWorkspace.shared.open($0)
        }
    ) {
        self.memory = memory
        self.secrets = secrets
        self.codexHome = codexHome
        self.openBrowser = openBrowser
    }
    func load() async {
        generation += 1
        let accepted = generation
        do {
            let list = try await memory.plugins().plugins
            var keys: [UInt64: Set<String>] = [:]
            for p in list { keys[p.id] = try await secrets.signedIn(p) }
            let calls = try await memory.pluginCalls()
            var issued: [UInt64: [String: Int64]] = [:]
            for p in list {
                for s in p.servers where s.kind.managedID != nil {
                    issued[p.id, default: [:]][s.key] = try await secrets.signedInAt(p, server: s)
                }
            }
            let expired = await secrets.expired
            guard accepted == generation else { return }
            plugins = list
            signedIn = keys
            signedInAt = issued
            self.expired = expired
            self.calls = calls
            await refreshManaged()
        } catch {
            guard accepted == generation else { return }
            fail(error)
        }
    }
    private func fail(_ error: any Error) {
        failureKey =
            switch error {
            case let e as ModelCheckFailure: e.key
            case PluginPackageReader.Failure.missing: "plugins.error.missing"
            case PluginPackageReader.Failure.tooLarge: "plugins.error.tooLarge"
            case IntegrationFailure.deleteIncomplete: "plugins.error.deleteIncomplete"
            case IntegrationFailure.invalid: "plugins.error.invalid"
            case ChatGPTFailure.cancelled: "plugins.google.signInCancelled"
            case let e as GoogleSignIn: e.key
            default: "error.storage"
            }
    }
    private func run(_ body: () async throws -> Void) async {
        guard !working else { return }
        working = true
        defer { working = false }
        do {
            try await body()
            failureKey = nil
        } catch { fail(error) }
        await load()
    }
    var codexFolders: [URL] { PluginPackageReader.codexPlugins(home: codexHome) }
    func chooseFolder() async {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.resolvesAliases = false
        guard await panel.begin() == .OK, let url = panel.url else { return }
        await open(url)
    }
    func open(_ folder: URL) async {
        await run {
            let (files, review) = try await memory.reviewPluginFolder(folder)
            draft = Draft(files: files, review: review)
        }
    }
    /// Install exactly the reviewed bytes, store entered keys and values and discover each remote
    /// server's tools. A local server starts only when the user presses «Run and show tools».
    func installDraft() async {
        guard var d = draft, d.plugin == nil else { return }
        await run {
            let id = try await memory.installPlugin(
                d.files, hash: d.review.hash, at: Int64(Date().timeIntervalSince1970))
            guard var p = try await memory.plugins().plugins.first(where: { $0.id == id }) else {
                throw ModelCheckFailure.stale
            }
            for s in p.servers {
                if s.kind.isLocal {
                    let values = d.values[s.key, default: [:]]
                    if s.needsKey, values.count == s.kind.envNames.count,
                        values.values.allSatisfy({ !$0.isEmpty })
                    {
                        p = try await secrets.replace(p, server: s.key, values: values)
                    }
                    continue
                }
                let token = d.tokens[s.key, default: ""].trimmingCharacters(in: .whitespaces)
                if !token.isEmpty { p = try await secrets.replace(p, server: s.key, token: token) }
            }
            d.plugin = p
            for s in p.servers where !s.kind.startsProcess {
                show(try await discover(p, s), server: s.key, in: &d)
            }
            draft = d
        }
    }
    private func show(_ found: Discovery, server: String, in d: inout Draft) {
        d.discoveries[server] = found
        for t in found.tools ?? [] where t.approvable { d.selected.insert("\(server)/\(t.name)") }
    }
    /// One discovery on its own queue; the operation is closed after actual return. Only the
    /// run of a local program is shown with «Cancel».
    private func discover(_ p: PluginRecord, _ s: PluginServer, cancellable: Bool = false)
        async throws -> Discovery
    {
        let key = try await secrets.key(p, server: s)
        let handle = try await memory.sourceMemoryHandle()
        let root = try await memory.pluginServersRoot()
        let operation = try PluginOperation.begin()
        defer { try? operation.close() }
        if cancellable { running = (p.id, s.key, operation) }
        defer { if cancellable { running = nil } }
        return try await discovery.discover(
            memory: handle, operation: operation, root: root, plugin: p.id, server: s.key,
            generation: s.generation, key: key)
    }
    /// «Run and show tools» for a local server of the plugin under review.
    func runDraft(server: String) async {
        guard var d = draft, let p = d.plugin, let s = p.servers.first(where: { $0.key == server })
        else { return }
        await run {
            show(try await discover(p, s, cancellable: true), server: server, in: &d)
            draft = d
        }
    }
    /// «Run and show tools» for an installed plugin: the result marks changed pins.
    func runInstalled(_ p: PluginRecord, server: String) async {
        guard let s = p.servers.first(where: { $0.key == server }) else { return }
        await run {
            let found = try await discover(p, s, cancellable: true)
            live[p.id, default: [:]][server] = found
        }
    }
    /// Requests a stop; the discovery returns after its program is stopped.
    func cancelRun() { running?.operation.cancel() }
    /// Pin the checked declared read-only tools, then enable the plugin.
    func enableDraft() async {
        guard let d = draft, var p = d.plugin else { return }
        await run {
            for (server, found) in d.discoveries {
                for t in found.tools ?? []
                where t.approvable && d.selected.contains("\(server)/\(t.name)") {
                    let revision = try await memory.approvePluginTool(p, server: server, tool: t)
                    p = try await current(p.id, revision: revision)
                }
            }
            try await memory.setPluginEnabled(p.id, enabled: true)
            draft = nil
        }
    }
    private func current(_ id: UInt64, revision: UInt64) async throws -> PluginRecord {
        guard let p = try await memory.plugins().plugins.first(where: { $0.id == id }),
            p.revision == revision
        else { throw ModelCheckFailure.stale }
        return p
    }
    func setEnabled(_ p: PluginRecord, enabled: Bool, chatGPT: ChatGPTModel) async {
        await run { try await memory.setPluginEnabled(p.id, enabled: enabled) }
        // Disabling cleared dependent assistance in Core; drop the process preview too.
        if !enabled { await chatGPT.resetAssistance() }
    }
    func saveToken(_ p: PluginRecord, server: String, token: String, chatGPT: ChatGPTModel) async {
        await run { _ = try await secrets.replace(p, server: server, token: token) }
        await chatGPT.resetAssistance()
    }
    func saveValues(
        _ p: PluginRecord, server: String, values: [String: String], chatGPT: ChatGPTModel
    ) async {
        await run { _ = try await secrets.replace(p, server: server, values: values) }
        await chatGPT.resetAssistance()
    }
    func signOut(_ p: PluginRecord, server: String, chatGPT: ChatGPTModel) async {
        var revoked = true
        await run { revoked = try await secrets.signOut(p, server: server).revoked }
        if !revoked { failureKey = "plugins.google.revokeFailed" }
        await chatGPT.resetAssistance()
    }
    func delete(_ p: PluginRecord, chatGPT: ChatGPTModel) async {
        let ids = p.servers.compactMap(\.kind.managedID)
        var revoked = true
        await run {
            // The managed server's folder goes too, so nothing may be using it (ADR-022).
            if !ids.isEmpty { try requireIdle(chatGPT) }
            revoked = try await secrets.delete(p)
            for id in ids { try await removeManagedFolder(id) }
        }
        if !revoked { failureKey = "plugins.google.revokeFailed" }
        live[p.id] = nil
        await chatGPT.resetAssistance()
    }

    // MARK: Google through the managed workspace-mcp server (ADR-022, D71)

    /// «Add Google»: the built-in package goes through the ordinary review and install.
    func addGoogle() async {
        await run {
            let files = try ManagedServers.builtin("google-workspace")
            draft = Draft(files: files, review: try PluginBridge.review(files))
        }
    }
    private func refreshManaged() async {
        guard let root = try? await memory.pluginServersRoot() else { return }
        var ids = Set(plugins.flatMap { $0.servers.compactMap(\.kind.managedID) })
        ids.formUnion(draft?.review.servers.compactMap(\.kind.managedID) ?? [])
        for id in ids { managedStatus[id] = try? ManagedServers.status(id, root: root) }
    }
    /// «Install» or «Update»: one cancellable blocking install with the user's uv.
    func installManaged(_ id: String) async {
        guard installing == nil else { return }
        installResult = nil
        do {
            // Claimed before the first suspension, so a second press cannot start a second install.
            let operation = try PluginOperation.begin()
            defer { try? operation.close() }
            installing = (id, operation)
            defer { installing = nil }
            let root = try await memory.pluginServersRoot()
            installResult = try await managed.install(id, root: root, operation: operation)
        } catch { fail(error) }
        await refreshManaged()
    }
    func cancelInstall() { installing?.operation.cancel() }
    /// «Remove server»: the whole managed folder, only while nothing uses it.
    func removeManaged(_ id: String, chatGPT: ChatGPTModel) async {
        await run {
            try requireIdle(chatGPT)
            try await removeManagedFolder(id)
        }
    }
    private func requireIdle(_ chatGPT: ChatGPTModel) throws {
        guard installing == nil, running == nil, !chatGPT.assisting, !chatGPT.checking else {
            throw ModelCheckFailure.busy
        }
    }
    private func removeManagedFolder(_ id: String) async throws {
        guard id == "google-workspace" else { throw IntegrationFailure.invalid }
        let folder = try await memory.pluginServersRoot().appending(
            path: "google-workspace-mcp", directoryHint: .isDirectory)
        do { try FileManager.default.removeItem(at: folder) } catch CocoaError.fileNoSuchFile {}
    }
    /// «Sign in with Google»: loopback 127.0.0.1, the browser, then Rust checks state and issuer
    /// and exchanges the code; the tokens go to the Keychain through the secret owner.
    func signInWithGoogle(_ p: PluginRecord, server: String) async {
        guard let id = p.servers.first(where: { $0.key == server })?.kind.managedID else { return }
        await run {
            let loopback = ChatGPTLoopback(path: "/")
            signIn = loopback
            signingIn = true
            defer {
                signIn = nil
                signingIn = false
            }
            do {
                let redirect = try await loopback.start()
                let begin = try await managed.begin(id, redirect: redirect)
                guard begin.status == .ok, let text = begin.url, let url = URL(string: text),
                    let state = begin.state, let verifier = begin.verifier
                else { throw GoogleSignIn.unavailable }
                guard openBrowser(url) else { throw GoogleSignIn.failed }
                let callback = try await loopback.next()
                await loopback.respond()
                await loopback.close()
                guard let query = URLComponents(string: callback)?.percentEncodedQuery else {
                    throw GoogleSignIn.invalid
                }
                let answer = try await managed.exchange(
                    id, redirect: redirect, query: query, state: state, verifier: verifier)
                switch answer.status {
                case .ok: _ = try await secrets.signIn(p, server: server, tokens: answer)
                case .denied: throw GoogleSignIn.denied
                case .invalid: throw GoogleSignIn.invalid
                case .failed, .unavailable, .invalidGrant: throw GoogleSignIn.failed
                }
            } catch {
                await loopback.close()
                throw error
            }
            // The review sheet shows the new sign-in state of the plugin it installed.
            if let current = try await memory.plugins().plugins.first(where: { $0.id == p.id }),
                draft?.plugin?.id == p.id
            {
                draft?.plugin = current
            }
        }
    }
    func cancelSignIn() { Task { await signIn?.close() } }
    /// Settings repeat discovery of remote servers and show a pinned tool whose live definition
    /// changed. A local server is never started here: only by «Run and show tools» or by use.
    func rediscover(_ p: PluginRecord) async {
        var found: [String: Discovery] = [:]
        for s in p.servers where !s.kind.startsProcess {
            found[s.key] = try? await discover(p, s)
        }
        live[p.id] = live[p.id, default: [:]].merging(found) { $1 }
    }
    func changed(_ p: PluginRecord, _ pin: PluginPin) -> Bool {
        guard let d = live[p.id]?[pin.server], d.status == .ok else { return false }
        return d.tools?.first(where: { $0.name == pin.name })?.hash != pin.hash
    }
    /// Re-pinning a changed tool after review, or removing its approval.
    func repin(_ p: PluginRecord, _ pin: PluginPin, chatGPT: ChatGPTModel) async {
        guard let tool = live[p.id]?[pin.server]?.tools?.first(where: { $0.name == pin.name }),
            tool.approvable
        else {
            await unapprove(p, pin, chatGPT: chatGPT)
            return
        }
        await run { _ = try await memory.approvePluginTool(p, server: pin.server, tool: tool) }
        await chatGPT.resetAssistance()
    }
    func unapprove(_ p: PluginRecord, _ pin: PluginPin, chatGPT: ChatGPTModel) async {
        await run {
            _ = try await memory.unapprovePluginTool(p, server: pin.server, tool: pin.name)
        }
        await chatGPT.resetAssistance()
    }
}
