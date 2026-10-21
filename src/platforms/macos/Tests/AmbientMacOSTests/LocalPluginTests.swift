import Darwin
import Foundation
import Testing

@testable import AmbientMacOS

/// The test-only stdio MCP server shared with the Rust local-host tests.
private let stdioServer = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "providers/tests/fixtures/stdio_server.py").path

private func localFolder(mode: String, env: [String]) throws -> URL {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-local-plugin-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    try Data(#"{"name":"Local tools"}"#.utf8).write(to: root.appending(path: "plugin.json"))
    let values = Dictionary(uniqueKeysWithValues: env.map { ($0, "package value is ignored") })
    let server: [String: Any] = [
        "command": "/usr/bin/python3", "args": [stdioServer, mode], "env": values,
    ]
    try JSONSerialization.data(withJSONObject: ["mcpServers": ["local": server]]).write(
        to: root.appending(path: "mcp.json"))
    return root
}
private func starts(_ root: URL, _ plugin: UInt64) -> [Int32] {
    let file = root.appending(path: "\(plugin)/starts")
    return ((try? String(contentsOf: file, encoding: .utf8)) ?? "").split(separator: "\n")
        .compactMap { Int32($0) }
}
/// `kill(-group, 0)` fails with ESRCH once no process of the group is left.
private func groupGone(_ pid: Int32) -> Bool { kill(-pid, 0) != 0 && errno == ESRCH }

@Test @MainActor func launchIgnoresSigpipeSoAWriteToAnExitedLocalServerIsUnavailable()
    async throws
{
    // The real launch function of the app, not a signal call in the test.
    AmbientProcess.launch()
    var current = sigaction()
    #expect(sigaction(SIGPIPE, nil, &current) == 0)
    let handler = unsafeBitCast(current.__sigaction_u.__sa_handler, to: Int.self)
    #expect(handler == unsafeBitCast(SIG_IGN, to: Int.self))
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-sigpipe-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let memory = MemorySession(databaseURL: directory.appending(path: "memory.sqlite3"))
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    // The server closes its stdin after initialize, so the next write meets a closed pipe.
    let folder = try localFolder(mode: "close-stdin", env: [])
    defer { try? FileManager.default.removeItem(at: folder) }
    let (files, review) = try await memory.reviewPluginFolder(folder)
    let id = try await memory.installPlugin(files, hash: review.hash, at: 1)
    let p = try #require(try await memory.plugins().plugins.first { $0.id == id })
    let s = try #require(p.servers.first)
    let operation = try PluginOperation.begin()
    let found = try await PluginDiscovery().discover(
        memory: try await memory.sourceMemoryHandle(), operation: operation,
        root: try await memory.pluginServersRoot(), plugin: p.id, server: s.key,
        generation: s.generation, key: nil)
    try operation.close()
    // Still running: EPIPE became «unavailable» instead of SIGPIPE ending this process.
    #expect(found.status == .unavailable)
    #expect(groupGone(try #require(starts(try await memory.pluginServersRoot(), p.id).first)))
    try await memory.close()
}

@Test func localValuesAreAnEnvSecretWithCompactSortedTextAndStrictNames() throws {
    let secret = PluginSecret.env(values: ["ZED": "z/1", "API_TOKEN": "a\"b"])
    try secret.validate()
    #expect(try secret.dispatchText() == #"{"API_TOKEN":"a\"b","ZED":"z/1"}"#)
    let e = PluginEnvelope(
        version: 1, instance: String(repeating: "a", count: 32), pluginID: 7, server: "local",
        generation: 9, secret: secret)
    let decoded = try JSONDecoder().decode(PluginEnvelope.self, from: JSONEncoder().encode(e))
    #expect(decoded == e)
    let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(e)) as? [String: Any]
    let env = (json?["secret"] as? [String: Any])?["env"] as? [String: Any]
    #expect(env?.keys.map { $0 } == ["values"])
    for bad: [String: String] in [
        [:], ["lower": "x"], ["A": ""], ["A": "with space"], ["A": "é"],
        Dictionary(uniqueKeysWithValues: (0..<17).map { ("V\($0)", "x") }),
    ] {
        #expect(throws: IntegrationFailure.invalid) { try PluginSecret.env(values: bad).validate() }
    }
}

@Test @MainActor func onlyRunAndShowToolsStartsALocalProgramAndRemovalDeletesItsFolder()
    async throws
{
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let folder = try localFolder(mode: "ok", env: ["API_TOKEN"])
    defer { try? FileManager.default.removeItem(at: folder) }
    let memory = MemorySession(databaseURL: fixture.databaseURL())
    let secrets = PluginSecretOwner(
        memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain))
    let plugins = PluginsModel(memory: memory, secrets: secrets)
    let root = try await memory.pluginServersRoot()
    await plugins.open(folder)
    let spec = try #require(plugins.draft?.review.servers.first)
    #expect(
        spec.kind
            == .local(command: "/usr/bin/python3", args: [stdioServer, "ok"], env: ["API_TOKEN"]))
    #expect(spec.needsKey && !spec.downloads)
    plugins.draft?.values["local"] = ["API_TOKEN": "local-secret-2"]
    // Installing reviews nothing by running: the program starts only on the user's request.
    await plugins.installDraft()
    #expect(plugins.failureKey == nil)
    let p = try #require(plugins.draft?.plugin)
    #expect(starts(root, p.id).isEmpty)
    #expect(plugins.draft?.discoveries.isEmpty == true)
    await plugins.runDraft(server: "local")
    #expect(plugins.draft?.discoveries["local"]?.status == .ok)
    #expect(plugins.draft?.discoveries["local"]?.tools?.map(\.name) == ["echo", "env", "pid"])
    #expect(starts(root, p.id).count == 1)
    #expect(groupGone(try #require(starts(root, p.id).first)), "discovery stopped it")
    plugins.draft?.selected = ["local/echo"]
    await plugins.enableDraft()
    let installed = try #require(plugins.plugins.first)
    #expect(installed.tools.map(\.name) == ["echo"])
    // Opening the settings repeats remote discovery only.
    await plugins.rediscover(installed)
    #expect(starts(root, p.id).count == 1)
    await plugins.runInstalled(installed, server: "local")
    #expect(plugins.live[p.id]?["local"]?.status == .ok)
    #expect(starts(root, p.id).count == 2)
    #expect(
        try await secrets.dispatchKeys().map(\.key) == [#"{"API_TOKEN":"local-secret-2"}"#])
    // Values never reach SQLite.
    let bytes = try Data(contentsOf: fixture.databaseURL())
    #expect(bytes.range(of: Data("local-secret-2".utf8)) == nil)
    let folderOfPlugin = root.appending(path: String(p.id), directoryHint: .isDirectory)
    #expect(FileManager.default.fileExists(atPath: folderOfPlugin.path))
    #expect(try await secrets.delete(installed))
    #expect(!FileManager.default.fileExists(atPath: folderOfPlugin.path))
    try await memory.close()
}
