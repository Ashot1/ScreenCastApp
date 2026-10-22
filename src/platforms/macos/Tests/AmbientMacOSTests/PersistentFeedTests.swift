import Foundation
import Testing

@testable import AmbientMacOS

@Test func savedFeedReadsWithoutHighDuringPauseAndDoesNotReserveTransport() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    let task = try await memory.createTask("test-owned intent")
    #expect(try await memory.workspaceRevision(task.id) == 1)
    try await memory.setManualPause(true)
    #expect(try await memory.proposalFeed().isEmpty)
    #expect(try await memory.checkSummary().attempts == 0)
    try await memory.clearDiscussion(task, workspaceRevision: 1)
    #expect(try await memory.workspaceRevision(task.id) == 2)
    try await memory.close()
}

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func persistentFeedUsesOneFullSetAndKeepsClosedIdentityThroughRestart()
        async throws
    {
        let server = try AssistanceHTTPServer(mode: "feed")
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        var memory = MemorySession(databaseURL: url)
        var coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let task = try await memory.createTask("test-owned intent")
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        let p = try await coordinator.prepareAssistance(task, notes: [])
        _ = try await coordinator.runAssistance(p.requestID)
        let first = try await coordinator.proposalFeed()
        #expect(first.count == 2)
        let preview = try await coordinator.prepareContinuation(first[0])
        try await coordinator.setProposalVisibility(first[0].target, visible: false)
        _ = try await coordinator.runAssistance(preview.requestID)
        let rows = try await coordinator.proposalFeed()
        #expect(rows.count == 3)
        #expect(rows[0].id == first[0].id && !rows[0].visible)
        #expect(rows[0].contentRevision == first[0].contentRevision + 1)
        #expect(rows[1].id == first[1].id)
        #expect(try server.requestContexts().count == 3)
        try await coordinator.shutdown()
        try await memory.close()
        memory = MemorySession(databaseURL: url)
        coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        #expect(try await coordinator.proposalFeed() == rows)
        try await coordinator.setCreditReport(true)
        // Copying a kept card needs only a valid session and no pause, not a new check (ADR-025).
        #expect(try await coordinator.copyProposal(rows[0].target) == rows[0].message)
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        #expect(try await coordinator.copyProposal(rows[0].target) == rows[0].message)
        #expect(try await memory.task(task.id).revision == task.revision)
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif
