import CAmbient
import Foundation
import Testing

@testable import AmbientMacOS

@Test func modelNativeABIRejectsPauseAndCancelledAdmissionWithoutNetwork() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    #expect(MemoryLayout<AmbientModelAttempt>.size == 80)
    #expect(MemoryLayout<AmbientModelSummary>.size == 128)
    try await memory.setManualPause(true)
    await #expect(throws: ModelCheckFailure.paused) {
        _ = try await memory.beginCheck(userReported: true, at: 1)
    }
    try await memory.setManualPause(false)
    let handle = try await memory.beginCheck(userReported: true, at: 2)
    let bridge = ModelCheckBridge()
    bridge.cancel(handle)
    await #expect(throws: ModelCheckFailure.cancelled) {
        _ = try await bridge.dispatch(
            handle, access: "test-only-access", keys: [],
            root: FileManager.default.temporaryDirectory)
    }
    try await bridge.close(handle)
    #expect(try await memory.checkSummary().attempts == 0)
    try await memory.close()
}
private final class CheckTestStore: ChatGPTCredentialStore, @unchecked Sendable {
    // Test-only synchronous credential storage, serialized across the coordinator's executor.
    private let lock = NSLock()
    private var value: ChatGPTEnvelope
    init() {
        value = ChatGPTEnvelope(
            host: "urn:uuid:12345678-1234-4234-9234-123456789abc", registration: 3,
            clientID: "test-client", issuer: "https://auth.openai.com", subject: "test-subject",
            hint: "test-hint", nonce: "test-nonce",
            credentials: ChatGPTCredentials(
                access: "test-access", refresh: "test-refresh",
                scopes: ["openid", "resource.invoke", "chatgpt.tokens.use.direct"],
                expiresAt: Int64(Date().timeIntervalSince1970) + 3600))
    }
    func load() -> ChatGPTEnvelope? { lock.withLock { value } }
    func save(_ envelope: ChatGPTEnvelope) { lock.withLock { value = envelope } }
}
private actor HeldCheckBridge: ModelCheckProtocol {
    private let outcome: ModelCheckOutcome
    init(outcome: ModelCheckOutcome = .confirmed) { self.outcome = outcome }
    private var started = false
    private var startWaiter: CheckedContinuation<Void, Never>?
    private var releaseWaiter: CheckedContinuation<Void, Never>?
    func waitForDispatch() async {
        if !started { await withCheckedContinuation { startWaiter = $0 } }
    }
    func release() {
        releaseWaiter?.resume()
        releaseWaiter = nil
    }
    func dispatch(_ handle: UInt64, access: String, keys: [PluginDispatchKey], root: URL)
        async throws -> ModelOperationResult
    {
        started = true
        startWaiter?.resume()
        startWaiter = nil
        await withCheckedContinuation { releaseWaiter = $0 }
        return ModelOperationResult(
            attempt: ModelCheckAttempt(
                id: 1, atUTC: 1, stage: 2, outcome: outcome, model: .expected, effort: .low,
                tier: .priority,
                usage: ModelCheckUsage(input: 8, output: 3, total: 11)), publishedTaskID: nil,
            workspaceRevision: nil)
    }
    nonisolated func cancel(_ handle: UInt64) { _ = ambient_model_v6_cancel(handle) }
    func close(_ handle: UInt64) throws {
        try ModelCheckBridge.check(ambient_model_v6_close(handle))
    }
}
@Test func creditReportBelongsToOneCoordinatorAndSignOutFencesLateSuccessUntilCleanup() async throws
{
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    let store = CheckTestStore()
    let bridge = HeldCheckBridge()
    let coordinator = ChatGPTCoordinator(store: store, memory: memory, modelBridge: bridge)
    await #expect(throws: ModelCheckFailure.report) { _ = try await coordinator.runCheck() }
    try await coordinator.setCreditReport(true)
    #expect(await coordinator.creditReport())
    let operation = Task { try await coordinator.runCheck() }
    await bridge.waitForDispatch()
    await coordinator.cancel()
    #expect(try await coordinator.load() == .cancelling)
    await #expect(throws: ChatGPTFailure.busy) { try await coordinator.run(refresh: false) }
    try await coordinator.signOut()
    #expect(await coordinator.creditReport() == false)
    let shutdown = Task { try await coordinator.shutdown() }
    await bridge.release()
    await #expect(throws: ChatGPTFailure.cancelled) { _ = try await operation.value }
    try await shutdown.value
    let fresh = ChatGPTCoordinator(store: store, memory: memory)
    #expect(await fresh.creditReport() == false)
    try await memory.close()
}

@Test @MainActor func successfulJournalRereadClearsOnlyTheReadFailure() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    try Data("invalid temporary database".utf8).write(to: url)
    let memory = MemorySession(databaseURL: url)
    let coordinator = ChatGPTCoordinator(store: CheckTestStore(), memory: memory)
    let model = ChatGPTModel(coordinator: coordinator)
    await model.reloadChecks()
    #expect(model.checkSummary == nil)
    try FileManager.default.removeItem(at: url)
    await model.reloadChecks()
    #expect(model.checkSummary?.attempts == 0)
    #expect(model.checkFailure == nil)
    await model.checkConnection()
    #expect(model.checkFailure == .report)
    await model.reloadChecks()
    #expect(model.checkFailure == .report)
    try await memory.close()
}

@Test func modelTimeoutSurvivesActualCleanupBeyondSixtySeconds() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    let bridge = HeldCheckBridge(outcome: .timeout)
    let coordinator = ChatGPTCoordinator(
        store: CheckTestStore(), memory: memory, modelBridge: bridge)
    try await coordinator.setCreditReport(true)
    let operation = Task { try await coordinator.runCheck() }
    await bridge.waitForDispatch()
    // Real cleanup can outlive the transport deadline, e.g. a system DNS worker.
    try await Task.sleep(for: .seconds(61))
    await bridge.release()
    #expect(try await operation.value.outcome == .timeout)
    try await coordinator.shutdown()
    try await memory.close()
}
