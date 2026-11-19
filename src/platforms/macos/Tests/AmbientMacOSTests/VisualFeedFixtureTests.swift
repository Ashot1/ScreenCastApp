import Foundation
import Testing

@testable import AmbientMacOS

#if AMBIENT_MODEL_TEST_SUPPORT
    /// Test-only HTTP is the sole model path used to seed the real persisted feed. No stub or
    /// credentials are passed to Ambient Visual; it later opens only the resulting memory file.
    @MainActor func seedVisualFeed(_ memory: MemorySession) async throws {
        let server = try AssistanceHTTPServer(mode: "visual-feed")
        defer { server.stop() }
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let tasks = try await memory.tasks().items
        try #require(tasks.count == 6)
        // Suggestions with saved steps require a confirmed plan; the feed fixture uses the real
        // Core gate instead of weakening it for the mockup's closed review card.
        try await memory.setPlanCompleteness(tasks[1], value: .complete)
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        for task in tasks.prefix(3) {
            let preview = try await coordinator.prepareAssistance(
                try await memory.task(task.id), notes: [])
            _ = try await coordinator.runAssistance(preview.requestID)
        }
        let rows = try await coordinator.proposalFeed()
        let closed = try #require(rows.first { $0.task.taskID == tasks[1].id })
        try await coordinator.setProposalVisibility(closed.target, visible: false)
        let changed = try await memory.task(tasks[2].id)
        _ = try await memory.editTask(changed, text: changed.text + " (пределы уточнены)")
        try await coordinator.shutdown()
    }

    /// A visual seed must persist real Core cards, including hidden and invalidated ones, rather
    /// than manufacturing view state that disappears when Ambient Visual opens the database.
    @Test @MainActor func visualFixtureKeepsFeedStatesAfterRestart() async throws {
        let root = FileManager.default.temporaryDirectory.appending(
            path: "ambient-visual-feed-" + UUID().uuidString, directoryHint: .isDirectory)
        defer { try? FileManager.default.removeItem(at: root) }
        try await writeVisualFixtureTemplates(seed: root)
        let memory = MemorySession(databaseURL: root.appending(path: "cases/memory.sqlite3"))
        let rows = try await memory.proposalFeed()
        #expect(rows.count == 3)
        #expect(rows.filter { $0.kind == .clarification && !$0.stale && $0.visible }.count == 1)
        #expect(rows.filter { $0.kind == .suggestion && !$0.stale && !$0.visible }.count == 1)
        #expect(rows.filter { $0.kind == .suggestion && $0.stale && $0.visible }.count == 1)
        #expect(try await memory.checkSummary().attempts == 4)
        try await memory.close()
    }
#endif
