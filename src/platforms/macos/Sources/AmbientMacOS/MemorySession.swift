import CAmbient
import Foundation

/// Owns the native handle and serializes synchronous storage calls away from the UI actor.
public actor MemorySession {
    private let databaseURL: URL?
    private var handle: UInt64 = 0
    private var closed = false
    private weak var observationSession: ObservationSession?
    private var observationRevision: UInt64 = 0
    // Snapshot of the last successfully published ABI fact, not another credential owner.
    private var observationSessionValid = false
    private var observationSessionEpoch: UInt64 = 0

    func observeChanges(_ session: ObservationSession) async {
        observationSession = session
        await observationDidMutate()
    }
    func observationDidMutate() async {
        observationRevision += 1
        await observationSession?.memoryChanged(
            revision: observationRevision, epoch: observationSessionEpoch,
            sessionValid: observationSessionValid)
    }
    func withObservationMutation<T: Sendable>(_ body: () throws -> T) async throws -> T {
        let result: T
        do { result = try body() } catch {
            // A write may have committed before returned projection validation failed.
            await observationDidMutate()
            throw error
        }
        await observationDidMutate()
        return result
    }
    public init(databaseURL: URL? = nil) { self.databaseURL = databaseURL }
    deinit {
        // Explicit close reports errors; this releases the last owner's handle if it disappears.
        if handle != 0 { _ = ambient_v3_close(handle) }
    }
    public func load(
        selectedTaskID: UInt64? = nil, tasksAfter: UInt64 = 0,
        evidenceAfter: UInt64 = 0, linksAfter: UInt64 = 0, decisionsAfter: UInt64 = 0,
        selectedStepID: UInt64? = nil, stepsAfter: UInt64 = 0,
        dependenciesAfter: UInt64 = 0, referencesAfter: UInt64 = 0
    ) throws -> MemorySnapshot {
        try openIfNeeded()
        let tasks = try tasks(after: tasksAfter)
        let selected = try (selectedTaskID ?? tasks.items.first?.id).map { try task($0) }
        let evidence =
            try selected.map { try evidence(taskID: $0.id, after: evidenceAfter) } ?? .empty
        let links = try selected.map { try links(taskID: $0.id, after: linksAfter) } ?? .empty
        let decisions =
            try selected.map { try decisions(taskID: $0.id, after: decisionsAfter) } ?? .empty
        let steps = try selected.map { try steps(taskID: $0.id, after: stepsAfter) } ?? .empty
        let dependencies =
            try selected.map { try dependencies(taskID: $0.id, after: dependenciesAfter) } ?? .empty
        let references =
            try selected.map { try references(taskID: $0.id, after: referencesAfter) } ?? .empty
        let selectedStep: MemoryStep?
        if let task = selected, let id = selectedStepID {
            guard validID(id) else { throw MemoryFailure.invalidInput }
            if let step = steps.items.first(where: { $0.id == id }) {
                selectedStep = step
            } else {
                let page = try self.steps(taskID: task.id, after: id - 1)
                guard let step = page.items.first, step.id == id else {
                    throw MemoryFailure.deleted
                }
                guard page.taskRevision == task.revision else { throw MemoryFailure.stale }
                selectedStep = step
            }
        } else {
            selectedStep = nil
        }
        let checks =
            try selectedStep.map { try self.checks(taskID: $0.taskID, stepID: $0.id) } ?? .empty
        if let selected {
            let revisions = [
                evidence.taskRevision, links.taskRevision, decisions.taskRevision,
                steps.taskRevision, dependencies.taskRevision, references.taskRevision,
            ]
            guard revisions.allSatisfy({ $0 == selected.revision }),
                selectedStep == nil || checks.taskRevision == selected.revision,
                try task(selected.id).revision == selected.revision
            else { throw MemoryFailure.stale }
        }
        return MemorySnapshot(
            tasks: tasks, selectedTask: selected, evidence: evidence, links: links,
            decisions: decisions, steps: steps, selectedStep: selectedStep, checks: checks,
            dependencies: dependencies, references: references)
    }
    public func task(_ id: UInt64) throws -> MemoryTask {
        try openIfNeeded()
        var record = AmbientTask()
        var bytes = [UInt8](repeating: 0, count: 16_384)
        try bytes.withUnsafeMutableBufferPointer {
            try check(ambient_v3_task(handle, id, &record, $0.baseAddress, UInt32($0.count)))
        }
        return try decodeTask(record, bytes)
    }
    public func tasks(after: UInt64 = 0) throws -> MemoryPage<MemoryTask> {
        try openIfNeeded()
        return try page(
            empty: AmbientTask(),
            call: { p, r, b in
                ambient_v3_tasks(handle, after, 50, p, r, 50, b, 262_144)
            }, decode: decodeTask)
    }
    public func createTask(_ text: String) async throws -> MemoryTask {
        return try await withObservationMutation {
            try openIfNeeded()
            var id: UInt64 = 0
            try withText(text) { try check(ambient_v3_create_task(handle, $0, &id)) }
            return try task(id)

        }
    }
    public func editTask(_ task: MemoryTask, text: String) async throws -> MemoryTask {
        return try await withObservationMutation {
            try openIfNeeded()
            try withText(text) {
                try check(ambient_v3_edit_task(handle, task.id, task.revision, $0))
            }
            return try self.task(task.id)

        }
    }
    public func deleteTask(_ task: MemoryTask) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            let others = try deletionVersions(task: task, evidenceID: 0)
            try others.withUnsafeBufferPointer {
                try check(
                    ambient_v3_delete_task(
                        handle, task.id, task.revision, $0.baseAddress, UInt32($0.count)))
            }

        }
    }
    public func addNote(to task: MemoryTask, text: String, at: Int64) async throws -> UInt64 {
        return try await withObservationMutation {
            try openIfNeeded()
            var id: UInt64 = 0
            try withText(text) {
                try check(ambient_v3_add_note(handle, task.id, task.revision, $0, at, &id))
            }
            guard validID(id) else { throw MemoryFailure.native }
            return id

        }
    }
    public func editEvidence(_ id: UInt64, from task: MemoryTask, text: String, at: Int64)
        async throws
    {
        return try await withObservationMutation {
            try openIfNeeded()
            try withText(text) {
                try check(ambient_v3_edit_evidence(handle, id, task.id, task.revision, $0, at))
            }

        }
    }
    public func deleteEvidence(_ id: UInt64, from task: MemoryTask) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            let others = try deletionVersions(task: task, evidenceID: id)
            try others.withUnsafeBufferPointer {
                try check(
                    ambient_v3_delete_evidence(
                        handle, id, task.id, task.revision, $0.baseAddress, UInt32($0.count)))
            }

        }
    }
    public func moveEvidence(_ id: UInt64, from: MemoryTask, to: MemoryTask, at: Int64) async throws
    {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(
                ambient_v3_move_evidence(handle, id, from.id, from.revision, to.id, to.revision, at)
            )

        }
    }
    public func setLink(from: MemoryTask, to: MemoryTask, related: Bool, at: Int64) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(
                ambient_v3_set_link(
                    handle, from.id, from.revision, to.id, to.revision, related ? 1 : 0, at))

        }
    }
    func beginCheck(userReported: Bool, profile: ModelProfile = .low, at: Int64) throws -> UInt64 {
        try openIfNeeded()
        var operation: UInt64 = 0
        try ModelCheckBridge.check(
            ambient_model_v6_begin(
                handle, userReported ? 1 : 0, profile.rawValue, at, 0, &operation))
        guard operation != 0 else { throw ModelCheckFailure.native }
        return operation
    }
    func setObservationPolicy(_ policy: ObservationPolicy) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            guard (1...60).contains(policy.perHourLimit),
                policy.started == policy.creditAcknowledged
            else {
                throw ModelCheckFailure.invalid
            }
            let encoder = JSONEncoder()
            encoder.keyEncodingStrategy = .convertToSnakeCase
            let bytes = try encoder.encode(policy)
            try bytes.withUnsafeBytes {
                try ModelCheckBridge.check(
                    ambient_observe_v1_set_policy(
                        handle,
                        AmbientText(
                            bytes: $0.bindMemory(to: UInt8.self).baseAddress,
                            length: UInt32($0.count)))
                )
            }

        }
    }
    func submitObservationFrame(_ frame: ObservationFrame, idleSeconds: UInt32, at: ObservationTime)
        throws -> ObservationFrameResult
    {
        try openIfNeeded()
        return try ObservationBridge.submit(frame, memory: handle, idleSeconds: idleSeconds, at: at)
    }
    func observationTick(idleSeconds: UInt32, appSwitched: Bool, at: ObservationTime) throws
        -> ObservationCadence
    {
        try openIfNeeded()
        var next = AmbientObserveNext()
        try ModelCheckBridge.check(
            ambient_observe_v1_tick(
                handle, at.utc, at.monotonicMS, idleSeconds, appSwitched ? 1 : 0, &next))
        return try ObservationBridge.cadence(next)
    }
    func beginObservation(at: ObservationTime) throws -> ObservationBegin {
        try openIfNeeded()
        var out = AmbientObserveBegin()
        try ModelCheckBridge.check(ambient_observe_v1_begin(handle, at.utc, at.monotonicMS, &out))
        guard out.reserved == 0, (out.operation != 0) == (out.block == 0) else {
            throw ModelCheckFailure.native
        }
        switch out.block {
        case 0: return .ready(out.operation)
        case 1: return .notDue
        case 2: return .busy
        case 3: return .cloudUnavailable
        default: throw ModelCheckFailure.native
        }
    }
    func observationHighlight() throws -> ObservationHighlightState {
        try openIfNeeded()
        let state = try ObservationBridge.json(ObservationHighlightState.self, limit: 8192) {
            ambient_observe_v1_highlight(handle, $0, $1, $2)
        }
        if let value = state.highlight {
            guard value.proposalId > 0, value.proposalId <= UInt64(Int64.max),
                value.frameId > 0, value.frameId <= UInt64(Int64.max),
                !value.displayKey.isEmpty, value.displayKey.utf8.count <= 128,
                !value.anchor.isEmpty, value.anchor.utf8.count <= 1024, value.rect.valid
            else { throw ModelCheckFailure.native }
        }
        return state
    }
    func observationJournal(at utc: Int64) throws -> ObservationJournal {
        try openIfNeeded()
        let journal = try ObservationBridge.json(ObservationJournal.self, limit: 16_777_216) {
            ambient_observe_v1_journal(handle, utc, $0, $1, $2)
        }
        guard journal.sends.count <= 1440, journal.unsentFrames <= journal.logFrames,
            journal.logFrames <= 20_000, (1...60).contains(journal.policy.perHourLimit),
            journal.policy.started == journal.policy.creditAcknowledged
        else { throw ModelCheckFailure.native }
        return journal
    }
    func deleteObservationLog() async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try ModelCheckBridge.check(ambient_observe_v1_delete_log(handle))

        }
    }
    func checkSummary() throws -> ModelCheckSummary {
        try openIfNeeded()
        var result = AmbientModelSummary()
        try ModelCheckBridge.check(ambient_model_v6_summary(handle, &result))
        return try ModelCheckBridge.decode(result)
    }
    func setModelSession(epoch: UInt64, valid: Bool, reported: Bool) async throws {
        try openIfNeeded()
        try ModelCheckBridge.check(
            ambient_model_v6_session(handle, epoch, valid ? 1 : 0, reported ? 1 : 0))
        guard observationSessionEpoch != epoch || observationSessionValid != valid else { return }
        observationSessionEpoch = epoch
        observationSessionValid = valid
        await observationDidMutate()
    }
    func assistanceAvailable() throws -> Bool {
        try openIfNeeded()
        var available: UInt32 = 0
        try ModelCheckBridge.check(ambient_model_v6_available(handle, &available))
        guard available <= 1 else { throw ModelCheckFailure.native }
        return available == 1
    }
    func clearAssistance() throws {
        try openIfNeeded()
        try ModelCheckBridge.check(ambient_model_v6_clear(handle))
    }
    func discardAssistancePreview(_ request: UInt64) throws {
        try openIfNeeded()
        try ModelCheckBridge.check(ambient_model_v6_discard_preview(handle, request))
    }
    func workspaceRevision(_ taskID: UInt64) throws -> UInt64 {
        try openIfNeeded()
        var revision: UInt64 = 0
        try ModelCheckBridge.check(ambient_model_v6_workspace_revision(handle, taskID, &revision))
        guard revision > 0, revision <= UInt64(Int64.max) else { throw ModelCheckFailure.native }
        return revision
    }
    func prepareAssistance(_ task: MemoryTask, notes: [UInt64]) throws -> AssistancePreview {
        try prepare(
            task: AssistanceTaskVersion(taskID: task.id, revision: task.revision),
            workspaceRevision: workspaceRevision(task.id), mode: .newContext, notes: notes,
            reply: nil)
    }
    func prepareContinuation(_ proposal: Proposal, text: String? = nil) throws -> AssistancePreview
    {
        try prepare(
            task: proposal.task, workspaceRevision: proposal.workspaceRevision, mode: .continue,
            notes: [], reply: text.map { (proposal.target, $0) })
    }
    private func prepare(
        task: AssistanceTaskVersion, workspaceRevision: UInt64, mode: AssistanceMode,
        notes: [UInt64], reply: (ProposalTarget, String)?
    ) throws -> AssistancePreview {
        try openIfNeeded()
        guard notes.count <= 32 else { throw ModelCheckFailure.limit }
        let text = Array((reply?.1 ?? "").utf8)
        guard text.count <= 16384 else { throw ModelCheckFailure.limit }
        let request = AmbientAssistanceRequest(
            task_id: task.taskID, revision: task.revision, workspace_revision: workspaceRevision,
            proposal_id: reply?.0.id ?? 0, content_revision: reply?.0.contentRevision ?? 0,
            mode: mode.rawValue, reserved: 0)
        var out = AmbientAssistancePreview()
        var bytes = [UInt8](repeating: 0, count: 262144)
        try notes.withUnsafeBufferPointer { notes in
            try text.withUnsafeBufferPointer { text in
                try bytes.withUnsafeMutableBufferPointer { buffer in
                    try ModelCheckBridge.check(
                        ambient_model_v6_prepare(
                            handle, request, notes.baseAddress, UInt32(notes.count),
                            text.baseAddress, UInt32(text.count), &out, buffer.baseAddress,
                            UInt32(buffer.count)))
                }
            }
        }
        guard out.request_id > 0, out.task_id == task.taskID, out.revision == task.revision,
            out.workspace_revision == workspaceRevision, out.mode == mode.rawValue,
            out.reserved == 0, out.context.offset == 0, out.context.length <= 262144
        else { throw ModelCheckFailure.native }
        return AssistancePreview(
            requestID: out.request_id, taskID: out.task_id, revision: out.revision,
            workspaceRevision: out.workspace_revision, mode: mode,
            context: try ModelCheckBridge.text(out.context, bytes, used: Int(out.context.length)))
    }
    func beginAssistance(_ request: UInt64, at: Int64) throws -> UInt64 {
        try openIfNeeded()
        guard request > 0 else { throw ModelCheckFailure.invalid }
        var operation: UInt64 = 0
        try ModelCheckBridge.check(
            ambient_model_v6_begin(handle, 1, ModelProfile.high.rawValue, at, request, &operation))
        guard operation > 0 else { throw ModelCheckFailure.native }
        return operation
    }
    func proposalFeed() throws -> [Proposal] {
        try openIfNeeded()
        var length: UInt32 = 0
        var bytes = [UInt8](repeating: 0, count: 4_194_304)
        try bytes.withUnsafeMutableBufferPointer { buffer in
            try ModelCheckBridge.check(
                ambient_model_v6_feed(handle, buffer.baseAddress, UInt32(buffer.count), &length))
        }
        let feed = try ModelCheckBridge.decodeProjection(
            ProposalFeed.self, bytes: bytes, length: length, limit: 4_194_304)
        guard feed.proposals.count <= 128,
            Set(feed.proposals.map(\.id)).count == feed.proposals.count,
            feed.proposals.map(\.id) == feed.proposals.map(\.id).sorted()
        else { throw ModelCheckFailure.native }
        for row in feed.proposals { try ModelCheckBridge.validate(row) }
        return feed.proposals
    }
    func proposalDetails(_ target: ProposalTarget) throws -> ProposalDetails {
        try openIfNeeded()
        var length: UInt32 = 0
        var bytes = [UInt8](repeating: 0, count: 262144)
        try bytes.withUnsafeMutableBufferPointer { buffer in
            try ModelCheckBridge.check(
                ambient_model_v6_details(
                    handle, target.native, buffer.baseAddress, UInt32(buffer.count), &length))
        }
        let value = try ModelCheckBridge.decodeProjection(
            ProposalDetails.self, bytes: bytes, length: length, limit: 262144)
        try ModelCheckBridge.validate(value.proposal)
        guard value.proposal.target == target, value.citations.count <= 32,
            !value.proposal.stale || value.citations.isEmpty
        else { throw ModelCheckFailure.native }
        return value
    }
    func validateProposal(_ target: ProposalTarget) throws -> AssistanceKind {
        try openIfNeeded()
        var kind: UInt32 = 0
        try ModelCheckBridge.check(ambient_model_v6_validate(handle, target.native, &kind))
        switch kind {
        case 1: return .suggestion
        case 2: return .clarification
        default: throw ModelCheckFailure.native
        }
    }
    func setProposalVisibility(_ target: ProposalTarget, visible: Bool) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try ModelCheckBridge.check(
                ambient_model_v6_visibility(handle, target.native, visible ? 1 : 0))

        }
    }
    func clearDiscussion(_ task: MemoryTask, workspaceRevision: UInt64) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try ModelCheckBridge.check(
                ambient_model_v6_clear_discussion(handle, task.id, task.revision, workspaceRevision)
            )

        }
    }
    func copyProposal(_ target: ProposalTarget) throws -> String {
        try openIfNeeded()
        var length: UInt32 = 0
        var bytes = [UInt8](repeating: 0, count: 16384)
        try bytes.withUnsafeMutableBufferPointer { buffer in
            try ModelCheckBridge.check(
                ambient_model_v6_copy(
                    handle, target.native, buffer.baseAddress, UInt32(buffer.count), &length))
        }
        return try ModelCheckBridge.text(
            AmbientModelSpan(offset: 0, length: length), bytes, used: Int(length))
    }
    public func manualPause() throws -> Bool {
        try openIfNeeded()
        var paused: UInt32 = 0
        try check(ambient_v3_manual_pause(handle, &paused))
        // The C boundary supplies an integer, not a Swift Bool.
        guard paused <= 1 else { throw MemoryFailure.native }
        return paused == 1
    }
    public func setManualPause(_ paused: Bool) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(ambient_v3_set_manual_pause(handle, paused ? 1 : 0))

        }
    }
    public func evidence(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<MemoryEvidence> {
        try openIfNeeded()
        return try page(
            empty: AmbientEvidence(),
            call: { p, r, b in
                ambient_v3_evidence(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { record, bytes in
                guard validID(record.id), record.task_id == taskID, record.has_observed_at <= 1,
                    let completeness = MemoryCompleteness(rawValue: record.completeness),
                    let kind = MemoryAssertionKind(rawValue: record.kind)
                else { throw MemoryFailure.native }
                let text = try string(record.text, bytes)
                guard !text.isEmpty else { throw MemoryFailure.native }
                let origin: MemoryOrigin
                switch record.origin {
                case 0:
                    guard record.has_observed_at == 1, record.namespace.length == 0,
                        record.scope.length == 0, record.object_id.length == 0,
                        record.has_version == 0, record.version.length == 0
                    else { throw MemoryFailure.native }
                    origin = .user
                case 1:
                    origin = .external(
                        key: try sourceKey(record.namespace, record.scope, record.object_id, bytes),
                        version: try version(record.has_version, record.version, bytes)
                    )
                default: throw MemoryFailure.native
                }
                return MemoryEvidence(
                    id: record.id, taskID: taskID, text: text, origin: origin,
                    observedAtUTC: record.has_observed_at == 1 ? record.observed_at_utc : nil,
                    completeness: completeness, kind: kind
                )
            })
    }
    public func sources(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<MemorySourceBinding>
    {
        try openIfNeeded()
        return try page(
            empty: AmbientSource(),
            call: { p, r, b in
                ambient_v3_sources(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { record, bytes in
                guard validID(record.id), validID(record.evidence_id), record.task_id == taskID
                else {
                    throw MemoryFailure.native
                }
                return MemorySourceBinding(
                    id: record.id, taskID: taskID, evidenceID: record.evidence_id,
                    key: try sourceKey(record.namespace, record.scope, record.object_id, bytes),
                    version: try version(record.has_version, record.version, bytes)
                )
            })
    }
    public func links(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<MemoryLink> {
        try openIfNeeded()
        return try page(
            empty: AmbientLink(),
            call: { p, r, b in
                ambient_v3_links(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { record, bytes in
                guard validID(record.id), validID(record.other_task_id),
                    validID(record.other_task_revision), record.related <= 1
                else { throw MemoryFailure.native }
                return MemoryLink(
                    id: record.id,
                    otherTask: MemoryTask(
                        id: record.other_task_id, revision: record.other_task_revision,
                        text: try string(record.other_task_text, bytes),
                        lifecycle: try lifecycle(record.other_task_lifecycle),
                        initiativeSuppressed: try boolean(record.other_task_initiative_suppressed),
                        planCompleteness: try completeness(record.other_task_plan_completeness)),
                    related: record.related == 1,
                    actionAtUTC: record.action_at_utc)
            })
    }
    public func decisions(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<
        MemoryBindingDecision
    > {
        try openIfNeeded()
        return try page(
            empty: AmbientDecision(),
            call: { p, r, b in
                ambient_v3_decisions(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { record, _ in
                guard validID(record.id), validID(record.evidence_id), record.accepted <= 1 else {
                    throw MemoryFailure.native
                }
                return MemoryBindingDecision(
                    id: record.id, evidenceID: record.evidence_id, accepted: record.accepted == 1,
                    actionAtUTC: record.action_at_utc)
            })
    }
    public func ingestSource(
        to task: MemoryTask, key: MemorySourceKey, version: String?, evidence: MemorySourceEvidence
    ) async throws -> MemoryIngestOutcome {
        return try await withObservationMutation {
            try openIfNeeded()
            var result = AmbientIngest()
            try withSource(key, version, evidence) { k, e in
                try check(ambient_v3_ingest_source(handle, task.id, task.revision, k, e, &result))
            }
            switch result.outcome {
            case 0 where validID(result.evidence_id): return .applied(result.evidence_id)
            case 1 where result.evidence_id == 0: return .forgotten
            case 2 where result.evidence_id == 0: return .userProtected
            default: throw MemoryFailure.native
            }

        }
    }
    public func bindSource(
        to task: MemoryTask, key: MemorySourceKey, version: String?, evidence: MemorySourceEvidence,
        at: Int64
    ) async throws -> UInt64 {
        return try await withObservationMutation {
            try openIfNeeded()
            var id: UInt64 = 0
            try withSource(key, version, evidence) { k, e in
                try check(ambient_v3_bind_source(handle, task.id, task.revision, k, e, at, &id))
            }
            guard validID(id) else { throw MemoryFailure.native }
            return id

        }
    }
    public func close() throws {
        guard !closed else { return }
        closed = true
        guard handle != 0 else { return }
        let consumed = handle
        handle = 0
        try check(ambient_v3_close(consumed))
    }
    private func deletionVersions(task: MemoryTask, evidenceID: UInt64) throws
        -> [AmbientTaskVersion]
    {
        var versions: [AmbientTaskVersion] = []
        var after: UInt64 = 0
        repeat {
            let result = try page(
                empty: AmbientTask(),
                call: { p, r, b in
                    ambient_v3_deletion_impact(
                        handle, task.id, evidenceID, after, 50, p, r, 50, b, 262_144)
                }, decode: decodeTask)
            versions += result.items.map {
                AmbientTaskVersion(task_id: $0.id, revision: $0.revision)
            }
            guard versions.count < 1024 else { throw MemoryFailure.native }
            guard let next = result.nextAfter else { break }
            guard next > after else { throw MemoryFailure.native }
            after = next
        } while true
        return versions
    }
    private func page<Record, Value>(
        empty: Record,
        call: (
            UnsafeMutablePointer<AmbientPage>, UnsafeMutablePointer<Record>?,
            UnsafeMutablePointer<UInt8>?
        ) -> UInt32,
        decode: (Record, [UInt8]) throws -> Value
    ) throws -> MemoryPage<Value> where Value: Equatable & Sendable {
        var metadata = AmbientPage()
        var records = [Record](repeating: empty, count: 50)
        var bytes = [UInt8](repeating: 0, count: 262_144)
        let status = records.withUnsafeMutableBufferPointer { r in
            bytes.withUnsafeMutableBufferPointer { b in
                call(&metadata, r.baseAddress, b.baseAddress)
            }
        }
        try check(status)
        guard metadata.count <= 50, metadata.bytes_used <= 262_144, metadata.has_more <= 1,
            MemoryLayout<AmbientPage>.stride + Int(metadata.count) * MemoryLayout<Record>.stride
                + Int(metadata.bytes_used) <= 262_144,
            metadata.task_revision == 0 || validID(metadata.task_revision),
            MemoryCompleteness(rawValue: metadata.plan_completeness) != nil,
            metadata.has_more == 1
                ? validID(metadata.next_after) && metadata.count > 0 : metadata.next_after == 0
        else { throw MemoryFailure.native }
        bytes.removeSubrange(Int(metadata.bytes_used)..<bytes.count)
        return MemoryPage(
            items: try records.prefix(Int(metadata.count)).map { try decode($0, bytes) },
            nextAfter: metadata.has_more == 1 ? metadata.next_after : nil,
            taskRevision: metadata.task_revision == 0 ? nil : metadata.task_revision,
            planCompleteness: metadata.task_revision == 0
                ? nil : MemoryCompleteness(rawValue: metadata.plan_completeness)
        )
    }
    private func decodeTask(_ record: AmbientTask, _ bytes: [UInt8]) throws -> MemoryTask {
        guard validID(record.id), validID(record.revision) else { throw MemoryFailure.native }
        let text = try string(record.text, bytes)
        guard !text.isEmpty, text.utf8.count <= 16_384 else { throw MemoryFailure.native }
        return MemoryTask(
            id: record.id, revision: record.revision, text: text,
            lifecycle: try lifecycle(record.lifecycle),
            initiativeSuppressed: try boolean(record.initiative_suppressed),
            planCompleteness: try completeness(record.plan_completeness))
    }
    private func string(_ span: AmbientSpan, _ bytes: [UInt8]) throws -> String {
        let start = Int(span.offset)
        let end = start + Int(span.length)
        guard end <= bytes.count, let value = String(bytes: bytes[start..<end], encoding: .utf8)
        else { throw MemoryFailure.native }
        return value
    }
    private func sourceKey(
        _ namespace: AmbientSpan, _ scope: AmbientSpan, _ object: AmbientSpan, _ bytes: [UInt8]
    ) throws -> MemorySourceKey {
        let key = try MemorySourceKey(
            namespace: string(namespace, bytes), scope: string(scope, bytes),
            objectID: string(object, bytes))
        for (value, cap) in [(key.namespace, 64), (key.scope, 256), (key.objectID, 2048)] {
            guard !value.isEmpty, value.utf8.count <= cap, !value.utf8.contains(0) else {
                throw MemoryFailure.native
            }
        }
        return key
    }
    private func version(_ present: UInt32, _ span: AmbientSpan, _ bytes: [UInt8]) throws -> String?
    {
        guard present <= 1 else { throw MemoryFailure.native }
        if present == 0 {
            guard span.length == 0 else { throw MemoryFailure.native }
            return nil
        }
        let value = try string(span, bytes)
        guard !value.isEmpty, value.utf8.count <= 256, !value.utf8.contains(0) else {
            throw MemoryFailure.native
        }
        return value
    }
    private func validID(_ value: UInt64) -> Bool { value > 0 && value <= Int64.max }
    private func withText<T>(
        _ text: String, limit: Int = 16_384, _ operation: (AmbientText) throws -> T
    ) throws -> T {
        let bytes = Array(text.utf8)
        guard !bytes.isEmpty, bytes.count <= limit else { throw MemoryFailure.invalidInput }
        return try bytes.withUnsafeBufferPointer {
            try operation(AmbientText(bytes: $0.baseAddress, length: UInt32($0.count)))
        }
    }
    private func withSource<T>(
        _ key: MemorySourceKey, _ version: String?, _ evidence: MemorySourceEvidence,
        _ operation: (UnsafePointer<AmbientSourceKey>, UnsafePointer<AmbientSourceEvidence>) throws
            -> T
    ) throws -> T {
        try withText(key.namespace, limit: 64) { namespace in
            try withText(key.scope, limit: 256) { scope in
                try withText(key.objectID, limit: 2048) { object in
                    try withText(evidence.text) { text in
                        func invoke(_ versionText: AmbientText) throws -> T {
                            var k = AmbientSourceKey(
                                namespace: namespace, scope: scope, object_id: object,
                                has_version: version == nil ? 0 : 1, version: versionText)
                            var e = AmbientSourceEvidence(
                                text: text, observed_at_utc: evidence.observedAtUTC ?? 0,
                                has_observed_at: evidence.observedAtUTC == nil ? 0 : 1,
                                completeness: evidence.completeness.rawValue,
                                kind: evidence.kind.rawValue)
                            return try operation(&k, &e)
                        }
                        if let version { return try withText(version, limit: 256, invoke) }
                        return try invoke(AmbientText())
                    }
                }
            }
        }
    }
    func sourceMemoryHandle() throws -> UInt64 {
        try openIfNeeded()
        return handle
    }
    func sourceDatabaseURL() throws -> URL {
        do { return try databaseURL ?? Self.applicationDatabaseURL() } catch {
            throw MemoryFailure.opening
        }
    }
    private func openIfNeeded() throws {
        guard !closed else { throw MemoryFailure.closed }
        guard handle == 0 else { return }
        let url = try sourceDatabaseURL()
        let bytes = Array(url.path.utf8)
        guard !bytes.isEmpty, bytes.count <= 16_384 else { throw MemoryFailure.opening }
        try bytes.withUnsafeBufferPointer {
            try check(ambient_v3_open($0.baseAddress, UInt32($0.count), &handle), opening: true)
        }
    }
    private static func applicationDatabaseURL() throws -> URL {
        let manager = FileManager.default
        let support = try manager.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil,
            create: true)
        let directory = support.appendingPathComponent("dev.ambient.foundation", isDirectory: true)
        try manager.createDirectory(
            at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700]
        )
        let attributes = try manager.attributesOfItem(atPath: directory.path)
        guard attributes[.type] as? FileAttributeType == .typeDirectory else {
            throw MemoryFailure.opening
        }
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
        return directory.appendingPathComponent("memory.sqlite3")
    }
    private func check(_ status: UInt32, opening: Bool = false) throws {
        switch status {
        case AMBIENT_V3_OK: return
        case AMBIENT_V3_INVALID_ARGUMENT, AMBIENT_V3_INVALID_UTF8, AMBIENT_V3_TOO_LONG:
            throw opening ? MemoryFailure.opening : MemoryFailure.invalidInput
        case AMBIENT_V3_STORAGE_ERROR: throw opening ? MemoryFailure.opening : MemoryFailure.storage
        case AMBIENT_V3_UNSUPPORTED_SCHEMA: throw MemoryFailure.schema
        case AMBIENT_V3_STALE_REVISION: throw MemoryFailure.stale
        case AMBIENT_V3_DELETED, AMBIENT_V3_NOT_FOUND: throw MemoryFailure.deleted
        case AMBIENT_V3_TASK_COMPLETED: throw MemoryFailure.completed
        case AMBIENT_V3_RESOURCE_LIMIT: throw MemoryFailure.limit
        default: throw MemoryFailure.native
        }
    }
    private func boolean(_ value: UInt32) throws -> Bool {
        guard value <= 1 else { throw MemoryFailure.native }
        return value == 1
    }
    private func lifecycle(_ value: UInt32) throws -> MemoryTaskLifecycle {
        guard let result = MemoryTaskLifecycle(rawValue: value) else { throw MemoryFailure.native }
        return result
    }
    private func completeness(_ value: UInt32) throws -> MemoryCompleteness {
        guard let result = MemoryCompleteness(rawValue: value) else { throw MemoryFailure.native }
        return result
    }
    private func optionalText(
        _ flag: UInt32, _ span: AmbientSpan, _ bytes: [UInt8], limit: Int, noNUL: Bool = false
    ) throws -> String? {
        if try !boolean(flag) {
            guard span.length == 0 else { throw MemoryFailure.native }
            return nil
        }
        let value = try string(span, bytes)
        guard !value.isEmpty, value.utf8.count <= limit, !noNUL || !value.utf8.contains(0) else {
            throw MemoryFailure.native
        }
        return value
    }
    public func steps(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<MemoryStep> {
        try openIfNeeded()
        return try page(
            empty: AmbientStep(),
            call: { p, r, b in
                ambient_v3_steps(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { r, b in
                guard validID(r.id), r.task_id == taskID else { throw MemoryFailure.native }
                let description = try string(r.description, b)
                guard !description.isEmpty, description.utf8.count <= 16_384 else {
                    throw MemoryFailure.native
                }
                return MemoryStep(
                    id: r.id, taskID: taskID, description: description,
                    prompt: try optionalText(r.has_prompt, r.prompt, b, limit: 16_384),
                    reference: try optionalText(
                        r.has_reference, r.reference, b, limit: 2048, noNUL: true),
                    completedAtUTC: try boolean(r.has_completed) ? r.completed_at_utc : nil,
                    ready: try boolean(r.ready))
            })
    }
    public func checks(taskID: UInt64, stepID: UInt64, after: UInt64 = 0) throws -> MemoryPage<
        MemoryCheck
    > {
        try openIfNeeded()
        return try page(
            empty: AmbientCheck(),
            call: { p, r, b in
                ambient_v3_checks(handle, taskID, stepID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { r, b in
                guard validID(r.id), r.task_id == taskID, r.step_id == stepID,
                    let result = MemoryCheckResult(rawValue: r.result), r.has_confirmation <= 1,
                    (result == .unknown) == (r.has_confirmation == 0)
                else { throw MemoryFailure.native }
                let description = try string(r.description, b)
                guard !description.isEmpty, description.utf8.count <= 16_384 else {
                    throw MemoryFailure.native
                }
                return MemoryCheck(
                    id: r.id, taskID: taskID, stepID: stepID, description: description,
                    result: result,
                    confirmedAtUTC: r.has_confirmation == 1 ? r.confirmed_at_utc : nil)
            })
    }
    public func dependencies(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<
        MemoryDependency
    > {
        try openIfNeeded()
        return try page(
            empty: AmbientDependency(),
            call: { p, r, b in
                ambient_v3_dependencies(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { r, _ in
                guard validID(r.id), r.task_id == taskID, validID(r.predecessor),
                    validID(r.dependent), r.predecessor != r.dependent
                else { throw MemoryFailure.native }
                return MemoryDependency(
                    id: r.id, taskID: taskID, predecessor: r.predecessor, dependent: r.dependent)
            })
    }
    public func references(taskID: UInt64, after: UInt64 = 0) throws -> MemoryPage<MemoryReference>
    {
        try openIfNeeded()
        return try page(
            empty: AmbientReference(),
            call: { p, r, b in
                ambient_v3_references(handle, taskID, after, 50, p, r, 50, b, 262_144)
            },
            decode: { r, b in
                guard validID(r.id), r.task_id == taskID else { throw MemoryFailure.native }
                guard let text = try optionalText(1, r.text, b, limit: 2048, noNUL: true) else {
                    throw MemoryFailure.native
                }
                return MemoryReference(id: r.id, taskID: taskID, text: text)
            })
    }
    public func addStep(to task: MemoryTask, input: MemoryStepInput) async throws -> UInt64 {
        return try await withObservationMutation {
            try openIfNeeded()
            var id: UInt64 = 0
            try withStep(input) {
                try check(ambient_v3_add_step(handle, task.id, task.revision, $0, &id))
            }
            guard validID(id) else { throw MemoryFailure.native }
            return id

        }
    }
    public func editStep(_ task: MemoryTask, id: UInt64, input: MemoryStepInput) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try withStep(input) {
                try check(ambient_v3_edit_step(handle, task.id, task.revision, id, $0))
            }

        }
    }
    public func deleteStep(_ task: MemoryTask, id: UInt64) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(ambient_v3_delete_step(handle, task.id, task.revision, id))

        }
    }
    public func setStepCompleted(_ task: MemoryTask, id: UInt64, at: Int64?) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(
                ambient_v3_set_step_completed(
                    handle, task.id, task.revision, id, at == nil ? 0 : 1, at ?? 0))

        }
    }
    public func addCheck(to task: MemoryTask, stepID: UInt64, description: String) async throws
        -> UInt64
    {
        return try await withObservationMutation {
            try openIfNeeded()
            var id: UInt64 = 0
            try withText(description) {
                try check(ambient_v3_add_check(handle, task.id, task.revision, stepID, $0, &id))
            }
            guard validID(id) else { throw MemoryFailure.native }
            return id

        }
    }
    public func editCheck(_ task: MemoryTask, id: UInt64, description: String) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try withText(description) {
                try check(ambient_v3_edit_check(handle, task.id, task.revision, id, $0))
            }

        }
    }
    public func deleteCheck(_ task: MemoryTask, id: UInt64) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(ambient_v3_delete_check(handle, task.id, task.revision, id))

        }
    }
    public func setCheckResult(
        _ task: MemoryTask, id: UInt64, result: MemoryCheckResult, at: Int64?
    ) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(
                ambient_v3_set_check_result(
                    handle, task.id, task.revision, id, result.rawValue, at == nil ? 0 : 1, at ?? 0)
            )

        }
    }
    public func addDependency(_ task: MemoryTask, predecessor: UInt64, dependent: UInt64)
        async throws
    {
        return try await withObservationMutation {
            try openIfNeeded()
            var id: UInt64 = 0
            try check(
                ambient_v3_add_dependency(
                    handle, task.id, task.revision, predecessor, dependent, &id))

        }
    }
    public func deleteDependency(_ task: MemoryTask, id: UInt64) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(ambient_v3_delete_dependency(handle, task.id, task.revision, id))

        }
    }
    public func setPlanCompleteness(_ task: MemoryTask, value: MemoryCompleteness) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(
                ambient_v3_set_plan_completeness(handle, task.id, task.revision, value.rawValue))

        }
    }
    public func setInitiativeSuppressed(_ task: MemoryTask, value: Bool) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(
                ambient_v3_set_initiative_suppressed(handle, task.id, task.revision, value ? 1 : 0))

        }
    }
    public func reopenTask(_ task: MemoryTask) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            try check(ambient_v3_reopen_task(handle, task.id, task.revision))

        }
    }
    public func completeTask(
        _ task: MemoryTask, summary: String, keep: [UInt64], references: [String]
    ) async throws {
        return try await withObservationMutation {
            try openIfNeeded()
            guard keep.count <= 1023, references.count <= 32 else {
                throw MemoryFailure.invalidInput
            }
            var versions: [AmbientTaskVersion] = []
            var after: UInt64 = 0
            repeat {
                let result = try keep.withUnsafeBufferPointer { k in
                    try page(
                        empty: AmbientTask(),
                        call: { p, r, b in
                            ambient_v3_completion_impact(
                                handle, task.id, k.baseAddress, UInt32(k.count), after, 50, p, r,
                                50, b,
                                262_144)
                        }, decode: decodeTask)
                }
                guard result.taskRevision == task.revision else { throw MemoryFailure.stale }
                versions += result.items.map {
                    AmbientTaskVersion(task_id: $0.id, revision: $0.revision)
                }
                guard versions.count <= 1023 else { throw MemoryFailure.native }
                guard let next = result.nextAfter else { break }
                guard next > after else { throw MemoryFailure.native }
                after = next
            } while true
            try withText(summary) { summary in
                try withTexts(references, limit: 2048) { refs in
                    try keep.withUnsafeBufferPointer { k in
                        try refs.withUnsafeBufferPointer { r in
                            try versions.withUnsafeBufferPointer { v in
                                try check(
                                    ambient_v3_complete_task(
                                        handle, task.id, task.revision, summary, k.baseAddress,
                                        UInt32(k.count), r.baseAddress, UInt32(r.count),
                                        v.baseAddress,
                                        UInt32(v.count)))
                            }
                        }
                    }
                }
            }

        }
    }
    private func withTexts<T>(
        _ values: [String], limit: Int, _ operation: ([AmbientText]) throws -> T
    ) throws -> T {
        // Nested buffer lifetimes keep every caller-owned string alive through the one synchronous C call.
        func next(_ index: Int, _ texts: [AmbientText]) throws -> T {
            if index == values.count { return try operation(texts) }
            return try withText(values[index], limit: limit) { try next(index + 1, texts + [$0]) }
        }
        return try next(0, [])
    }
    private func withStep<T>(
        _ input: MemoryStepInput, _ operation: (UnsafePointer<AmbientStepInput>) throws -> T
    ) throws -> T {
        try withText(input.description) { description in
            try withOptionalText(input.prompt, limit: 16_384) { prompt in
                try withOptionalText(input.reference, limit: 2048) { reference in
                    var value = AmbientStepInput(
                        description: description, has_prompt: input.prompt == nil ? 0 : 1,
                        prompt: prompt, has_reference: input.reference == nil ? 0 : 1,
                        reference: reference)
                    return try operation(&value)
                }
            }
        }
    }
    private func withOptionalText<T>(
        _ value: String?, limit: Int, _ operation: (AmbientText) throws -> T
    ) throws -> T {
        if let value { return try withText(value, limit: limit, operation) }
        return try operation(AmbientText())
    }
}
