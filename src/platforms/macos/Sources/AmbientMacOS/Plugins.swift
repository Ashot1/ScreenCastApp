import CAmbient
import Darwin
import Dispatch
import Foundation

/// One package file read natively; Core parses and validates its content.
struct PluginPackageFile: Equatable, Sendable {
    enum Kind: UInt32, Sendable {
        case manifest = 1
        case mcp = 2
        case skill = 3
    }
    let kind: Kind
    let path: String
    let bytes: [UInt8]
}
/// How a server is reached, exactly as Core stored it; Swift decides nothing from it.
enum PluginServerKind: Decodable, Equatable, Sendable {
    case remote(url: String)
    /// A command on this Mac with the names (not values) of its environment variables.
    case local(command: String, args: [String], env: [String])
    /// A server Ambient installs and runs itself (only the built-in Google package names one).
    case managed(id: String)
    enum CodingKeys: String, CodingKey { case kind, url, command, args, env, id }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "remote": self = .remote(url: try c.decode(String.self, forKey: .url))
        case "local":
            self = .local(
                command: try c.decode(String.self, forKey: .command),
                args: try c.decode([String].self, forKey: .args),
                env: try c.decode([String].self, forKey: .env))
        case "managed": self = .managed(id: try c.decode(String.self, forKey: .id))
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .kind, in: c, debugDescription: "unknown server kind")
        }
    }
    var envNames: [String] {
        if case .local(_, _, let env) = self { env } else { [] }
    }
    var isLocal: Bool { if case .local = self { true } else { false } }
    var managedID: String? { if case .managed(let id) = self { id } else { nil } }
    /// Discovery starts a program: only on the user's «Run and show tools», never by itself.
    var startsProcess: Bool { if case .remote = self { false } else { true } }
}
/// One server of a review: Core's derived facts (`needs_key`, `downloads`) are decoded, never
/// recomputed.
struct PluginServerSpec: Decodable, Equatable, Sendable {
    let key: String
    let kind: PluginServerKind
    let needsKey: Bool
    let downloads: Bool
    enum CodingKeys: String, CodingKey {
        case key, kind, downloads
        case needsKey = "needs_key"
    }
}
struct PluginSkill: Decodable, Equatable, Sendable {
    let name: String
    let description: String
    let text: String
}
struct PluginReview: Decodable, Equatable, Sendable {
    let name: String
    let version: String
    let hash: String
    let servers: [PluginServerSpec]
    let skills: [PluginSkill]
    let warnings: [String]
}
struct PluginServer: Decodable, Equatable, Sendable {
    let key: String
    let kind: PluginServerKind
    let needsKey: Bool
    let downloads: Bool
    let generation: UInt64
    enum CodingKeys: String, CodingKey {
        case key, kind, downloads, generation
        case needsKey = "needs_key"
    }
}
struct PluginPin: Decodable, Equatable, Sendable {
    let server: String
    let name: String
    let hash: String
}
struct PluginRecord: Decodable, Equatable, Sendable, Identifiable {
    let id: UInt64
    let revision: UInt64
    let enabled: Bool
    let name: String
    let version: String
    let hash: String
    let installedAtUTC: Int64
    let servers: [PluginServer]
    let skills: [PluginSkill]
    let tools: [PluginPin]
    enum CodingKeys: String, CodingKey {
        case id, revision, enabled, name, version, hash, servers, skills, tools
        case installedAtUTC = "installed_at_utc"
    }
}
struct PluginList: Decodable, Equatable, Sendable {
    let instance: String
    let plugins: [PluginRecord]
}
/// A tool as the server lists it now; only approvable tools carry their exact definition.
struct DiscoveredTool: Decodable, Equatable, Sendable, Identifiable {
    let name: String
    let hash: String
    let readOnly: Bool
    let approvable: Bool
    let description: String?
    let definition: String?
    var id: String { name }
    enum CodingKeys: String, CodingKey {
        case name, hash, approvable, description, definition
        case readOnly = "read_only"
    }
}
enum DiscoveryStatus: String, Decodable, Sendable {
    case ok
    case loginRequired = "login_required"
    case notConfigured = "not_configured"
    case unavailable, invalid, timeout, cancelled
}
/// A local server's failure carries its redacted stderr tail (shown only here, never logged).
struct Discovery: Decodable, Equatable, Sendable {
    let status: DiscoveryStatus
    let tools: [DiscoveredTool]?
    let stderr: String?
    let stopConfirmed: Bool
    enum CodingKeys: String, CodingKey {
        case status, tools, stderr
        case stopConfirmed = "stop_confirmed"
    }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        status = try c.decode(DiscoveryStatus.self, forKey: .status)
        tools = try c.decodeIfPresent([DiscoveredTool].self, forKey: .tools)
        stderr = try c.decodeIfPresent(String.self, forKey: .stderr)
        stopConfirmed = try c.decodeIfPresent(Bool.self, forKey: .stopConfirmed) ?? true
    }
}
/// One cancellable blocking plugin operation (ambient_plugin.h v2). Cancel is a request: the
/// blocking call returns after its local processes are stopped; close follows that return.
struct PluginOperation: Sendable {
    let id: UInt64
    static func begin() throws -> PluginOperation {
        var id: UInt64 = 0
        try IntegrationBridge.check(ambient_plugin_v2_operation_begin(&id))
        return PluginOperation(id: id)
    }
    func cancel() { _ = ambient_plugin_v2_operation_cancel(id) }
    func close() throws { try IntegrationBridge.check(ambient_plugin_v2_operation_close(id)) }
}
struct PluginCall: Decodable, Equatable, Sendable, Identifiable {
    let id: Int64
    let atUTC: Int64
    let pluginID: UInt64
    let pluginName: String
    let server: String
    let tool: String
    let arguments: String
    let stage: Int
    let outcome: UInt32
    let resultBytes: Int
    enum CodingKeys: String, CodingKey {
        case id, server, tool, arguments, stage, outcome
        case atUTC = "at_utc"
        case pluginID = "plugin_id"
        case pluginName = "plugin_name"
        case resultBytes = "result_bytes"
    }
}
/// One key passed to the model dispatch; borrowed for that call only.
struct PluginDispatchKey: Equatable, Sendable {
    let plugin: UInt64
    let server: String
    let generation: UInt64
    let key: String
}

/// Discovery blocks for up to 120 s when a local program starts for it, so it runs on its own
/// queue and never holds the memory actor; the bridge locks the Store only for Core's short steps.
actor PluginDiscovery {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.plugin.discovery")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    /// Lists one server's tools with the key of exactly this generation; stores nothing.
    func discover(
        memory: UInt64, operation: PluginOperation, root: URL, plugin: UInt64, server: String,
        generation: UInt64, key: String?
    ) throws -> Discovery {
        let texts = [Array(root.path.utf8), Array(server.utf8), Array((key ?? "").utf8)]
        let data = try PluginBridge.withTexts(texts) { t in
            try IntegrationBridge.json(capacity: 524_288) {
                ambient_plugin_v2_discover(
                    memory, operation.id, t[0], plugin, t[1], generation, t[2], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(Discovery.self, from: data)
    }
}
/// The managed server's install state on disk (Core-independent, from providers).
enum ManagedStatus: String, Decodable, Sendable {
    case notInstalled = "not_installed"
    case installed, outdated
    case uvMissing = "uv_missing"
}
struct ManagedInstall: Decodable, Equatable, Sendable {
    enum Status: String, Decodable, Sendable {
        case installed, failed, cancelled, timeout
        case hashMismatch = "hash_mismatch"
        case uvMissing = "uv_missing"
    }
    let status: Status
    let stderrTail: String?
    enum CodingKeys: String, CodingKey {
        case status
        case stderrTail = "stderr_tail"
    }
}
/// Every status the stateless Google sign-in calls answer with (ambient_plugin.h v2).
enum OAuthStatus: String, Decodable, Sendable {
    case ok, denied, invalid, failed, unavailable
    case invalidGrant = "invalid_grant"
}
/// The answers of the stateless Google sign-in calls; tokens go straight to the Keychain.
struct OAuthBegin: Decodable, Sendable {
    let status: OAuthStatus
    let url: String?
    let state: String?
    let verifier: String?
}
struct OAuthAnswer: Decodable, Sendable {
    let status: OAuthStatus
    let accessToken: String?
    let refreshToken: String?
    let expiresIn: Int64?
    enum CodingKeys: String, CodingKey {
        case status
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case expiresIn = "expires_in"
    }
}
/// The managed server and Google sign-in calls of ambient_plugin.h v2. The install blocks for up
/// to 10 minutes and token requests for up to 15 s, so each runs on this actor's own queue.
actor ManagedServers {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.plugin.managed")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    /// The built-in package bytes «Add Google» reviews like any folder.
    nonisolated static func builtin(_ id: String) throws -> [PluginPackageFile] {
        struct Files: Decodable {
            struct File: Decodable {
                let kind: UInt32
                let path: String
                let text: String
            }
            let files: [File]
        }
        let data = try PluginBridge.withTexts([Array(id.utf8)]) { t in
            try IntegrationBridge.json(capacity: 262_144) {
                ambient_plugin_v2_builtin(t[0], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(Files.self, from: data).files.map {
            guard let kind = PluginPackageFile.Kind(rawValue: $0.kind) else {
                throw IntegrationFailure.invalid
            }
            return PluginPackageFile(kind: kind, path: $0.path, bytes: Array($0.text.utf8))
        }
    }
    nonisolated static func status(_ id: String, root: URL) throws -> ManagedStatus {
        struct Answer: Decodable { let status: ManagedStatus }
        let data = try PluginBridge.withTexts([Array(root.path.utf8), Array(id.utf8)]) { t in
            try IntegrationBridge.json(capacity: 1024) {
                ambient_plugin_v2_managed_status(t[0], t[1], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(Answer.self, from: data).status
    }
    func install(_ id: String, root: URL, operation: PluginOperation) throws -> ManagedInstall {
        let data = try PluginBridge.withTexts([Array(root.path.utf8), Array(id.utf8)]) { t in
            try IntegrationBridge.json(capacity: 65_536) {
                ambient_plugin_v2_managed_install(operation.id, t[0], t[1], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(ManagedInstall.self, from: data)
    }
    func begin(_ id: String, redirect: String) throws -> OAuthBegin {
        let data = try PluginBridge.withTexts([Array(id.utf8), Array(redirect.utf8)]) { t in
            try IntegrationBridge.json(capacity: 65_536) {
                ambient_plugin_v2_oauth_begin(t[0], t[1], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(OAuthBegin.self, from: data)
    }
    func exchange(
        _ id: String, redirect: String, query: String, state: String, verifier: String
    ) throws -> OAuthAnswer {
        let texts = [id, redirect, query, state, verifier].map { Array($0.utf8) }
        let data = try PluginBridge.withTexts(texts) { t in
            try IntegrationBridge.json(capacity: 65_536) {
                ambient_plugin_v2_oauth_exchange(t[0], t[1], t[2], t[3], t[4], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(OAuthAnswer.self, from: data)
    }
    /// Best-effort revoke at Google on sign-out and removal; `true` when Google confirmed it.
    func revoke(_ id: String, token: String) throws -> Bool {
        struct Answer: Decodable { let status: String }
        let data = try PluginBridge.withTexts([Array(id.utf8), Array(token.utf8)]) { t in
            try IntegrationBridge.json(capacity: 1024) {
                ambient_plugin_v2_oauth_revoke(t[0], t[1], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(Answer.self, from: data).status == "revoked"
    }
    func refresh(_ id: String, token: String) throws -> OAuthAnswer {
        let data = try PluginBridge.withTexts([Array(id.utf8), Array(token.utf8)]) { t in
            try IntegrationBridge.json(capacity: 65_536) {
                ambient_plugin_v2_oauth_refresh(t[0], t[1], $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(OAuthAnswer.self, from: data)
    }
}
enum PluginBridge {
    /// Copies texts into buffers that stay valid for exactly one synchronous C call.
    static func withTexts<T>(_ texts: [[UInt8]], _ body: ([AmbientText]) throws -> T) rethrows
        -> T
    {
        let buffers = texts.map { bytes -> UnsafeMutableBufferPointer<UInt8>? in
            guard !bytes.isEmpty else { return nil }
            let b = UnsafeMutableBufferPointer<UInt8>.allocate(capacity: bytes.count)
            _ = b.initialize(from: bytes)
            return b
        }
        defer { for b in buffers { b?.deallocate() } }
        return try body(
            zip(buffers, texts).map {
                // An empty text crosses as {NULL, 0}.
                AmbientText(
                    bytes: $0.0.map { UnsafePointer($0.baseAddress) } ?? nil,
                    length: UInt32($0.1.count))
            })
    }
    static func withFiles<T>(
        _ files: [PluginPackageFile],
        _ body: (UnsafePointer<AmbientPluginFile>?, UInt32) throws -> T
    ) rethrows -> T {
        try withTexts(files.flatMap { [Array($0.path.utf8), $0.bytes] }) { texts in
            let raw = files.indices.map {
                AmbientPluginFile(
                    kind: files[$0].kind.rawValue, reserved: 0, path: texts[2 * $0],
                    bytes: texts[2 * $0 + 1])
            }
            return try raw.withUnsafeBufferPointer { try body($0.baseAddress, UInt32($0.count)) }
        }
    }
    static func review(_ files: [PluginPackageFile]) throws -> PluginReview {
        let data = try withFiles(files) { p, n in
            try IntegrationBridge.json(capacity: 262_144) {
                ambient_plugin_v2_review(p, n, $0, $1, $2)
            }
        }
        return try JSONDecoder().decode(PluginReview.self, from: data)
    }
}
extension MemorySession {
    func plugins() throws -> PluginList {
        let h = try sourceMemoryHandle()
        return try JSONDecoder().decode(
            PluginList.self,
            from: IntegrationBridge.json(capacity: 1_048_576) {
                ambient_plugin_v2_list(h, $0, $1, $2)
            })
    }
    func pluginCalls() throws -> [PluginCall] {
        let h = try sourceMemoryHandle()
        return try JSONDecoder().decode(
            [PluginCall].self,
            from: IntegrationBridge.json(capacity: 786_432) {
                ambient_plugin_v2_calls(h, $0, $1, $2)
            })
    }
    func installPlugin(_ files: [PluginPackageFile], hash: String, at: Int64) throws -> UInt64 {
        let h = try sourceMemoryHandle()
        var id: UInt64 = 0
        try PluginBridge.withFiles(files) { p, n in
            try LocalSourceBridge.withText(hash) {
                try IntegrationBridge.check(ambient_plugin_v2_install(h, p, n, $0, at, &id))
            }
        }
        return id
    }
    /// The private folders of local plugin servers sit next to the memory database.
    func pluginServersRoot() throws -> URL {
        try sourceDatabaseURL().deletingLastPathComponent().appending(
            path: "plugins", directoryHint: .isDirectory)
    }
    func approvePluginTool(_ p: PluginRecord, server: String, tool: DiscoveredTool) throws
        -> UInt64
    {
        guard let definition = tool.definition, tool.approvable else {
            throw IntegrationFailure.invalid
        }
        let h = try sourceMemoryHandle()
        var revision: UInt64 = 0
        let texts = [Array(server.utf8), Array(definition.utf8), Array(tool.hash.utf8)]
        try PluginBridge.withTexts(texts) { t in
            try IntegrationBridge.check(
                ambient_plugin_v2_approve(h, p.id, p.revision, t[0], t[1], t[2], &revision))
        }
        return revision
    }
    func unapprovePluginTool(_ p: PluginRecord, server: String, tool: String) throws -> UInt64 {
        let h = try sourceMemoryHandle()
        var revision: UInt64 = 0
        try PluginBridge.withTexts([Array(server.utf8), Array(tool.utf8)]) { t in
            try IntegrationBridge.check(
                ambient_plugin_v2_unapprove(h, p.id, p.revision, t[0], t[1], &revision))
        }
        return revision
    }
    func setPluginEnabled(_ id: UInt64, enabled: Bool) async throws {
        return try await withObservationMutation {
            let h = try sourceMemoryHandle()
            try IntegrationBridge.check(ambient_plugin_v2_set_enabled(h, id, enabled ? 1 : 0))

        }
    }
    /// Core first: the new generation is the one native writes into the Keychain envelope.
    func revokePluginCredential(_ p: PluginRecord, server: String) throws -> UInt64 {
        let h = try sourceMemoryHandle()
        var revision: UInt64 = 0
        try LocalSourceBridge.withText(server) {
            try IntegrationBridge.check(
                ambient_plugin_v2_revoke(h, p.id, p.revision, $0, &revision))
        }
        return revision
    }
    func beginPluginRemoval(_ p: PluginRecord) async throws -> UInt64 {
        return try await withObservationMutation {
            let h = try sourceMemoryHandle()
            var revision: UInt64 = 0
            try IntegrationBridge.check(
                ambient_plugin_v2_begin_removal(h, p.id, p.revision, &revision))
            return revision

        }
    }
    func finishPluginRemoval(_ id: UInt64, revision: UInt64) async throws {
        return try await withObservationMutation {
            let h = try sourceMemoryHandle()
            try IntegrationBridge.check(ambient_plugin_v2_finish_removal(h, id, revision))

        }
    }
}

/// Bounded, no-follow reads of one plugin folder: the manifest, the MCP server list and up to 8
/// skills. Everything else in the folder is never opened.
enum PluginPackageReader {
    static let maxFile = 262_144
    static let maxTotal = 1_048_576
    enum Failure: Error, Equatable { case missing, unreadable, tooLarge }
    static func read(folder: URL) throws -> [PluginPackageFile] {
        let root = open(folder.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard root >= 0 else { throw Failure.unreadable }
        defer { close(root) }
        var files: [PluginPackageFile] = []
        var total = 0
        func add(_ kind: PluginPackageFile.Kind, _ path: String) throws -> Bool {
            guard let bytes = try readFile(root, path) else { return false }
            total += bytes.count
            guard total <= maxTotal else { throw Failure.tooLarge }
            files.append(PluginPackageFile(kind: kind, path: path, bytes: bytes))
            return true
        }
        if try !add(.manifest, "plugin.json") {
            guard try add(.manifest, ".codex-plugin/plugin.json") else { throw Failure.missing }
        }
        if try !add(.mcp, "mcp.json") { _ = try add(.mcp, ".mcp.json") }
        // Skill directories are listed by name only; Core rejects more than 8 and odd names.
        for name in try entries(root, "skills").sorted().prefix(64) {
            _ = try add(.skill, "skills/\(name)/SKILL.md")
        }
        return files
    }
    /// Opens each component relative to the folder without following links; a missing file is
    /// nil, anything that is not a regular file within the limit fails.
    private static func readFile(_ root: Int32, _ path: String) throws -> [UInt8]? {
        let parts = path.split(separator: "/").map(String.init)
        var dir = root
        var opened: [Int32] = []
        defer { for fd in opened { close(fd) } }
        for part in parts.dropLast() {
            let fd = openat(dir, part, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            if fd < 0 {
                if errno == ENOENT { return nil }
                throw Failure.unreadable
            }
            opened.append(fd)
            dir = fd
        }
        guard let last = parts.last else { throw Failure.unreadable }
        let fd = openat(dir, last, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        if fd < 0 {
            if errno == ENOENT { return nil }
            throw Failure.unreadable
        }
        defer { close(fd) }
        var st = stat()
        guard fstat(fd, &st) == 0, st.st_mode & S_IFMT == S_IFREG else { throw Failure.unreadable }
        guard st.st_size >= 0, Int(st.st_size) <= maxFile else { throw Failure.tooLarge }
        var bytes = [UInt8](repeating: 0, count: maxFile + 1)
        var used = 0
        // One byte past the limit detects a file that grew after fstat.
        while used < bytes.count {
            let n = bytes.withUnsafeMutableBytes { b -> Int in
                guard let base = b.baseAddress else { return -1 }
                return pread(fd, base + used, b.count - used, off_t(used))
            }
            if n < 0 { throw Failure.unreadable }
            if n == 0 { break }
            used += n
        }
        guard used <= maxFile else { throw Failure.tooLarge }
        return Array(bytes.prefix(used))
    }
    private static func entries(_ root: Int32, _ name: String) throws -> [String] {
        let fd = openat(root, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        if fd < 0 {
            if errno == ENOENT { return [] }
            throw Failure.unreadable
        }
        guard let dir = fdopendir(fd) else {
            close(fd)
            throw Failure.unreadable
        }
        defer { closedir(dir) }
        var names: [String] = []
        while let e = readdir(dir) {
            let name = withUnsafeBytes(of: e.pointee.d_name) {
                String(decoding: $0.prefix(Int(e.pointee.d_namlen)), as: UTF8.self)
            }
            if name != "." && name != ".." && e.pointee.d_type == DT_DIR { names.append(name) }
            if names.count > 64 { break }
        }
        return names
    }
    /// Installed Codex plugins: `~/.codex/plugins/cache/<marketplace>/<plugin>/<version>/`.
    static func codexPlugins(home: URL = FileManager.default.homeDirectoryForCurrentUser) -> [URL] {
        let cache = home.appending(path: ".codex/plugins/cache", directoryHint: .isDirectory)
        let fm = FileManager.default
        func dirs(_ u: URL) -> [URL] {
            ((try? fm.contentsOfDirectory(at: u, includingPropertiesForKeys: [.isSymbolicLinkKey]))
                ?? [])
                .filter {
                    (try? $0.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey]))
                        .map { $0.isDirectory == true && $0.isSymbolicLink != true } ?? false
                }
                .sorted { $0.path < $1.path }
        }
        return dirs(cache).flatMap(dirs).flatMap(dirs).prefix(64).map { $0 }
    }
}
