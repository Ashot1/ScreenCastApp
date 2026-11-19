import Foundation
import Testing

@testable import AmbientMacOS

private actor ObservationUISource: FrameSource {
    var permission = false
    var requests = 0
    var permissionChecks = 0
    var warmUps = 0
    var captures = 0
    private var checkHeld = false
    private var checkEntered = false
    private var checkArrival: CheckedContinuation<Void, Never>?
    private var checkRelease: CheckedContinuation<Void, Never>?
    func holdPermissionCheck() { checkHeld = true }
    func waitForPermissionCheck() async {
        if !checkEntered { await withCheckedContinuation { checkArrival = $0 } }
    }
    func releasePermissionCheck() {
        checkRelease?.resume()
        checkRelease = nil
    }
    private var permissionHeld = false
    private var requestEntered = false
    private var requestArrival: CheckedContinuation<Void, Never>?
    private var requestRelease: CheckedContinuation<Void, Never>?
    func holdPermissionRequest() { permissionHeld = true }
    func waitForPermissionRequest() async {
        if !requestEntered { await withCheckedContinuation { requestArrival = $0 } }
    }
    func releasePermissionRequest() {
        requestRelease?.resume()
        requestRelease = nil
    }
    private var captureHeld = false
    private var captureEntered = false
    private var captureArrival: CheckedContinuation<Void, Never>?
    private var captureRelease: CheckedContinuation<Void, Never>?
    func holdCapture() { captureHeld = true }
    func waitForCapture() async {
        if !captureEntered { await withCheckedContinuation { captureArrival = $0 } }
    }
    func releaseCapture() {
        captureRelease?.resume()
        captureRelease = nil
    }
    func requestPermission() async -> Bool {
        requests += 1
        if permissionHeld {
            requestEntered = true
            requestArrival?.resume()
            requestArrival = nil
            await withCheckedContinuation { requestRelease = $0 }
        }
        return permission
    }
    func hasPermission() async -> Bool {
        permissionChecks += 1
        if checkHeld {
            checkEntered = true
            checkArrival?.resume()
            checkArrival = nil
            await withCheckedContinuation { checkRelease = $0 }
        }
        return permission
    }
    func setPermission(_ value: Bool) { permission = value }
    func idleSeconds() -> UInt32 { 0 }
    func warmUp() { warmUps += 1 }
    func capture() async -> ObservationFrame? {
        captures += 1
        if captureHeld {
            captureHeld = false
            captureEntered = true
            captureArrival?.resume()
            captureArrival = nil
            await withCheckedContinuation { captureRelease = $0 }
        }
        return nil
    }
}
private actor ObservationUIDispatcher: ObservationDispatching {
    var available = false
    var cancellations = 0
    func setAvailable(_ value: Bool) { available = value }
    func prepareObservation(at: ObservationTime) -> Bool { available }
    func runObservation(at: ObservationTime) -> ModelOperationResult? { nil }
    func cancelObservation() { cancellations += 1 }
}
@MainActor private func observationUIEventually(
    timeout: Duration = .seconds(2), _ condition: () async throws -> Bool
) async throws {
    let deadline = ContinuousClock.now + timeout
    while try await !condition(), ContinuousClock.now < deadline {
        try await Task.sleep(for: .milliseconds(5))
    }
    #expect(try await condition())
}
private func observationUISession(
    _ memory: MemorySession, _ source: ObservationUISource,
    dispatcher: any ObservationDispatching = ObservationUIDispatcher()
) -> ObservationSession {
    ObservationSession(
        memory: memory, dispatcher: dispatcher, source: source,
        draw: { _ in }, changed: {},
        clock: ObservationClock(now: { .init(utc: 100, monotonicMS: 0) }))
}

@Test @MainActor func observationUICreditPermissionDenialAndExplicitRetry() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let source = ObservationUISource()
    let session = observationUISession(memory, source)
    await #expect(throws: ModelCheckFailure.report) {
        try await session.start(creditAcknowledged: false, perHourLimit: 7)
    }
    #expect(await source.requests == 0)
    #expect(try await !memory.observationJournal(at: 100).policy.started)
    try await session.start(creditAcknowledged: true, perHourLimit: 7)
    try await observationUIEventually { session.display.state == .needsPermission }
    #expect(await source.requests == 1)
    #expect(await source.warmUps == 0)
    #expect(await source.captures == 0)
    #expect(
        session.display.journal?.policy
            == .init(started: true, perHourLimit: 7, creditAcknowledged: true))
    await source.setPermission(true)
    try await session.start(creditAcknowledged: true, perHourLimit: 7)
    // Native activity setup shares MainActor with host tests; wait for actual source progress first.
    try await observationUIEventually(timeout: .seconds(30)) { await source.warmUps == 1 }
    try await observationUIEventually { session.display.state == .waiting }
    #expect(await source.requests == 2)
    #expect(session.display.permission == true)
    #expect(session.display.journal?.policy.perHourLimit == 7)
    await session.shutdown()
    try await memory.close()
}

@Test @MainActor func observationUIRestoreKeepsCustomPolicyWithoutPrompt() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let saved = ObservationPolicy(started: true, perHourLimit: 9, creditAcknowledged: true)
    try await memory.setObservationPolicy(saved)
    let source = ObservationUISource()
    let session = observationUISession(memory, source)
    await session.restore()
    try await observationUIEventually { session.display.state == .needsPermission }
    #expect(await source.requests == 0)
    #expect(session.display.journal?.policy == saved)
    #expect(try await memory.observationJournal(at: 100).policy == saved)
    await session.shutdown()
    try await memory.close()
}

@Test @MainActor func observationUIOrdinaryVisualCleanupCannotRestoreCapture() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 11, creditAcknowledged: true))
    let source = ObservationUISource()
    let session = observationUISession(memory, source)
    await session.cleanup()
    try await observationUIEventually { session.display.journal != nil }
    #expect(session.display.state == .stopped)
    #expect(session.display.permission == nil)
    #expect(session.display.journal?.policy.perHourLimit == 11)
    #expect(await source.requests == 0)
    #expect(await source.permissionChecks == 0)
    #expect(await source.warmUps == 0)
    #expect(await source.captures == 0)
    await session.shutdown()
    try await memory.close()
}

private func observationUIJournal(outcome: UInt32, tier: UInt32) throws -> ObservationJournal {
    let text = """
        {"policy":{"started":false,"per_hour_limit":20,"credit_acknowledged":false},
        "log_frames":0,"unsent_frames":0,"hour_attempts":1,
        "sends":[{"attempt":1,"at_utc":100,"first_frame":1,"last_frame":1,"apps":["Fixture"],
        "images":1,"dropped_frames":0,"dropped_bytes":0,"dropped_images":0,
        "outcome":\(outcome),"tier":\(tier),"added_cards":0,"rejected_items":0}]}
        """
    let bytes = Array(text.utf8)
    return try ObservationBridge.json(ObservationJournal.self, limit: 16_777_216) {
        output, capacity, length in
        length.pointee = UInt32(bytes.count)
        guard let output, capacity >= bytes.count else { return 6 }
        bytes.withUnsafeBufferPointer { input in
            if let address = input.baseAddress { output.update(from: address, count: bytes.count) }
        }
        return 0
    }
}
@Test func observationUIJournalRejectsInvalidFFIOutcomeAndTierButKeepsOther() throws {
    #expect(try observationUIJournal(outcome: 0, tier: 4).sends.first?.tier == .other)
    #expect(try observationUIJournal(outcome: 9, tier: 0).sends.first?.outcome == .quota)
    #expect(throws: ModelCheckFailure.native) { try observationUIJournal(outcome: 10, tier: 1) }
    #expect(throws: ModelCheckFailure.native) { try observationUIJournal(outcome: 1, tier: 5) }
}
@Test @MainActor func observationUIStopPublishesActualStoppedState() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let source = ObservationUISource()
    let session = observationUISession(memory, source)
    try await session.start(creditAcknowledged: true, perHourLimit: 8)
    try await observationUIEventually { session.display.state == .needsPermission }
    try await session.stop()
    #expect(session.display.state == .stopped)
    #expect(session.display.working == false)
    #expect(
        session.display.journal?.policy
            == .init(started: false, perHourLimit: 8, creditAcknowledged: false))
    await session.shutdown()
    try await memory.close()
}

@Test @MainActor func observationUIHourlyLimitPreservesIntentAndDoesNotRestart() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let source = ObservationUISource()
    let dispatcher = ObservationUIDispatcher()
    await source.setPermission(true)
    await source.holdCapture()
    await dispatcher.setAvailable(true)
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    let session = observationUISession(memory, source, dispatcher: dispatcher)
    try await session.start(creditAcknowledged: true, perHourLimit: 8)
    await source.waitForCapture()
    // A limit update must finish while the same capture is still running and ignores cancellation.
    try await session.setHourlyLimit(1)
    #expect(
        session.display.journal?.policy
            == .init(started: true, perHourLimit: 1, creditAcknowledged: true))
    try await session.setHourlyLimit(60)
    #expect(session.display.journal?.policy.perHourLimit == 60)
    #expect(await source.requests == 1)
    #expect(await source.captures == 1)
    #expect(await source.warmUps == 1)
    await #expect(throws: ModelCheckFailure.invalid) { try await session.setHourlyLimit(0) }
    await #expect(throws: ModelCheckFailure.invalid) { try await session.setHourlyLimit(61) }
    #expect(try await memory.observationJournal(at: 100).policy.perHourLimit == 60)
    await source.releaseCapture()
    try await session.stop()
    try await session.setHourlyLimit(9)
    #expect(
        session.display.journal?.policy
            == .init(started: false, perHourLimit: 9, creditAcknowledged: false))
    await session.shutdown()
    try await memory.close()
}
@Test @MainActor func observationUIDeleteWaitsForActualCaptureWithoutStoppingIntent() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let source = ObservationUISource()
    let dispatcher = ObservationUIDispatcher()
    await dispatcher.setAvailable(true)
    await source.setPermission(true)
    await source.holdCapture()
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    let policy = ObservationPolicy(started: true, perHourLimit: 13, creditAcknowledged: true)
    try await memory.setObservationPolicy(policy)
    _ = try await memory.submitObservationFrame(
        .init(
            displayKey: "fixture", bundleID: "test.editor", appName: "Fixture", title: "Saved",
            lines: [.init(text: "Saved fixture", words: [])], jpeg: Data([1])),
        idleSeconds: 0, at: .init(utc: 100, monotonicMS: 0))
    let session = observationUISession(memory, source, dispatcher: dispatcher)
    await session.restore()
    await source.waitForCapture()
    var finished = false
    let deletion = Task {
        try await session.deleteJournal()
        finished = true
    }
    try await observationUIEventually {
        try await memory.observationJournal(at: 100).logFrames == 0
    }
    #expect(!finished)
    #expect(try await memory.observationJournal(at: 100).policy == policy)
    await #expect(throws: ModelCheckFailure.busy) { try await session.setHourlyLimit(19) }
    await source.releaseCapture()
    try await deletion.value
    #expect(session.display.journal?.policy == policy)
    #expect(session.display.journal?.logFrames == 0)
    #expect(session.display.journal?.sends.isEmpty == true)
    #expect(session.display.working == false)
    await session.shutdown()
    try await memory.close()
}

@Test @MainActor func observationUILimitCannotOverwriteStopWhileCaptureFinishes() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let source = ObservationUISource()
    let dispatcher = ObservationUIDispatcher()
    await source.setPermission(true)
    await source.holdCapture()
    await dispatcher.setAvailable(true)
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    let session = observationUISession(memory, source, dispatcher: dispatcher)
    try await session.start(creditAcknowledged: true, perHourLimit: 12)
    await source.waitForCapture()
    let stop = Task { try await session.stop() }
    try await observationUIEventually {
        try await !memory.observationJournal(at: 100).policy.started
    }
    await #expect(throws: ModelCheckFailure.busy) { try await session.setHourlyLimit(18) }
    await source.releaseCapture()
    try await stop.value
    try await session.setHourlyLimit(18)
    #expect(
        session.display.journal?.policy
            == .init(started: false, perHourLimit: 18, creditAcknowledged: false))
    await session.shutdown()
    try await memory.close()
}
@Test @MainActor func observationUIClosingWaitsForPermissionAndCannotPublishLateStart() async throws
{
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let source = ObservationUISource()
    await source.setPermission(true)
    await source.holdPermissionRequest()
    let dispatcher = ObservationUIDispatcher()
    let session = observationUISession(memory, source, dispatcher: dispatcher)
    let start = Task { try await session.start(creditAcknowledged: true, perHourLimit: 6) }
    await source.waitForPermissionRequest()
    let previousCancellations = await dispatcher.cancellations
    var closed = false
    let shutdown = Task {
        await session.shutdown()
        closed = true
    }
    try await observationUIEventually { await dispatcher.cancellations > previousCancellations }
    await #expect(throws: ModelCheckFailure.closed) { try await session.setHourlyLimit(3) }
    #expect(!closed)
    await source.releasePermissionRequest()
    await #expect(throws: ModelCheckFailure.closed) { try await start.value }
    await shutdown.value
    #expect(closed)
    #expect(session.display.state == .stopped)
    #expect(!session.display.working)
    #expect(session.display.permission == nil)
    await session.restore()
    await session.refreshJournal()
    #expect(session.display.journal == nil)
    #expect(await source.captures == 0)
    try await memory.close()
}
@Test @MainActor func observationUIFailedLoadIsNotEmptyOrStillLoading() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    try await memory.close()
    let source = ObservationUISource()
    let session = observationUISession(memory, source)
    await #expect(throws: MemoryFailure.closed) {
        try await session.start(creditAcknowledged: true)
    }
    #expect(session.display.state == .failed)
    #expect(session.display.failure == .native)
    #expect(!session.display.loading)
    #expect(!session.display.working)
    #expect(session.display.journal == nil)
    await session.shutdown()
}
@Test(arguments: [false, true]) @MainActor
func observationUIRestoreHonorsManualPauseAndInvalidSession(paused: Bool) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 14, creditAcknowledged: true))
    try await memory.setManualPause(paused)
    let source = ObservationUISource()
    await source.setPermission(true)
    let session = observationUISession(memory, source)
    await session.restore()
    try await observationUIEventually { session.display.state == (paused ? .paused : .waiting) }
    #expect(session.display.journal?.policy.perHourLimit == 14)
    #expect(await source.requests == 0)
    #expect(await source.captures == 0)
    await session.shutdown()
    try await memory.close()
}

@Test @MainActor func observationUIMenuUsesActualStateAndPreservesPauseClosingPrecedence()
    async throws
{
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    let model = AssistantModel(session: memory)
    let display = ObservationDisplay()
    display.update(state: .observing, failure: nil, working: false)
    #expect(model.menuStatusKey(display) == "status.pauseUnknown")
    await model.loadManualPause()
    for state: ObservationState in [
        .stopped, .preparing, .needsPermission, .observing, .paused, .waiting, .failed,
    ] {
        display.update(state: state, failure: nil, working: false)
        #expect(model.menuStatusKey(display) == "observation.state.\(state)")
    }
    try await memory.setManualPause(true)
    await model.refreshManualPause()
    display.update(state: .observing, failure: nil, working: false)
    #expect(model.menuStatusKey(display) == "status.paused")
    #expect(model.menuStatusSymbol(display) == "pause.circle")
    model.beginClosing()
    #expect(model.menuStatusKey(display) == "status.closing")
    #expect(model.menuStatusSymbol(display) == "hourglass")
    try await memory.close()
}

// Test-only storage hold reuses the real actor/FFI path; no production load hook is added.
private struct ObservationUILoadGate: Sendable {
    let entered: AsyncStream<Void>
    let arrival: AsyncStream<Void>.Continuation
    let release = DispatchSemaphore(value: 0)
    init() { (entered, arrival) = AsyncStream.makeStream() }
}
extension MemorySession {
    fileprivate func observationUIHold(_ gate: ObservationUILoadGate) {
        gate.arrival.yield(())
        _ = gate.release.wait(timeout: .now() + 10)
    }
}
@Test @MainActor func observationUIClosingDuringLoadWaitsAndRejectsLateSnapshot() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 17, creditAcknowledged: true))
    let source = ObservationUISource()
    let dispatcher = ObservationUIDispatcher()
    let session = observationUISession(memory, source, dispatcher: dispatcher)
    let gate = ObservationUILoadGate()
    let held = Task { await memory.observationUIHold(gate) }
    defer { gate.release.signal() }
    var arrival = gate.entered.makeAsyncIterator()
    _ = await arrival.next()
    let restoration = Task { await session.restore() }
    try await observationUIEventually { session.display.working }
    var finished = false
    let shutdown = Task {
        await session.shutdown()
        finished = true
    }
    try await observationUIEventually { await dispatcher.cancellations > 0 }
    #expect(!finished)
    gate.release.signal()
    await held.value
    await restoration.value
    await shutdown.value
    #expect(session.display.journal == nil)
    #expect(session.display.state == .stopped)
    #expect(!session.display.working)
    #expect(await source.permissionChecks == 0)
    try await memory.close()
}
@Test @MainActor func observationUIClosingDuringStatusReadCannotPublishPermissionAfterHalt()
    async throws
{
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 6, creditAcknowledged: true))
    let source = ObservationUISource()
    await source.holdPermissionCheck()
    let dispatcher = ObservationUIDispatcher()
    let session = observationUISession(memory, source, dispatcher: dispatcher)
    await session.restore()
    await source.waitForPermissionCheck()
    var finished = false
    let shutdown = Task {
        await session.shutdown()
        finished = true
    }
    try await observationUIEventually { await dispatcher.cancellations > 0 }
    #expect(!finished)
    await source.releasePermissionCheck()
    await shutdown.value
    #expect(session.display.state == .stopped)
    #expect(session.display.permission == nil)
    #expect(!session.display.working)
    #expect(await source.captures == 0)
    try await memory.close()
}
