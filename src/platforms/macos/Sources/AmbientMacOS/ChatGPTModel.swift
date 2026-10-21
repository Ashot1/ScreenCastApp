import Foundation
import Observation

@Observable @MainActor public final class ChatGPTModel {
    private let coordinator: ChatGPTCoordinator
    /// The plugin settings share the coordinator's secret owner, so dispatch and edits serialize.
    let plugins: PluginsModel
    let voice: VoiceReplyModel
    private(set) var state: ChatGPTState = .unavailable
    private(set) var failure: ChatGPTFailure?
    private(set) var checkJournal: Result<ModelCheckSummary, ModelCheckFailure>?
    var checkSummary: ModelCheckSummary? {
        guard case .success(let summary) = checkJournal else { return nil }
        return summary
    }
    var journalFailure: ModelCheckFailure? {
        guard case .failure(let failure) = checkJournal else { return nil }
        return failure
    }
    private(set) var checkFailure: ModelCheckFailure?
    private(set) var creditReported = false
    private(set) var checking = false
    var checkProfile: ModelProfile = .low
    private(set) var assistanceReady = false
    private(set) var assistancePreview: AssistancePreview?
    private(set) var feed: [Proposal] = []
    private(set) var details: ProposalDetails?
    private(set) var assistanceFailure: ModelCheckFailure?
    private(set) var assisting = false
    private(set) var selectedAssistanceNotes: Set<UInt64> = []
    private var previewNotes: Set<UInt64> = []
    private(set) var replyDraft = ""
    private(set) var replyParent: ProposalTarget?
    private(set) var replyPreview: AssistancePreview?
    private(set) var replyFailure: ModelCheckFailure?
    private(set) var preparingReply = false
    private(set) var replyConflict = false
    private var replyVersion: UInt64 = 0
    private var previewVersion: UInt64?
    private var replyPreparation: Task<Void, Never>?
    var previewExpanded = false
    private var assistanceGeneration: UInt64 = 0
    private var feedGeneration: UInt64 = 0
    private var detailsGeneration: UInt64 = 0
    private var edgeDismissed = false
    /// The «Ambient Visual» build passes its own ChatGPT Keychain service and Codex home.
    public init(
        session: MemorySession = MemorySession(),
        chatGPTKeychainService: String = "dev.ambient.foundation.chatgpt",
        codexHome: URL = FileManager.default.homeDirectoryForCurrentUser
    ) {
        coordinator = ChatGPTCoordinator(
            store: ChatGPTKeychain(service: chatGPTKeychainService), memory: session)
        voice = .production()
        plugins = PluginsModel(memory: session, secrets: coordinator.plugins, codexHome: codexHome)
    }
    init(coordinator: ChatGPTCoordinator, voice: VoiceReplyModel? = nil) {
        self.coordinator = coordinator
        self.voice = voice ?? .production()
        plugins = PluginsModel(memory: coordinator.memory, secrets: coordinator.plugins)
    }
    var busy: Bool { state == .working || state == .cancelling }
    var replyProposal: Proposal? {
        guard let replyParent else { return nil }
        return feed.first { $0.target == replyParent && !$0.stale && $0.kind == .clarification }
    }
    var replyIsStale: Bool {
        replyParent != nil && (replyProposal == nil || !assistanceReady)
            || !replyDraft.isEmpty && replyParent == nil
    }
    var canSendReply: Bool {
        !busy && !voice.busy && !preparingReply && !replyIsStale && !replyDraft.isEmpty
            && replyParent != nil && replyPreview != nil && previewVersion == replyVersion
    }
    var edgeVisible: Bool {
        !edgeDismissed && (feed.contains { $0.visible } || !replyDraft.isEmpty || voice.busy)
    }
    func hideEdge() async {
        edgeDismissed = true
        for proposal in feed where proposal.visible {
            do {
                try await coordinator.setProposalVisibility(proposal.target, visible: false)
            } catch {
                assistanceFailure = error as? ModelCheckFailure ?? .native
                break
            }
        }
        await refreshFeed()
    }
    func chooseReply(_ proposal: Proposal) {
        guard !busy, !voice.busy, !proposal.stale, proposal.kind == .clarification else { return }
        guard replyDraft.isEmpty || replyParent == proposal.target else {
            replyConflict = true
            return
        }
        if replyParent != proposal.target {
            invalidateReply()
            replyParent = proposal.target
        }
        replyConflict = false
        edgeDismissed = false
    }
    private func defaultReplyTarget() {
        if replyParent == nil, replyDraft.isEmpty, feed.count == 1, let proposal = feed.first,
            proposal.kind == .clarification, !proposal.stale
        {
            replyParent = proposal.target
        }
    }
    var canStartVoice: Bool {
        !busy && voice.canRecord && assistanceReady && !replyIsStale
            && (replyProposal != nil
                || replyParent == nil && feed.count == 1
                    && feed.first.map({ $0.kind == .clarification && !$0.stale }) == true)
    }
    func startVoice(replacing: Bool = false) {
        defaultReplyTarget()
        guard canStartVoice, replyDraft.isEmpty || replacing, let target = replyParent,
            let models = voice.modelDirectory
        else { return }
        let version = replyVersion
        voice.start(
            models: models,
            validate: { [weak self] in
                guard let self, !self.busy, self.replyVersion == version,
                    self.replyParent == target, !self.replyIsStale
                else { return false }
                do {
                    let kind = try await self.coordinator.validateProposal(target)
                    return !self.busy && self.replyVersion == version && self.replyParent == target
                        && kind == .clarification && !self.replyIsStale
                } catch { return false }
            },
            apply: { [weak self] text in
                guard let self, !self.busy, self.replyVersion == version,
                    self.replyParent == target, !self.replyIsStale
                else { return false }
                self.setReply(text)
                return self.replyDraft == text
            })
    }
    public func cleanupLocalAudioAfterLaunch() async { await voice.startupCleanup() }
    public func reload() async {
        do { state = try await coordinator.load() } catch {
            state = .unavailable
            failure = error as? ChatGPTFailure ?? .response
        }
        if state != .saved && state != .working && state != .cancelling {
            assistancePreview = nil
            assistanceReady = false
            invalidateReply()
        }
        await refreshFeed()
    }
    func reloadChecks() async {
        creditReported = await coordinator.creditReport()
        do {
            checkJournal = .success(try await coordinator.checkSummary())
        } catch {
            checkJournal = .failure(error as? ModelCheckFailure ?? .storage)
        }
    }
    func reportCreditSetting(_ reported: Bool) async {
        do { try await coordinator.setCreditReport(reported) } catch {
            checkFailure = error as? ModelCheckFailure ?? .session
        }
        creditReported = await coordinator.creditReport()
        if !creditReported { await resetAssistance() }
        await reloadAssistanceAvailability()
    }
    func checkConnection() async {
        guard !busy else { return }
        state = .working
        checking = true
        checkFailure = nil
        do { _ = try await coordinator.runCheck(profile: checkProfile) } catch {
            if error as? ChatGPTFailure != .cancelled {
                checkFailure = error as? ModelCheckFailure ?? .native
            }
        }
        checking = false
        await reload()
        await reloadChecks()
        await reloadAssistanceAvailability()
        if !assistanceReady {
            assistancePreview = nil
            invalidateReply()
        }
    }
    func connect(refresh: Bool = false) async {
        guard !busy else { return }
        assistanceGeneration += 1
        invalidateReply()
        assistancePreview = nil
        assistanceReady = false
        state = .working
        failure = nil
        do { try await coordinator.run(refresh: refresh) } catch {
            failure = error as? ChatGPTFailure ?? .response
        }
        await reload()
        creditReported = await coordinator.creditReport()
        await reloadAssistanceAvailability()
    }
    func cancel() async {
        guard busy else { return }
        assistanceGeneration += 1
        invalidateReply()
        assistancePreview = nil
        state = .cancelling
        await coordinator.cancel()
    }
    func signOut() async {
        assistanceGeneration += 1
        invalidateReply()
        assistancePreview = nil
        assistanceReady = false
        failure = nil
        do { try await coordinator.signOut() } catch {
            failure = error as? ChatGPTFailure ?? .response
        }
        await reload()
        creditReported = await coordinator.creditReport()
    }
    func recover() async {
        failure = nil
        do { try await coordinator.recoverRegistration() } catch {
            failure = error as? ChatGPTFailure ?? .response
        }
        await reload()
        creditReported = await coordinator.creditReport()
    }
    func reloadAssistanceAvailability() async {
        do {
            assistanceReady = try await coordinator.assistanceAvailable()
            if !assistanceReady {
                assistancePreview = nil
                invalidateReply()
            }
        } catch {
            assistanceReady = false
            assistanceFailure = error as? ModelCheckFailure ?? .session
            if error as? ModelCheckFailure != .busy {
                assistancePreview = nil
                invalidateReply()
            }
        }
    }
    func selectedTaskChanged() {
        selectedAssistanceNotes = []
        previewNotes = []
        assistancePreview = nil
    }
    func resetAssistance(clearNotes: Bool = false) async {
        assistanceGeneration += 1
        invalidateReply()
        assistancePreview = nil
        assistanceFailure = nil
        previewNotes = []
        if clearNotes { selectedAssistanceNotes = [] }
        do { try await coordinator.clearAssistance() } catch {
            assistanceFailure = error as? ModelCheckFailure ?? .storage
        }
        await reloadAssistanceAvailability()
        await refreshFeed()
    }
    func selectAssistanceNote(_ id: UInt64, selected: Bool) async {
        guard !busy else { return }
        if selected {
            guard selectedAssistanceNotes.count < 32 else {
                assistanceFailure = .limit
                return
            }
            selectedAssistanceNotes.insert(id)
        } else {
            selectedAssistanceNotes.remove(id)
        }
        await resetAssistance()
    }
    func beginLocalRead(_ task: MemoryTask, root: LocalRootSettings) async throws
        -> LocalSourceOperation
    {
        try await coordinator.beginLocalRead(task, root: root)
    }
    func bindLocalBranch(_ operation: LocalSourceOperation, index: Int) async throws -> UInt64 {
        try await coordinator.bindLocalBranch(operation, index: index)
    }
    func beginIntegrationRead(_ task: MemoryTask, profile: IntegrationProfile, target: String)
        async throws -> IntegrationOperation
    {
        try await coordinator.beginIntegrationRead(task, profile: profile, target: target)
    }
    func bindIntegrationObjects(_ op: IntegrationOperation, indices: [Int]) async throws
        -> [UInt64]
    {
        try await coordinator.bindIntegrationObjects(op, indices: indices)
    }
    func beginCodexRead(_ task: MemoryTask, selection: CodexSelection, cursor: UInt64) async throws
        -> CodexOperation
    {
        try await coordinator.beginCodexRead(task, selection: selection, cursor: cursor)
    }
    func bindCodexEvent(_ operation: CodexOperation, index: Int) async throws -> UInt64 {
        try await coordinator.bindCodexEvent(operation, index: index)
    }

    func refreshFeed() async {
        feedGeneration += 1
        let generation = feedGeneration
        do {
            let rows = try await coordinator.proposalFeed()
            guard generation == feedGeneration else { return }
            if rows.contains(where: { row in
                row.visible && !feed.contains(where: { old in old.id == row.id && old.visible })
            }) {
                edgeDismissed = false
            }
            feed = rows
            detailsGeneration += 1
            if let details, !rows.contains(details.proposal) { self.details = nil }
            if replyParent != nil, replyProposal == nil {
                invalidateReply()
            } else if !replyDraft.isEmpty, let proposal = replyProposal, let preview = replyPreview,
                preview.workspaceRevision != proposal.workspaceRevision
            {
                setReply(replyDraft)
            }
        } catch {
            guard generation == feedGeneration else { return }
            assistanceFailure = error as? ModelCheckFailure ?? .storage
        }
    }
    func loadDetails(_ proposal: Proposal) async {
        detailsGeneration += 1
        let generation = detailsGeneration
        do {
            let value = try await coordinator.proposalDetails(proposal.target)
            guard generation == detailsGeneration, feed.contains(value.proposal) else { return }
            details = value
        } catch {
            if generation == detailsGeneration {
                assistanceFailure = error as? ModelCheckFailure ?? .native
            }
        }
    }
    func setVisibility(_ proposal: Proposal, visible: Bool) async {
        if visible { edgeDismissed = false }
        do { try await coordinator.setProposalVisibility(proposal.target, visible: visible) } catch
        { assistanceFailure = error as? ModelCheckFailure ?? .native }
        await refreshFeed()
    }
    func clearDiscussion(_ proposal: Proposal) async {
        do {
            let task = try await coordinator.savedTask(proposal.task.taskID)
            try await coordinator.clearDiscussion(
                task, workspaceRevision: proposal.workspaceRevision)
            await resetAssistance()
        } catch { assistanceFailure = error as? ModelCheckFailure ?? .native }
        await refreshFeed()
    }
    func prepareNewContext(_ proposal: Proposal) async {
        do {
            let task = try await coordinator.savedTask(proposal.task.taskID)
            selectedAssistanceNotes = []
            await prepareAssistance(task)
        } catch { assistanceFailure = error as? ModelCheckFailure ?? .native }
    }
    func prepareAssistance(_ task: MemoryTask) async {
        guard !busy, !voice.busy else { return }
        assistanceGeneration += 1
        let generation = assistanceGeneration
        assistancePreview = nil
        assistanceFailure = nil
        let notes = selectedAssistanceNotes
        state = .working
        invalidateReply()
        // The new request owns the sole Core preview after the old preparation actually finishes.
        replyPreparation?.cancel()
        if let replyPreparation { await replyPreparation.value }
        if generation == assistanceGeneration {
            do {
                try await coordinator.clearReply()
                let preview = try await coordinator.prepareAssistance(task, notes: notes.sorted())
                if generation == assistanceGeneration {
                    assistancePreview = preview
                    previewNotes = notes
                }
            } catch {
                if generation == assistanceGeneration {
                    assistanceFailure = error as? ModelCheckFailure ?? .native
                }
            }
        }
        await reload()
        await reloadAssistanceAvailability()
    }
    func prepareContinuation(_ proposal: Proposal) async {
        guard !busy, !voice.busy else { return }
        assistanceGeneration += 1
        let generation = assistanceGeneration
        assistancePreview = nil
        assistanceFailure = nil
        state = .working
        invalidateReply()
        replyPreparation?.cancel()
        if let replyPreparation { await replyPreparation.value }
        if generation == assistanceGeneration {
            do {
                try await coordinator.clearReply()
                let preview = try await coordinator.prepareContinuation(proposal)
                if generation == assistanceGeneration { assistancePreview = preview }
            } catch {
                if generation == assistanceGeneration {
                    assistanceFailure = error as? ModelCheckFailure ?? .native
                }
            }
        }
        await reload()
        await reloadAssistanceAvailability()
    }
    func sendAssistance(_ task: MemoryTask) async {
        guard let preview = assistancePreview, preview.taskID == task.id,
            preview.revision == task.revision,
            preview.mode != .newContext || previewNotes == selectedAssistanceNotes
        else {
            assistanceFailure = .stale
            return
        }
        await sendPreparedAssistance()
    }
    func sendPreparedAssistance() async {
        guard !busy, let preview = assistancePreview else {
            assistanceFailure = .stale
            return
        }
        let generation = assistanceGeneration
        assistancePreview = nil
        assistanceFailure = nil
        assisting = true
        state = .working
        do { _ = try await coordinator.runAssistance(preview.requestID) } catch {
            if generation == assistanceGeneration {
                assistanceFailure = error as? ModelCheckFailure ?? .cancelled
            }
        }
        assisting = false
        await reload()
        await reloadChecks()
        await reloadAssistanceAvailability()
    }
    private func invalidateReply(dropTarget: Bool = false) {
        voice.cancel()
        replyVersion += 1
        replyPreview = nil
        previewVersion = nil
        if dropTarget { replyParent = nil }
    }
    func updateReply(_ text: String) {
        guard !busy, !voice.busy else { return }
        defaultReplyTarget()
        setReply(text)
    }
    private func setReply(_ text: String) {
        guard text.utf8.count <= 16384 else {
            replyFailure = .limit
            return
        }
        assistancePreview = nil
        replyDraft = text
        replyVersion += 1
        replyPreview = nil
        previewVersion = nil
        replyFailure = nil
        if replyIsStale { return }
        if replyPreparation == nil {
            preparingReply = true
            replyPreparation = Task { await self.prepareLatestReply() }
        }
    }
    private func prepareLatestReply() async {
        while !Task.isCancelled {
            let version = replyVersion
            let text = replyDraft
            let target = replyParent
            do {
                if let proposal = replyProposal,
                    !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                {
                    let preview = try await coordinator.prepareContinuation(proposal, text: text)
                    if !Task.isCancelled, replyVersion == version, replyParent == target {
                        replyPreview = preview
                        previewVersion = version
                    }
                } else {
                    try await coordinator.clearReply()
                }
            } catch {
                if !Task.isCancelled, replyVersion == version {
                    replyFailure = error as? ModelCheckFailure ?? .native
                }
            }
            if replyVersion == version || replyParent == nil || replyIsStale { break }
        }
        preparingReply = false
        replyPreparation = nil
    }
    func waitForReplyPreview() async { if let replyPreparation { await replyPreparation.value } }
    func discardReply() async {
        guard !busy, !voice.busy else { return }
        invalidateReply(dropTarget: true)
        replyDraft = ""
        replyFailure = nil
        replyConflict = false
        let version = replyVersion
        if let replyPreparation { await replyPreparation.value }
        guard replyVersion == version else { return }
        do { try await coordinator.clearReply() } catch {
            replyFailure = error as? ModelCheckFailure ?? .native
        }
    }
    func sendReply() async {
        guard canSendReply, let preview = replyPreview else {
            replyFailure = .stale
            return
        }
        let version = replyVersion
        let generation = assistanceGeneration
        replyPreview = nil
        previewVersion = nil
        replyFailure = nil
        assisting = true
        state = .working
        do {
            _ = try await coordinator.runAssistance(preview.requestID)
            if generation == assistanceGeneration, replyVersion == version {
                replyDraft = ""
                invalidateReply(dropTarget: true)
            }
        } catch {
            if generation == assistanceGeneration {
                replyFailure = error as? ModelCheckFailure ?? .cancelled
                invalidateReply()
            }
        }
        assisting = false
        await reload()
        await reloadChecks()
        await reloadAssistanceAvailability()
    }
    func refreshCurrentResult() async {
        await reloadAssistanceAvailability()
        await refreshFeed()
    }
    func suppress(_ proposal: Proposal) async {
        do {
            try await coordinator.suppressProposalTask(proposal)
            await resetAssistance()
        } catch { assistanceFailure = error as? ModelCheckFailure ?? .native }
        await refreshFeed()
    }
    func copy(_ proposal: Proposal, using model: AssistantModel) async {
        guard !busy else { return }
        do {
            let text = try await coordinator.copyProposal(proposal.target)
            guard feed.contains(where: { $0.target == proposal.target && !$0.stale }) else {
                return
            }
            model.copy(text)
        } catch { assistanceFailure = error as? ModelCheckFailure ?? .native }
    }
    public func shutdown() async -> Bool {
        assistanceGeneration += 1
        feedGeneration += 1
        detailsGeneration += 1
        invalidateReply()
        assistancePreview = nil
        assistanceReady = false
        let voiceShutdown = Task { await self.voice.shutdown() }
        if let replyPreparation { await replyPreparation.value }
        do {
            try await coordinator.shutdown()
            return await voiceShutdown.value
        } catch {
            failure = error as? ChatGPTFailure ?? .response
            _ = await voiceShutdown.value
            return false
        }
    }
}

extension ChatGPTState {
    var key: String {
        switch self {
        case .unavailable: "auth.unavailable"
        case .signedOut: "auth.signedOut"
        case .saved: "auth.saved"
        case .expired: "auth.expired"
        case .pending: "auth.pending"
        case .unknown: "auth.unknown"
        case .working: "auth.working"
        case .cancelling: "auth.cancelling"
        }
    }
}
extension ChatGPTFailure {
    var key: String {
        switch self {
        case .storage: "auth.error.storage"
        case .corrupt: "auth.error.corrupt"
        case .invalid: "auth.error.invalid"
        case .busy: "auth.error.busy"
        case .cancelled: "auth.error.cancelled"
        case .expired: "auth.error.expired"
        case .network: "auth.error.network"
        case .denied: "auth.error.denied"
        case .response: "auth.error.response"
        case .identity: "auth.error.identity"
        case .browser: "auth.error.browser"
        case .registrationUnknown: "auth.error.registrationUnknown"
        case .closed: "auth.error.closed"
        }
    }
}
