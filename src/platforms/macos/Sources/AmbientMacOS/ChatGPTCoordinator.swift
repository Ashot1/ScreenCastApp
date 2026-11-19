import AppKit
import CAmbient
import Dispatch
import Foundation

enum ChatGPTState: Equatable, Sendable {
    case unavailable, signedOut, saved, expired, pending, unknown, working, cancelling
}
private enum ModelRequest {
    case check(ModelProfile)
    case assistance(UInt64)
    case observation(ObservationTime)
}
actor ChatGPTCoordinator: ObservationDispatching {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.auth.credentials")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    private let store: any ChatGPTCredentialStore
    private let bridge: any ChatGPTProtocol
    private let makeListener: @Sendable () -> any ChatGPTCallbackListener
    private let openBrowser: @MainActor @Sendable (URL) -> Bool
    let memory: MemorySession
    private let modelBridge: any ModelCheckProtocol
    let plugins: PluginSecretOwner
    private var modelHandle: UInt64?
    private var observationInFlight = false
    private var reportedCreditSetting = false
    private var connectionEpoch = UInt64.random(in: 1...UInt64.max)
    private var modelSessionPublished = false
    private let operationTimeout: Duration
    private var busy = false
    private var cancelling = false
    private var closed = false
    private var generation: UInt64 = 0
    private var handle: UInt64?
    private var listener: (any ChatGPTCallbackListener)?
    private var expiration: ContinuousClock.Instant?
    private var expired = false
    private var waiters: [CheckedContinuation<Void, Never>] = []
    init(
        store: any ChatGPTCredentialStore = ChatGPTKeychain(),
        bridge: any ChatGPTProtocol = ChatGPTBridge(),
        makeListener: @escaping @Sendable () -> any ChatGPTCallbackListener = { ChatGPTLoopback() },
        operationTimeout: Duration = .seconds(300),
        memory: MemorySession = MemorySession(),
        modelBridge: any ModelCheckProtocol = ModelCheckBridge(),
        plugins: PluginSecretOwner? = nil,
        openBrowser: @escaping @MainActor @Sendable (URL) -> Bool = { NSWorkspace.shared.open($0) }
    ) {
        self.memory = memory
        self.modelBridge = modelBridge
        self.plugins = plugins ?? PluginSecretOwner(memory: memory)
        self.store = store
        self.bridge = bridge
        self.makeListener = makeListener
        self.operationTimeout = operationTimeout
        self.openBrowser = openBrowser
    }
    func load() async throws -> ChatGPTState {
        do {
            let state = try readState()
            if state != .saved && state != .working && state != .cancelling && modelSessionPublished
            {
                try await memory.setModelSession(
                    epoch: connectionEpoch, valid: false, reported: reportedCreditSetting)
            }
            return state
        } catch {
            if modelSessionPublished {
                try await memory.setModelSession(
                    epoch: connectionEpoch, valid: false, reported: false)
            }
            throw error
        }
    }
    private func readState() throws -> ChatGPTState {
        if busy { return cancelling ? .cancelling : .working }
        return try savedState()
    }
    private func savedState() throws -> ChatGPTState {
        guard !closed else { throw ChatGPTFailure.closed }
        guard let e = try store.load() else { return .signedOut }
        try e.validate()
        if e.registration == 1 { return .unknown }
        if let c = e.credentials {
            return c.expiresAt > Int64(Date().timeIntervalSince1970) ? .saved : .expired
        }
        return e.registration == 2 || e.hint != nil ? .pending : .signedOut
    }
    private func fenceGeneration(_ revision: UInt64) throws {
        guard generation == revision, !closed else { throw ChatGPTFailure.cancelled }
    }
    private func fence(_ revision: UInt64) throws {
        do { try fenceGeneration(revision) } catch {
            throw expired ? ChatGPTFailure.expired : ChatGPTFailure.cancelled
        }
        guard let expiration, ContinuousClock.now < expiration else { throw ChatGPTFailure.expired }
    }
    func run(refresh: Bool) async throws {
        guard !closed else { throw ChatGPTFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ChatGPTFailure.busy }
        if !refresh {
            reportedCreditSetting = false
            connectionEpoch = UInt64.random(in: 1...UInt64.max)
        }
        busy = true
        cancelling = false
        expired = false
        generation += 1
        let revision = generation
        expiration = .now + operationTimeout
        let timer = Task {
            do { try await Task.sleep(for: operationTimeout) } catch { return }
            await expire(revision)
        }
        var failure: Error?
        do {
            if !refresh && modelSessionPublished {
                try await memory.setModelSession(
                    epoch: connectionEpoch, valid: false, reported: false)
            }
            var envelope: ChatGPTEnvelope
            if let saved = try store.load() {
                envelope = saved
                try envelope.validate()
            } else {
                guard !refresh else { throw ChatGPTFailure.invalid }
                envelope = ChatGPTEnvelope(host: "urn:uuid:" + UUID().uuidString.lowercased())
                try store.save(envelope)
            }
            if envelope.registration == 1 { throw ChatGPTFailure.registrationUnknown }
            if refresh && envelope.credentials == nil { throw ChatGPTFailure.invalid }
            try fence(revision)
            let callback: String?
            if refresh {
                callback = nil
            } else {
                let listener = makeListener()
                self.listener = listener
                callback = try await listener.start()
            }
            try fence(revision)
            let handle = try await bridge.begin(envelope, redirect: callback, refresh: refresh)
            self.handle = handle
            try fence(revision)
            let authorization = try await bridge.prepare(handle)
            try fence(revision)
            if refresh {
                // Remove rotating credentials atomically before the only token dispatch.
                envelope.credentials = nil
                try store.save(envelope)
            } else {
                guard let authorization, let url = URL(string: authorization),
                    url.scheme == "https", url.host == "auth.openai.com",
                    url.path == "/api/accounts/authorize",
                    url.port == nil, url.user == nil, url.password == nil, url.fragment == nil,
                    let listener
                else { throw ChatGPTFailure.response }
                let dynamic = envelope.clientID == nil
                if dynamic {
                    envelope.registration = 1
                    try store.save(envelope)
                }
                let opened = await openBrowser(url)
                if !opened {
                    if dynamic {
                        envelope.registration = 0
                        try store.save(envelope)
                    }
                    throw ChatGPTFailure.browser
                }
                try fence(revision)
                while true {
                    let callbackURL = try await listener.next()
                    try fence(revision)
                    do {
                        let issued = try await bridge.callback(handle, url: callbackURL)
                        try fence(revision)
                        if envelope.registration != 3 {
                            envelope.registration = 2
                            envelope.clientID = issued
                        }
                        try store.save(envelope)
                        await listener.respond()
                        await listener.close()
                        break
                    } catch ChatGPTFailure.invalid {
                        await listener.respond()
                    } catch ChatGPTFailure.identity {
                        await listener.respond()
                    }
                }
            }
            try fence(revision)
            guard let clientID = envelope.clientID else { throw ChatGPTFailure.response }
            let result = try await bridge.dispatch(handle, clientID: clientID)
            if refresh
                && (result.host != envelope.host || result.clientID != envelope.clientID
                    || result.issuer != envelope.issuer || result.subject != envelope.subject)
            {
                reportedCreditSetting = false
                throw ChatGPTFailure.identity
            }
            try fence(revision)
            try store.save(result)
        } catch ChatGPTFailure.cancelled where expired {
            // Expiry closes the listener through cancellation; retain the deadline as its cause.
            failure = ChatGPTFailure.expired
        } catch { failure = error }
        timer.cancel()
        // Busy covers actual Rust return and every accepted native connection's terminal state.
        if let listener { await listener.close() }
        if let handle {
            bridge.cancel(handle)
            do {
                try await bridge.close(handle)
                self.handle = nil
            } catch { if failure == nil { failure = error } }
        }
        if failure != nil && modelSessionPublished {
            do {
                try await memory.setModelSession(
                    epoch: connectionEpoch, valid: false, reported: reportedCreditSetting)
            } catch { failure = error }
        }
        if failure == nil { do { try fenceGeneration(revision) } catch { failure = error } }
        self.listener = nil
        expiration = nil
        busy = false
        cancelling = false
        for waiter in waiters { waiter.resume() }
        waiters.removeAll()
        if let failure { throw failure }
    }
    func creditReport() -> Bool { reportedCreditSetting }
    func setCreditReport(_ reported: Bool) async throws {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy || !reported else { throw ModelCheckFailure.busy }
        if reported { guard try readState() == .saved else { throw ModelCheckFailure.session } }
        reportedCreditSetting = reported
        if !reported, !observationInFlight { await cancel() }
        if modelSessionPublished {
            let effectiveReport = try await effectiveCreditReport()
            try await memory.setModelSession(
                epoch: connectionEpoch, valid: try savedState() == .saved, reported: effectiveReport
            )
        }
    }
    private func effectiveCreditReport(_ reported: Bool? = nil) async throws -> Bool {
        if let reported { return reported }
        if reportedCreditSetting { return true }
        return try await memory.observationJournal(
            at: Int64(Date().timeIntervalSince1970)
        ).policy.creditAcknowledged
    }
    private func currentEnvelope() async throws -> ChatGPTEnvelope {
        do {
            guard let envelope = try store.load() else { throw ModelCheckFailure.session }
            try envelope.validate()
            guard let credentials = envelope.credentials,
                credentials.expiresAt > Int64(Date().timeIntervalSince1970)
            else { throw ModelCheckFailure.session }
            return envelope
        } catch {
            if modelSessionPublished {
                try await memory.setModelSession(
                    epoch: connectionEpoch, valid: false, reported: reportedCreditSetting)
            }
            throw error
        }
    }
    private func publishSession(reported: Bool? = nil) async throws -> ChatGPTEnvelope {
        let envelope = try await currentEnvelope()
        // Both acknowledgements belong to this one session fact; observation's survives UI refreshes.
        let effectiveReport = try await effectiveCreditReport(reported)
        try await memory.setModelSession(
            epoch: connectionEpoch, valid: true, reported: effectiveReport)
        modelSessionPublished = true
        return envelope
    }
    private func finishBusy() {
        busy = false
        cancelling = false
        for waiter in waiters { waiter.resume() }
        waiters.removeAll()
    }
    func checkSummary() async throws -> ModelCheckSummary { try await memory.checkSummary() }
    func assistanceAvailable() async throws -> Bool {
        guard !closed, !busy, modelSessionPublished else { return false }
        _ = try await publishSession()
        return try await memory.assistanceAvailable()
    }
    func beginLocalRead(_ task: MemoryTask, root: LocalRootSettings) async throws
        -> LocalSourceOperation
    {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        let envelope = try await publishSession()
        try fenceGeneration(revision)
        guard let expiresAt = envelope.credentials?.expiresAt else {
            throw ModelCheckFailure.session
        }
        let operation = try await memory.beginLocalRead(task, root: root, expiresAt: expiresAt)
        do { try fenceGeneration(revision) } catch {
            operation.cancel()
            // No native read started; abort can safely finish this unused admission.
            try LocalSourceBridge.check(ambient_source_v1_abort(operation.memory, operation.id))
            try operation.release()
            throw error
        }
        return operation
    }
    func bindLocalBranch(_ operation: LocalSourceOperation, index: Int) async throws -> UInt64 {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        _ = try await publishSession()
        try fenceGeneration(revision)
        let id = try await memory.bindLocalBranch(operation, index: index)
        try fenceGeneration(revision)
        return id
    }
    func beginCodexRead(_ task: MemoryTask, selection: CodexSelection, cursor: UInt64) async throws
        -> CodexOperation
    {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        let envelope = try await publishSession()
        try fenceGeneration(revision)
        guard let expiresAt = envelope.credentials?.expiresAt else {
            throw ModelCheckFailure.session
        }
        let operation = try await memory.beginCodexRead(
            task, selection: selection, cursor: cursor, expiresAt: expiresAt)
        do { try fenceGeneration(revision) } catch {
            operation.cancel()
            // No native read started; abort can safely finish this unused admission.
            try CodexBridge.check(ambient_codex_v1_abort(operation.memory, operation.id))
            try operation.release()
            throw error
        }
        return operation
    }
    func bindCodexEvent(_ operation: CodexOperation, index: Int) async throws -> UInt64 {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        _ = try await publishSession()
        try fenceGeneration(revision)
        let id = try await memory.bindCodexEvent(operation, index: index)
        try fenceGeneration(revision)
        return id
    }
    func beginIntegrationRead(_ task: MemoryTask, profile: IntegrationProfile, target: String)
        async throws -> IntegrationOperation
    {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        let envelope = try await publishSession()
        try fenceGeneration(revision)
        guard let expiresAt = envelope.credentials?.expiresAt else {
            throw ModelCheckFailure.session
        }
        let op = try await memory.beginIntegrationRead(
            task, profile: profile, target: target, expiresAt: expiresAt)
        do { try fenceGeneration(revision) } catch {
            op.cancel()
            try op.abort()
            try op.release()
            throw error
        }
        return op
    }
    func bindIntegrationObjects(_ op: IntegrationOperation, indices: [Int]) async throws
        -> [UInt64]
    {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        _ = try await publishSession()
        try fenceGeneration(revision)
        let ids = try await memory.bindIntegrationObjects(op, indices: indices)
        try fenceGeneration(revision)
        return ids
    }
    func prepareAssistance(_ task: MemoryTask, notes: [UInt64]) async throws -> AssistancePreview {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        _ = try await publishSession()
        try fenceGeneration(revision)
        let preview = try await memory.prepareAssistance(task, notes: notes)
        try fenceGeneration(revision)
        return preview
    }
    private var replyPreviewRequest: UInt64?
    func prepareContinuation(_ proposal: Proposal, text: String? = nil) async throws
        -> AssistancePreview
    {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        _ = try await publishSession()
        try fenceGeneration(revision)
        let preview = try await memory.prepareContinuation(proposal, text: text)
        try fenceGeneration(revision)
        if text != nil { replyPreviewRequest = preview.requestID }
        return preview
    }
    // Durable reads neither reserve transport nor touch credentials or its generation.
    func proposalFeed() async throws -> [Proposal] { try await memory.proposalFeed() }
    func proposalDetails(_ target: ProposalTarget) async throws -> ProposalDetails {
        try await memory.proposalDetails(target)
    }
    func savedTask(_ id: UInt64) async throws -> MemoryTask { try await memory.task(id) }
    func clearReply() async throws {
        guard let request = replyPreviewRequest else { return }
        replyPreviewRequest = nil
        try await memory.discardAssistancePreview(request)
    }
    func clearAssistance() async throws {
        replyPreviewRequest = nil
        try await memory.clearAssistance()
    }
    func validateProposal(_ target: ProposalTarget) async throws -> AssistanceKind {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy else { throw ModelCheckFailure.busy }
        let revision = generation
        _ = try await currentEnvelope()
        try fenceGeneration(revision)
        let kind = try await memory.validateProposal(target)
        try fenceGeneration(revision)
        return kind
    }
    func setProposalVisibility(_ target: ProposalTarget, visible: Bool) async throws {
        if visible {
            guard !closed else { throw ModelCheckFailure.closed }
            guard !busy else { throw ModelCheckFailure.busy }
            let revision = generation
            _ = try await publishSession()
            try fenceGeneration(revision)
            try await memory.setProposalVisibility(target, visible: true)
            try fenceGeneration(revision)
        } else {
            try await memory.setProposalVisibility(target, visible: false)
        }
    }
    func clearDiscussion(_ task: MemoryTask, workspaceRevision: UInt64) async throws {
        try await memory.clearDiscussion(task, workspaceRevision: workspaceRevision)
    }
    func suppressProposalTask(_ proposal: Proposal) async throws {
        let task = try await memory.task(proposal.task.taskID)
        guard task.revision == proposal.task.revision else { throw ModelCheckFailure.stale }
        try await memory.setInitiativeSuppressed(task, value: true)
    }
    func copyProposal(_ target: ProposalTarget) async throws -> String {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        _ = try await publishSession()
        try fenceGeneration(revision)
        let text = try await memory.copyProposal(target)
        try fenceGeneration(revision)
        return text
    }
    func runCheck(profile: ModelProfile = .low) async throws -> ModelCheckAttempt {
        guard let result = try await runModel(.check(profile)) else {
            throw ModelCheckFailure.native
        }
        return result.attempt
    }
    func runAssistance(_ request: UInt64) async throws -> ModelOperationResult {
        guard let result = try await runModel(.assistance(request)) else {
            throw ModelCheckFailure.native
        }
        guard result.publishedTaskID != nil else { throw ModelCheckFailure.result }
        return result
    }
    func prepareObservation(at: ObservationTime) async throws -> Bool {
        guard !closed else { throw ModelCheckFailure.closed }
        if busy { return true }  // The current operation already owns session publication.
        busy = true
        generation += 1
        let revision = generation
        defer { finishBusy() }
        let policy = try await memory.observationJournal(at: at.utc).policy
        try fenceGeneration(revision)
        guard policy.started, policy.creditAcknowledged else { return false }
        _ = try await publishSession(reported: policy.creditAcknowledged)
        try fenceGeneration(revision)
        return true
    }
    func runObservation(at: ObservationTime) async throws -> ModelOperationResult? {
        // A cancelled caller may reach this actor before it ever owned an operation to cancel.
        try Task.checkCancellation()
        guard !busy else { return nil }
        return try await runModel(.observation(at))
    }
    func cancelObservation() async {
        if observationInFlight { await cancel() }
    }
    private func runModel(_ request: ModelRequest) async throws
        -> ModelOperationResult?
    {
        guard !closed else { throw ModelCheckFailure.closed }
        guard !busy, handle == nil, modelHandle == nil else { throw ModelCheckFailure.busy }
        if case .observation = request {
        } else {
            guard reportedCreditSetting else { throw ModelCheckFailure.report }
        }
        busy = true
        cancelling = false
        expired = false
        generation += 1
        let revision = generation
        if case .observation = request { observationInFlight = true }
        defer {
            observationInFlight = false
            finishBusy()
        }
        var result: ModelOperationResult?
        var failure: Error?
        do {
            let reported: Bool
            if case .observation(let time) = request {
                let policy = try await memory.observationJournal(at: time.utc).policy
                try fenceGeneration(revision)
                guard policy.started, policy.creditAcknowledged else { return nil }
                reported = policy.creditAcknowledged
            } else {
                reported = reportedCreditSetting
            }
            let accepted = try await publishSession(reported: reported)
            try fenceGeneration(revision)
            let operation: UInt64
            switch request {
            case .assistance(let id):
                operation = try await memory.beginAssistance(
                    id, at: Int64(Date().timeIntervalSince1970))
            case .check(let profile):
                operation = try await memory.beginCheck(
                    userReported: reportedCreditSetting, profile: profile,
                    at: Int64(Date().timeIntervalSince1970))
            case .observation(let time):
                guard case .ready(let id) = try await memory.beginObservation(at: time) else {
                    return nil
                }
                operation = id
            }
            modelHandle = operation
            try fenceGeneration(revision)
            // Re-read actual credentials immediately before transport; no second session owner or deadline.
            let current = try await currentEnvelope()
            guard current.host == accepted.host, current.clientID == accepted.clientID,
                current.issuer == accepted.issuer, current.subject == accepted.subject,
                let credentials = current.credentials
            else { throw ModelCheckFailure.session }
            try fenceGeneration(revision)
            // Keys of enabled plugins go to assistance only; a connectivity check never calls tools.
            let keys: [PluginDispatchKey]
            if case .assistance = request {
                keys = try await plugins.dispatchKeys()
            } else {
                keys = []
            }
            let root = try await memory.pluginServersRoot()
            try fenceGeneration(revision)
            result = try await modelBridge.dispatch(
                operation, access: credentials.access, keys: keys, root: root)
            try fenceGeneration(revision)
        } catch { failure = error }
        if let modelHandle {
            do {
                try await modelBridge.close(modelHandle)
                self.modelHandle = nil
            } catch { if failure == nil { failure = error } }
        }
        if case .assistance = request { await memory.observationDidMutate() }
        // A logout can interleave with close; retain the generation fence through actual cleanup.
        if failure == nil { do { try fenceGeneration(revision) } catch { failure = error } }
        if let failure { throw failure }
        guard let result else { throw ModelCheckFailure.native }
        return result
    }
    private func expire(_ revision: UInt64) async {
        guard busy, generation == revision else { return }
        expired = true
        await cancel()
    }
    func cancel() async {
        guard busy else { return }
        generation += 1
        cancelling = true
        if let handle { bridge.cancel(handle) }
        if let modelHandle { modelBridge.cancel(modelHandle) }
        if let listener { await listener.close() }
    }
    func signOut() async throws {
        reportedCreditSetting = false
        connectionEpoch = UInt64.random(in: 1...UInt64.max)
        await cancel()
        if modelSessionPublished {
            try await memory.setModelSession(epoch: connectionEpoch, valid: false, reported: false)
        }
        if var envelope = try store.load() {
            envelope.localSignOut()
            try store.save(envelope)
        }
    }
    func recoverRegistration() throws {
        guard !closed else { throw ChatGPTFailure.closed }
        guard !busy else { throw ChatGPTFailure.busy }
        if var envelope = try store.load(), envelope.registration == 1 {
            envelope.registration = 0
            try store.save(envelope)
        }
    }
    func shutdown() async throws {
        closed = true
        reportedCreditSetting = false
        await cancel()
        if modelSessionPublished {
            try await memory.setModelSession(epoch: connectionEpoch, valid: false, reported: false)
        }
        if busy { await withCheckedContinuation { waiters.append($0) } }
        if let handle {
            try await bridge.close(handle)
            self.handle = nil
        }
        if let modelHandle {
            try await modelBridge.close(modelHandle)
            self.modelHandle = nil
        }
    }
}
