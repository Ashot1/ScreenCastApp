import Foundation
import Testing

@testable import AmbientMacOS

/// A review-only package: its declared program is never started or installed by this test.
@Test func settingsPluginFixtureCanBeReviewedWithoutInstallingOrCallingTools() async throws {
    var project = URL(filePath: #filePath)
    for _ in 0..<6 { project.deleteLastPathComponent() }
    let verification = project.appending(
        path: "target/verification/26f", directoryHint: .isDirectory)
    let root = verification.appending(path: "fixture-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let folder = try writeSettingsReviewFixture(in: root)
    let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
    let (files, review) = try await memory.reviewPluginFolder(folder)
    #expect(files.map(\.path) == ["plugin.json", "mcp.json", "skills/compare/SKILL.md"])
    #expect(review.name == "Visual review fixture")
    #expect(review.skills.map(\.name) == ["compare-saved-names"])
    let server = try #require(review.servers.first)
    #expect(server.key == "local")
    #expect(
        server.kind == .local(command: "/usr/bin/false", args: [], env: ["FIXTURE_VALUE"]))
    #expect(server.needsKey && !server.downloads)
    #expect(try await memory.plugins().plugins.isEmpty)
    #expect(try await memory.pluginCalls().isEmpty)
    try await memory.close()

    if let path = ProcessInfo.processInfo.environment["AMBIENT_SETTINGS_FIXTURE_ROOT"] {
        let destination = URL(filePath: path).standardizedFileURL.resolvingSymlinksInPath()
        try #require(destination.path.hasPrefix(verification.path + "/"))
        _ = try writeSettingsReviewFixture(in: destination)
    }
}

private func writeSettingsReviewFixture(in root: URL) throws -> URL {
    let folder = root.appending(path: "plugin-review", directoryHint: .isDirectory)
    let skill = folder.appending(path: "skills/compare", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: skill, withIntermediateDirectories: true)
    try Data(#"{"name":"Visual review fixture","version":"1.0.0"}"#.utf8)
        .write(to: folder.appending(path: "plugin.json"))
    try Data(
        #"{"mcpServers":{"local":{"command":"/usr/bin/false","env":{"FIXTURE_VALUE":""}}}}"#.utf8
    ).write(to: folder.appending(path: "mcp.json"))
    try Data(
        "---\nname: compare-saved-names\ndescription: Compare saved task names\n---\nCompare only the names provided in saved task context.\n"
            .utf8
    ).write(to: skill.appending(path: "SKILL.md"))
    return folder
}
