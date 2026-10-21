import CAmbient
import Foundation
import Testing

@testable import AmbientMacOS

@Test func builtInIntegrationKindsUseTheFixedABIValues() {
    #expect(BuiltInIntegration.allCases.map(\.abi) == [1, 2, 3, 4, 5])
    #expect(BuiltInIntegration.todoist.titleKey == "integrations.todoist")
}

@Test @MainActor func integrationSwitchesStartEnabledPersistAndCloseReading() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-switches-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }
    let url = directory.appendingPathComponent("memory.sqlite3")
    let memory = MemorySession(databaseURL: url)
    let model = AssistantModel(session: memory)
    #expect(!model.integrations.isEnabled(.jira))
    await model.integrations.load()
    #expect(BuiltInIntegration.allCases.allSatisfy { model.integrations.isEnabled($0) })
    let p = try await memory.saveJiraProfile(id: 0, expected: 0, name: "Work", cloudID: "Site-ID")
    await model.integrations.set(.jira, enabled: false)
    await model.integrations.set(.localProjects, enabled: false)
    #expect(!model.integrations.isEnabled(.jira) && model.integrations.failureKey == nil)
    #expect(model.integrations.isEnabled(.github) && model.integrations.isEnabled(.codex))
    // The same value is accepted; settings of a disabled integration still change.
    await model.integrations.set(.jira, enabled: false)
    #expect(model.integrations.failureKey == nil)
    let renamed = try await memory.saveJiraProfile(
        id: p.id, expected: p.settingsRevision, name: "Renamed", cloudID: "Site-ID")
    #expect(renamed.name == "Renamed")
    try await memory.close()
    let reopened = MemorySession(databaseURL: url)
    #expect(try await reopened.integrationEnabled(.jira) == false)
    #expect(try await reopened.integrationEnabled(.localProjects) == false)
    #expect(try await reopened.integrationEnabled(.codex) == true)
    try await reopened.setIntegrationEnabled(.jira, enabled: true)
    #expect(try await reopened.integrationEnabled(.jira) == true)
    try await reopened.close()
}
