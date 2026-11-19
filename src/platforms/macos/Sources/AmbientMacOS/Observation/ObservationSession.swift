import AppKit
import Foundation

protocol ObservationDispatching: Sendable {
    func prepareObservation(at: ObservationTime) async throws -> Bool
    func runObservation(at: ObservationTime) async throws -> ModelOperationResult?
    func cancelObservation() async
}
enum ObservationState: Equatable, Sendable {
    case stopped, preparing, needsPermission, observing, paused, waiting, failed
}

/// Measures and draws; cadence, deduplication and admission stay in Core (ADR-025).
public actor ObservationSession {
    nonisolated let display = ObservationDisplay()
    private let memory: MemorySession
    private let dispatcher: any ObservationDispatching
    private let source: any FrameSource
    private let presentation: ObservationPresentation
    private var presentationRevision: UInt64 = 0
    private var memoryRevision: UInt64 = 0
    private var sessionRevision: UInt64 = 0
    private var sessionValid = false
    private var sessionEpoch: UInt64 = 0
    private var highlightReady = false
    private var subscribed = false
    private let changed: @MainActor @Sendable () async -> Void
    private var clock: ObservationClock
    private let journalInterval: Duration
    private enum Mode { case journal, observe }
    private var mode: Mode = .journal
    private var closed = false
    private var warmed = false
    private var highlightVersion: UInt64?
    private var worker: Task<Void, Never>?
    private var sleeper: Task<Void, Never>?
    // Dispatch has its own lifetime so a slow POST never blocks subsequent measurements.
    private var dispatch: Task<Void, Never>?
    private var stopping = false
    // Policy commands span actor suspension points; this gate does not cancel the capture loop.
    private var commandInProgress = false
    private var commandCompletion: CheckedContinuation<Void, Never>?
    private var appSwitched = false
    private(set) var state: ObservationState = .stopped
    private(set) var failure: ModelCheckFailure?

    init(
        memory: MemorySession, dispatcher: any ObservationDispatching, source: any FrameSource,
        draw: @escaping @MainActor @Sendable (ObservationHighlight?) -> Void,
        changed: @escaping @MainActor @Sendable () async -> Void,
        clock: ObservationClock = ObservationClock(), journalInterval: Duration = .seconds(60)
    ) {
        self.memory = memory
        self.dispatcher = dispatcher
        self.source = source
        self.presentation = ObservationPresentation(draw: draw)
        self.changed = changed
        self.clock = clock
        self.journalInterval = journalInterval
    }
    public func restore() async {
        guard !closed, !commandInProgress, worker == nil else { return }
        commandInProgress = true
        defer { finishCommand() }
        stopping = true
        await publish()
        do {
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed else { return }
            stopping = false
            await publish(journal, working: false)
            launch(journal.policy.started ? .observe : .journal)
        } catch {
            guard !closed else { return }
            stopping = false
            await record(error)
            launch(.journal)
        }
    }
    public func cleanup() {
        // Opening the app never arms capture, even with a saved started policy from a flag build.
        guard !closed, !commandInProgress, worker == nil else { return }
        launch(.journal)
    }
    public func start(creditAcknowledged: Bool, perHourLimit: UInt32 = 20) async throws {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !commandInProgress else { throw ModelCheckFailure.busy }
        guard creditAcknowledged else { throw ModelCheckFailure.report }
        commandInProgress = true
        defer { finishCommand() }
        stopping = true
        await publish()
        await halt()
        do {
            guard !closed else { throw ModelCheckFailure.closed }
            try await memory.setObservationPolicy(
                .init(
                    started: true, perHourLimit: perHourLimit,
                    creditAcknowledged: creditAcknowledged))
            guard !closed else { throw ModelCheckFailure.closed }
            let allowed = await source.requestPermission()
            guard !closed else { throw ModelCheckFailure.closed }
            await display.permissionChecked(allowed)
            failure = nil
            state = allowed ? .preparing : .needsPermission
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed else { throw ModelCheckFailure.closed }
            stopping = false
            await publish(journal, working: false)
            launch(.observe)
        } catch {
            if !closed {
                stopping = false
                await record(error, working: false)
                launch(.journal)
            }
            throw error
        }
    }
    public func stop() async throws {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !commandInProgress else { throw ModelCheckFailure.busy }
        commandInProgress = true
        defer { finishCommand() }
        stopping = true
        await publish()
        worker?.cancel()
        sleeper?.cancel()
        dispatch?.cancel()
        do {
            let policy = try await memory.observationJournal(at: clock.sample().utc).policy
            guard !closed else { throw ModelCheckFailure.closed }
            // Revoke Core admission before waiting for an operation that may ignore cancellation.
            try await memory.setObservationPolicy(
                .init(started: false, perHourLimit: policy.perHourLimit, creditAcknowledged: false))
            await halt()
            guard !closed else { throw ModelCheckFailure.closed }
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed else { throw ModelCheckFailure.closed }
            stopping = false
            failure = nil
            await publish(journal, working: false)
            launch(.journal)
        } catch {
            await halt()
            if !closed {
                stopping = false
                await record(error, working: false)
                launch(.journal)
            }
            throw error
        }
    }
    func setHourlyLimit(_ limit: UInt32) async throws {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !commandInProgress else { throw ModelCheckFailure.busy }
        commandInProgress = true
        defer { finishCommand() }
        await publish()
        do {
            let policy = try await memory.observationJournal(at: clock.sample().utc).policy
            guard !closed else { throw ModelCheckFailure.closed }
            try await memory.setObservationPolicy(
                .init(
                    started: policy.started, perHourLimit: limit,
                    creditAcknowledged: policy.creditAcknowledged))
            guard !closed else { throw ModelCheckFailure.closed }
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed else { throw ModelCheckFailure.closed }
            failure = nil
            await publish(journal, working: false)
        } catch {
            await record(error, working: false)
            throw error
        }
    }
    func deleteJournal() async throws {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !commandInProgress else { throw ModelCheckFailure.busy }
        commandInProgress = true
        defer { finishCommand() }
        stopping = true
        let resumeMode = mode
        await publish()
        worker?.cancel()
        sleeper?.cancel()
        dispatch?.cancel()
        do {
            // Core cancels admission and clears the log without changing saved intent or budget.
            try await memory.deleteObservationLog()
            await halt()
            guard !closed else { throw ModelCheckFailure.closed }
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed else { throw ModelCheckFailure.closed }
            stopping = false
            failure = nil
            await publish(journal, working: false)
            launch(resumeMode)
        } catch {
            await halt()
            if !closed {
                stopping = false
                await record(error, working: false)
                launch(resumeMode)
            }
            throw error
        }
    }
    func refreshJournal() async {
        guard !closed, !commandInProgress else { return }
        commandInProgress = true
        defer { finishCommand() }
        await publish()
        do {
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed else { return }
            failure = nil
            await publish(journal, working: false)
        } catch { await record(error, working: false) }
    }
    private func finishCommand() {
        commandInProgress = false
        commandCompletion?.resume()
        commandCompletion = nil
    }
    public func shutdown() async {
        closed = true
        stopping = true
        await halt()
        // UI commands may be awaiting native permission or Core; keep memory alive until they return.
        if commandInProgress { await withCheckedContinuation { commandCompletion = $0 } }
        await display.update(state: .stopped, failure: failure, working: false)
    }
    private func halt() async {
        let runningWorker = worker
        let runningDispatch = dispatch
        runningWorker?.cancel()
        sleeper?.cancel()
        runningDispatch?.cancel()
        highlightReady = false
        await hide()
        await dispatcher.cancelObservation()
        // Cancellation is only a request. Memory stays open until capture and dispatch actually return.
        if let runningWorker { await runningWorker.value }
        if let runningDispatch { await runningDispatch.value }
        worker = nil
        dispatch = nil
        sleeper = nil
        state = .stopped
        highlightVersion = nil
        await hide()
    }
    private func launch(_ mode: Mode) {
        self.mode = mode
        worker = Task { await self.run(mode) }
    }
    private func run(_ mode: Mode) async {
        guard !Task.isCancelled else { return }
        let activity: ObservationActivity?
        if mode == .observe {
            activity = await ObservationActivity()
            if !Task.isCancelled {
                await activity?.start { [weak self] in await self?.activated() }
            }
        } else {
            activity = nil
        }
        while !Task.isCancelled {
            let delay: Duration
            do {
                if mode == .journal {
                    // UTC expiry is Core-owned; this mode never asks for permissions, sources or credentials.
                    let journal = try await memory.observationJournal(at: clock.sample().utc)
                    guard !closed, !Task.isCancelled else { break }
                    await publish(journal)
                    delay = journalInterval
                } else {
                    let switched = appSwitched
                    appSwitched = false
                    switch try await cycle(appSwitched: switched) {
                    case .capture(let seconds): delay = .seconds(seconds)
                    case .hold: delay = .seconds(5)
                    }
                }
            } catch is CancellationError { break } catch {
                await record(error)
                delay = mode == .journal ? journalInterval : .seconds(5)
            }
            if Task.isCancelled { break }
            if mode == .observe, appSwitched { continue }
            sleeper = Task { do { try await Task.sleep(for: delay) } catch {} }
            await sleeper?.value
            sleeper = nil
        }
        await activity?.stop()
    }
    private func activated() {
        guard worker != nil, !stopping else { return }
        appSwitched = true
        sleeper?.cancel()
    }
    func cycle(appSwitched: Bool = false) async throws -> ObservationCadence {
        try Task.checkCancellation()
        guard !closed else { throw ModelCheckFailure.closed }
        guard !stopping else { throw CancellationError() }
        if worker == nil { mode = .observe }  // Test-driven cycles use the same real pipeline.
        if !subscribed {
            subscribed = true
            await memory.observeChanges(self)
        }
        let journal = try await memory.observationJournal(at: clock.sample().utc)
        let policy = journal.policy
        guard !closed, !stopping, !Task.isCancelled else { throw CancellationError() }
        await publish(journal)
        let allowed = await source.hasPermission()
        guard !closed, !stopping, !Task.isCancelled else { throw CancellationError() }
        await display.permissionChecked(allowed)
        guard allowed else {
            state = .needsPermission
            await publish()
            highlightVersion = nil
            await dispatcher.cancelObservation()
            await hide()
            return .hold
        }
        guard policy.started else {
            state = .stopped
            await publish()
            await hide()
            return .hold
        }
        guard try await !memory.manualPause() else {
            state = .paused
            await publish()
            await hide()
            return .hold
        }
        if !warmed {
            state = .preparing
            await publish()
            try await source.warmUp()
            try Task.checkCancellation()
            warmed = true
        }
        guard await source.hasPermission() else {
            state = .needsPermission
            await display.permissionChecked(false)
            await publish()
            await hide()
            return .hold
        }
        guard try await dispatcher.prepareObservation(at: clock.sample()) else {
            state = .waiting
            await publish()
            await hide()
            return .hold
        }
        try Task.checkCancellation()
        let idle = try await source.idleSeconds()
        let next = try await memory.observationTick(
            idleSeconds: idle, appSwitched: appSwitched, at: clock.sample())
        state = next == .hold ? .waiting : .observing
        failure = nil
        await publish()
        var resultNext = next
        if case .capture = next {
            do {
                let capturedRevision = sessionRevision
                if let frame = try await source.capture() {
                    try Task.checkCancellation()
                    guard await source.hasPermission() else {
                        state = .needsPermission
                        await display.permissionChecked(false)
                        await publish()
                        await hide()
                        return .hold
                    }
                    guard capturedRevision == sessionRevision, sessionValid else { return .hold }
                    let result = try await memory.submitObservationFrame(
                        frame, idleSeconds: idle, at: clock.sample())
                    resultNext = result.next
                    // An invalidation can finish while this actor awaits the synchronous Core reply.
                    if capturedRevision == sessionRevision,
                        result.disposition == .recorded || result.disposition == .duplicate
                    {
                        let wasReady = highlightReady
                        highlightReady = true
                        if !wasReady || highlightVersion != result.highlightVersion {
                            try await refreshHighlight()
                        }
                    }
                }
            } catch is CancellationError { throw CancellationError() } catch { await record(error) }
        }
        if dispatch == nil, !stopping { dispatch = Task { await self.send() } }
        return resultNext
    }
    private func send() async {
        defer { dispatch = nil }
        var completed = false
        do {
            let revision = sessionRevision
            if try await dispatcher.runObservation(at: clock.sample()) != nil {
                // The completed result still refreshes memory, but cannot reopen a newer native fence.
                if revision == sessionRevision { highlightReady = true }
                try await refreshHighlight()
                completed = true
            }
        } catch {
            if !stopping, !Task.isCancelled { await record(error) }
            // A failed dispatch may have removed the Core highlight as well.
            do { try await refreshHighlight() } catch { if !stopping { await record(error) } }
        }
        guard !closed, !stopping, !Task.isCancelled else { return }
        do {
            // Completion changes accounting and outcome even when the capture loop is sleeping.
            let journal = try await memory.observationJournal(at: clock.sample().utc)
            guard !closed, !stopping, !Task.isCancelled else { return }
            await publish(journal)
        } catch { await record(error) }
        if completed, !closed, !stopping, !Task.isCancelled { await changed() }
    }
    func memoryChanged(revision: UInt64, epoch: UInt64, sessionValid: Bool) async {
        guard revision > memoryRevision else { return }
        memoryRevision = revision
        let sessionChanged = sessionEpoch != epoch || self.sessionValid != sessionValid
        sessionEpoch = epoch
        self.sessionValid = sessionValid
        if sessionChanged {
            sessionRevision = revision
            highlightReady = false
            await hide()
            return
        }
        guard highlightReady else { return }
        do { try await refreshHighlight() } catch {
            await record(error)
            await hide()
        }
    }
    private func hide() async {
        presentationRevision += 1
        await presentation.render(nil, revision: presentationRevision)
    }
    private func refreshHighlight() async throws {
        presentationRevision += 1
        let revision = presentationRevision
        guard mode == .observe, !closed, !stopping, sessionValid, highlightReady else {
            await hide()
            return
        }
        let value = try await memory.observationHighlight()
        guard revision == presentationRevision else { return }
        let permission = await source.hasPermission()
        guard revision == presentationRevision else { return }
        highlightVersion = value.version
        let highlight =
            !stopping && sessionValid && permission && value.highlight?.needsRecheck == false
            ? value.highlight : nil
        await presentation.render(highlight, revision: revision)
    }
    private func publish(_ journal: ObservationJournal? = nil, working: Bool? = nil) async {
        guard !closed, !Task.isCancelled else { return }
        await display.update(
            state: state, failure: failure, working: working ?? (stopping || commandInProgress),
            journal: journal)
    }
    // UI tasks return command errors here instead of owning a second error state.
    func record(_ error: any Error, working: Bool? = nil) async {
        guard !closed else { return }
        failure = error as? ModelCheckFailure ?? .native
        state = .failed
        await publish(working: working)
    }
}

@MainActor private final class ObservationActivity {
    private var activity: NSObjectProtocol?
    private var observer: NSObjectProtocol?
    func start(activate: @escaping @Sendable () async -> Void) {
        activity = ProcessInfo.processInfo.beginActivity(
            options: .userInitiatedAllowingIdleSystemSleep, reason: "Background observation")
        observer = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { _ in
            Task { await activate() }
        }
    }
    func stop() {
        if let observer { NSWorkspace.shared.notificationCenter.removeObserver(observer) }
        observer = nil
        if let activity { ProcessInfo.processInfo.endActivity(activity) }
        activity = nil
    }
}

/// Main-actor ordering also rejects a show already queued before a newer hide.
@MainActor private final class ObservationPresentation {
    private let draw: @MainActor @Sendable (ObservationHighlight?) -> Void
    private var revision: UInt64 = 0
    private var value: ObservationHighlight?
    nonisolated init(draw: @escaping @MainActor @Sendable (ObservationHighlight?) -> Void) {
        self.draw = draw
    }
    func render(_ value: ObservationHighlight?, revision: UInt64) {
        guard revision >= self.revision else { return }
        self.revision = revision
        guard value != self.value else { return }
        self.value = value
        draw(value)
    }
}
