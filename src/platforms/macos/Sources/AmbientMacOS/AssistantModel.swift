import AppKit
import Foundation
import Observation

public enum MemorySelection: Equatable, Sendable {
    case task(UInt64)
    case newTask
    case newNote
    case note(UInt64)
    case newStep
    case step(UInt64)
    case newCheck(UInt64)
    case check(UInt64)
    case newDependency, completion
}
public enum MemoryPlanPage: Sendable { case steps, dependencies, references }
@MainActor @Observable public final class AssistantModel {
    public private(set) var snapshot: MemorySnapshot?
    public private(set) var manualPause: Bool?
    public var draft = ""
    public var draftHasPrompt = false
    public var draftPrompt = ""
    public var draftHasReference = false
    public var draftReference = ""
    public var draftKeepLinks: Set<UInt64> = []
    public var draftReferences: [String] = []
    public private(set) var copied = false
    public private(set) var failure: MemoryFailure?
    public private(set) var busy = false
    public private(set) var closing = false
    public private(set) var pendingSelection: MemorySelection?
    private enum Editor {
        case newTask
        case task(MemoryTask)
        case newNote(MemoryTask)
        case note(MemoryEvidence, MemoryTask)
        case newStep(MemoryTask)
        case step(MemoryStep, MemoryTask)
        case newCheck(UInt64, MemoryTask)
        case check(MemoryCheck, MemoryTask)
        case newDependency(MemoryTask)
        case completion(MemoryTask)
        var savedText: String {
            switch self {
            case .newTask, .newNote, .newStep, .newCheck, .newDependency, .completion: return ""
            case .task(let task): return task.text
            case .note(let note, _): return note.text
            case .step(let step, _): return step.description
            case .check(let check, _): return check.description
            }
        }
    }
    private var editor: Editor = .newTask
    let localProjects: LocalProjectsModel
    let codexSources: CodexSourcesModel
    let githubSources: GitHubSourcesModel
    let jiraSources: JiraSourcesModel
    let todoistSources: TodoistSourcesModel
    let integrations: IntegrationSwitchesModel
    private let session: MemorySession
    private enum OperationResult: Sendable {
        case snapshot(MemorySnapshot, replaceEditor: Bool)
        case pause(Bool)
    }
    private var inFlight: (readingPause: Bool, task: Task<Bool, Never>)?
    private(set) var tasksAfter: UInt64 = 0
    private(set) var evidenceAfter: UInt64 = 0
    private(set) var linksAfter: UInt64 = 0
    private var decisionsAfter: UInt64 = 0
    private var stepsAfter: UInt64 = 0
    private var dependenciesAfter: UInt64 = 0
    private var referencesAfter: UInt64 = 0
    private var planPosition:
        (step: UInt64?, steps: UInt64, dependencies: UInt64, references: UInt64)
    {
        (snapshot?.selectedStep?.id, stepsAfter, dependenciesAfter, referencesAfter)
    }
    public init(session: MemorySession = MemorySession()) {
        self.session = session
        self.localProjects = LocalProjectsModel(memory: session)
        self.codexSources = CodexSourcesModel(memory: session)
        self.githubSources = GitHubSourcesModel(memory: session)
        self.jiraSources = JiraSourcesModel(memory: session)
        self.todoistSources = TodoistSourcesModel(memory: session)
        self.integrations = IntegrationSwitchesModel(memory: session)
    }
    public var hasUnsavedChanges: Bool {
        // String equality accepts canonical equivalents; storage and this draft preserve exact UTF-8.
        if !draft.utf8.elementsEqual(editor.savedText.utf8) { return true }
        if isEditingStep {
            let original: MemoryStep?
            if case .step(let step, _) = editor { original = step } else { original = nil }
            return draftHasPrompt != (original?.prompt != nil)
                || draftHasReference != (original?.reference != nil)
                || (draftHasPrompt
                    && !draftPrompt.utf8.elementsEqual((original?.prompt ?? "").utf8))
                || (draftHasReference
                    && !draftReference.utf8.elementsEqual((original?.reference ?? "").utf8))
        }
        if isEditingDependency { return !draftReference.isEmpty }
        if isCompletingTask { return !draftKeepLinks.isEmpty || !draftReferences.isEmpty }
        return false
    }
    var isEditingNote: Bool {
        switch editor {
        case .newNote, .note: true
        default: false
        }
    }
    var isCreatingTask: Bool { if case .newTask = editor { true } else { false } }
    private func cancelSourceViews() {
        localProjects.cancel()
        codexSources.cancel()
        githubSources.cancel()
        jiraSources.cancel()
        todoistSources.cancel()
    }
    /// Disabling makes Core cancel this kind's reads and clear dependent assistance, so native
    /// views follow; enabling clears nothing and keeps the current preview.
    func setIntegrationEnabled(_ kind: BuiltInIntegration, enabled: Bool, chatGPT: ChatGPTModel)
        async
    {
        if !enabled { cancelSourceViews() }
        await integrations.set(kind, enabled: enabled)
        if !enabled { await chatGPT.resetAssistance() }
    }
    func saveLocalSourceSettings(chatGPT: ChatGPTModel) async {
        cancelSourceViews()
        await localProjects.saveScope(chatGPT: chatGPT)
    }
    func deleteLocalSourceSettings(_ root: LocalRootSettings, chatGPT: ChatGPTModel) async {
        cancelSourceViews()
        await localProjects.deleteScope(root, chatGPT: chatGPT)
    }
    func saveCodexSourceSettings(chatGPT: ChatGPTModel) async {
        cancelSourceViews()
        await codexSources.saveScope(chatGPT: chatGPT)
    }
    func deleteCodexSourceSettings(_ selection: CodexSelection, chatGPT: ChatGPTModel) async {
        cancelSourceViews()
        await codexSources.deleteScope(selection, chatGPT: chatGPT)
    }
    public func reload() async {
        await loadManualPause()
        // The editor can change while the shared initial pause read is pending.
        let replace = snapshot == nil ? draft.isEmpty : !hasUnsavedChanges
        let id = snapshot?.selectedTask?.id
        let tc = tasksAfter
        let ec = evidenceAfter
        let lc = linksAfter
        let dc = decisionsAfter
        let sc = stepsAfter
        let dep = dependenciesAfter
        let rc = referencesAfter
        let sid = snapshot?.selectedStep?.id
        let pauseFailure = manualPause == nil ? failure : nil
        await perform(replaceEditor: replace) { [session] in
            try await session.load(
                selectedTaskID: id, tasksAfter: tc, evidenceAfter: ec, linksAfter: lc,
                decisionsAfter: dc, selectedStepID: sid, stepsAfter: sc, dependenciesAfter: dep,
                referencesAfter: rc)
        }
        if !closing, failure == nil { failure = pauseFailure }
    }
    @discardableResult public func save() async -> Bool {
        let text = draft
        let acceptedEditor = editor
        let stepInput = MemoryStepInput(
            description: text, prompt: draftHasPrompt ? draftPrompt : nil,
            reference: draftHasReference ? draftReference : nil)
        let dependent = draftReference
        let keep = draftKeepLinks.sorted()
        let refs = draftReferences
        let tc = tasksAfter
        let at = Int64(Date().timeIntervalSince1970)
        let saved = await perform(replaceEditor: true) { [session] in
            let id: UInt64
            switch acceptedEditor {
            case .newTask:
                id = try await session.createTask(text).id
            case .task(let task):
                id = try await session.editTask(task, text: text).id
            case .newNote(let task):
                _ = try await session.addNote(to: task, text: text, at: at)
                id = task.id
            case .note(let note, let task):
                try await session.editEvidence(note.id, from: task, text: text, at: at)
                id = task.id
            case .newStep(let task):
                _ = try await session.addStep(to: task, input: stepInput)
                id = task.id
            case .step(let step, let task):
                try await session.editStep(task, id: step.id, input: stepInput)
                id = task.id
            case .newCheck(let step, let task):
                _ = try await session.addCheck(to: task, stepID: step, description: text)
                id = task.id
            case .check(let check, let task):
                try await session.editCheck(task, id: check.id, description: text)
                id = task.id
            case .newDependency(let task):
                guard let predecessor = UInt64(text), let dependent = UInt64(dependent) else {
                    throw MemoryFailure.invalidInput
                }
                try await session.addDependency(
                    task, predecessor: predecessor, dependent: dependent)
                id = task.id
            case .completion(let task):
                try await session.completeTask(task, summary: text, keep: keep, references: refs)
                id = task.id

            }
            return try await session.load(selectedTaskID: id, tasksAfter: tc)
        }
        if saved {
            resetPlanCursors()
            evidenceAfter = 0
            linksAfter = 0
            decisionsAfter = 0
        }
        return saved
    }
    public func requestSelection(_ selection: MemorySelection) async {
        guard !busy, !closing else { return }
        if hasUnsavedChanges { pendingSelection = selection } else { await select(selection) }
    }
    public func cancelSelection() { pendingSelection = nil }
    public func saveAndSwitch(to selection: MemorySelection? = nil) async {
        guard let target = selection ?? pendingSelection else { return }
        if await save() { await select(target) } else { pendingSelection = target }
    }
    public func discardAndSwitch(to selection: MemorySelection? = nil) async {
        guard let target = selection ?? pendingSelection else { return }
        await select(target)
    }
    public func discardChanges() {
        guard !busy, !closing else { return }
        resetEditor()
    }
    public func deleteTask() async {
        guard !hasUnsavedChanges, let task = snapshot?.selectedTask else { return }
        if await perform(
            replaceEditor: true,
            { [session] in
                try await session.deleteTask(task)
                return try await session.load()
            })
        {
            resetCursors()
        }
    }
    public func deleteNote(_ id: UInt64) async {
        guard !hasUnsavedChanges, let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        if await perform(
            replaceEditor: true,
            { [session] in
                try await session.deleteEvidence(id, from: task)
                return try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, selectedStepID: plan.step,
                    stepsAfter: plan.steps, dependenciesAfter: plan.dependencies,
                    referencesAfter: plan.references)
            })
        {
            evidenceAfter = 0
            decisionsAfter = 0
        }
    }
    public func moveNote(_ id: UInt64, to: MemoryTask) async {
        guard !hasUnsavedChanges, let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let at = Int64(Date().timeIntervalSince1970)
        if await perform(
            replaceEditor: true,
            { [session] in
                try await session.moveEvidence(id, from: task, to: to, at: at)
                return try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, selectedStepID: plan.step,
                    stepsAfter: plan.steps, dependenciesAfter: plan.dependencies,
                    referencesAfter: plan.references)
            })
        {
            evidenceAfter = 0
            decisionsAfter = 0
        }
    }
    public func link(to: MemoryTask, related: Bool) async {
        guard !hasUnsavedChanges, let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let at = Int64(Date().timeIntervalSince1970)
        if await perform(
            replaceEditor: true,
            { [session] in
                try await session.setLink(from: task, to: to, related: related, at: at)
                return try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, selectedStepID: plan.step,
                    stepsAfter: plan.steps, dependenciesAfter: plan.dependencies,
                    referencesAfter: plan.references)
            })
        {
            linksAfter = 0
        }
    }
    public func removeLink(_ link: MemoryLink) async {
        guard !hasUnsavedChanges, let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let at = Int64(Date().timeIntervalSince1970)
        if await perform(
            replaceEditor: true,
            { [session] in
                try await session.setLink(from: task, to: link.otherTask, related: false, at: at)
                return try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, selectedStepID: plan.step,
                    stepsAfter: plan.steps, dependenciesAfter: plan.dependencies,
                    referencesAfter: plan.references)
            })
        {
            linksAfter = 0
        }
    }
    public var canToggleManualPause: Bool { manualPause != nil && !busy && !closing }
    public func loadManualPause() async {
        guard manualPause == nil else { return }
        await refreshManualPause()
    }
    public func refreshManualPause() async {
        if let inFlight, inFlight.readingPause {
            _ = await inFlight.task.value
            return
        }
        await perform(readingPause: true) { [session] in
            .pause(try await session.manualPause())
        }
    }
    public func toggleManualPause() async {
        guard let manualPause, canToggleManualPause else { return }
        await perform { [session] in
            try await session.setManualPause(!manualPause)
            return .pause(!manualPause)
        }
    }
    public func pageTasks(after: UInt64) async {
        let plan = planPosition
        let id = snapshot?.selectedTask?.id
        let ec = evidenceAfter
        let lc = linksAfter
        let dc = decisionsAfter
        if await perform(
            replaceEditor: false,
            { [session] in
                try await session.load(
                    selectedTaskID: id, tasksAfter: after, evidenceAfter: ec, linksAfter: lc,
                    decisionsAfter: dc, selectedStepID: plan.step, stepsAfter: plan.steps,
                    dependenciesAfter: plan.dependencies, referencesAfter: plan.references)
            })
        {
            tasksAfter = after
        }
    }
    public func pageNotes(after: UInt64) async {
        guard let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let lc = linksAfter
        let dc = decisionsAfter
        if await perform(
            replaceEditor: false,
            { [session] in
                try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, evidenceAfter: after, linksAfter: lc,
                    decisionsAfter: dc, selectedStepID: plan.step, stepsAfter: plan.steps,
                    dependenciesAfter: plan.dependencies, referencesAfter: plan.references)
            })
        {
            evidenceAfter = after
        }
    }
    public func pageLinks(after: UInt64) async {
        guard let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let ec = evidenceAfter
        let dc = decisionsAfter
        if await perform(
            replaceEditor: false,
            { [session] in
                try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, evidenceAfter: ec, linksAfter: after,
                    decisionsAfter: dc, selectedStepID: plan.step, stepsAfter: plan.steps,
                    dependenciesAfter: plan.dependencies, referencesAfter: plan.references)
            })
        {
            linksAfter = after
        }
    }
    public func pageDecisions(after: UInt64) async {
        guard let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let ec = evidenceAfter
        let lc = linksAfter
        if await perform(
            replaceEditor: false,
            { [session] in
                try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, evidenceAfter: ec, linksAfter: lc,
                    decisionsAfter: after, selectedStepID: plan.step, stepsAfter: plan.steps,
                    dependenciesAfter: plan.dependencies, referencesAfter: plan.references)
            })
        {
            decisionsAfter = after
        }
    }
    public func close() async -> Bool {
        closing = true
        await localProjects.close()
        await codexSources.close()
        await githubSources.close()
        await jiraSources.close()
        await todoistSources.close()
        // An accepted FFI call finishes before disposal; cancellation would not prove it stopped.
        if let inFlight { _ = await inFlight.task.value }
        do {
            try await session.close()
            return true
        } catch {
            failure = error as? MemoryFailure ?? .native
            closing = false
            return false
        }
    }
    private func select(_ target: MemorySelection) async {
        guard !busy, !closing else { return }
        switch target {
        case .newStep, .step, .newCheck, .check, .newDependency, .completion:
            await selectPlan(target)
        case .newTask:
            clearAdditionalDraft()
            editor = .newTask
            draft = ""
            pendingSelection = nil
        case .newNote:
            guard let task = snapshot?.selectedTask else { return }
            guard task.lifecycle == .open else {
                failure = .completed
                pendingSelection = nil
                return
            }
            clearAdditionalDraft()
            editor = .newNote(task)
            draft = ""
            pendingSelection = nil
        case .note(let id):
            guard let selected = snapshot?.selectedTask, id > 0 else { return }
            let tc = tasksAfter
            let plan = planPosition
            // An accepted save may change pages. Resolve the requested stable ID before replacing the draft.
            if await perform(
                replaceEditor: false,
                { [session] in
                    let loaded = try await session.load(
                        selectedTaskID: selected.id, tasksAfter: tc, evidenceAfter: id - 1,
                        selectedStepID: plan.step, stepsAfter: plan.steps,
                        dependenciesAfter: plan.dependencies, referencesAfter: plan.references)
                    guard loaded.evidence.items.first?.id == id else { throw MemoryFailure.deleted }
                    return loaded
                }), let task = snapshot?.selectedTask, let note = snapshot?.evidence.items.first
            {
                editor = .note(note, task)
                draft = note.text
                evidenceAfter = id - 1
                linksAfter = 0
                decisionsAfter = 0
                pendingSelection = nil
            }
        case .task(let id):
            let tc = tasksAfter
            if await perform(
                replaceEditor: true,
                { [session] in try await session.load(selectedTaskID: id, tasksAfter: tc) })
            {
                resetPlanCursors()
                evidenceAfter = 0
                linksAfter = 0
                decisionsAfter = 0
                pendingSelection = nil
            }
        }
    }
    private func resetEditor() {
        clearAdditionalDraft()
        editor = snapshot?.selectedTask.map(Editor.task) ?? .newTask
        draft = editor.savedText
    }
    private func resetPlanCursors() {
        stepsAfter = 0
        dependenciesAfter = 0
        referencesAfter = 0
    }
    private func resetCursors() {
        resetPlanCursors()
        tasksAfter = 0
        evidenceAfter = 0
        linksAfter = 0
        decisionsAfter = 0
    }
    @discardableResult private func perform(
        replaceEditor: Bool, _ operation: @escaping @Sendable () async throws -> MemorySnapshot
    ) async -> Bool {
        let applied = await perform {
            .snapshot(try await operation(), replaceEditor: replaceEditor)
        }
        return applied && !closing
    }
    @discardableResult private func perform(
        readingPause: Bool = false,
        _ operation: @escaping @Sendable () async throws -> OperationResult
    ) async -> Bool {
        guard !busy, !closing else { return false }
        busy = true
        failure = nil
        // Joiners and close wait for application and busy release as well as the accepted FFI call.
        let task = Task { @MainActor in
            defer {
                inFlight = nil
                busy = false
            }
            do {
                let result = try await operation()
                guard !closing else { return false }
                switch result {
                case .snapshot(let loaded, let replaceEditor):
                    snapshot = loaded
                    if replaceEditor { resetEditor() }
                case .pause(let paused): manualPause = paused
                }
                return true
            } catch {
                guard !closing else { return false }
                // Keep the original draft identity/revision even when a refresh sees newer data.
                failure = error as? MemoryFailure ?? .native
                return false
            }
        }
        inFlight = (readingPause, task)
        return await task.value
    }
    var isEditingStep: Bool {
        switch editor {
        case .newStep, .step: true
        default: false
        }
    }
    var isEditingDependency: Bool { if case .newDependency = editor { true } else { false } }
    var isCompletingTask: Bool { if case .completion = editor { true } else { false } }
    var editorKey: String {
        switch editor {
        case .newTask: "tasks.new"
        case .task(let task): task.lifecycle == .completed ? "lifecycle.summary" : "intent.editor"
        case .newNote, .note: "notes.editor"
        case .newStep: "plan.newStep"
        case .step: "plan.editStep"
        case .newCheck: "plan.newCheck"
        case .check: "plan.editCheck"
        case .newDependency: "plan.newDependency"
        case .completion: "lifecycle.summary"
        }
    }
    private func clearAdditionalDraft() {
        draftHasPrompt = false
        draftPrompt = ""
        draftHasReference = false
        draftReference = ""
        draftKeepLinks = []
        draftReferences = []
    }
    private func selectPlan(_ target: MemorySelection) async {
        guard let selected = snapshot?.selectedTask else { return }
        guard selected.lifecycle == .open else {
            failure = .completed
            pendingSelection = nil
            return
        }
        let tc = tasksAfter
        var stepID: UInt64?
        switch target {
        case .step(let id), .newCheck(let id): stepID = id
        case .check(let id): stepID = snapshot?.checks.items.first(where: { $0.id == id })?.stepID
        default: break
        }
        if let stepID, stepID == 0 || stepID > Int64.max {
            failure = .invalidInput
            return
        }
        let acceptedStepID = stepID
        guard
            await perform(
                replaceEditor: false,
                { [session] in
                    try await session.load(
                        selectedTaskID: selected.id, tasksAfter: tc, selectedStepID: acceptedStepID,
                        stepsAfter: acceptedStepID.map { $0 - 1 } ?? 0)
                }), let task = snapshot?.selectedTask
        else { return }
        clearAdditionalDraft()
        switch target {
        case .newStep: editor = .newStep(task)
        case .step:
            guard let step = snapshot?.selectedStep else { return }
            editor = .step(step, task)
            draftHasPrompt = step.prompt != nil
            draftPrompt = step.prompt ?? ""
            draftHasReference = step.reference != nil
            draftReference = step.reference ?? ""
        case .newCheck(let id): editor = .newCheck(id, task)
        case .check(let id):
            guard let check = snapshot?.checks.items.first(where: { $0.id == id }) else { return }
            editor = .check(check, task)
        case .newDependency: editor = .newDependency(task)
        case .completion: editor = .completion(task)
        default: return
        }
        draft = editor.savedText
        stepsAfter = acceptedStepID.map { $0 - 1 } ?? 0
        dependenciesAfter = 0
        referencesAfter = 0
        pendingSelection = nil
    }
    public func pagePlan(_ kind: MemoryPlanPage, after: UInt64) async {
        guard let task = snapshot?.selectedTask else { return }
        let tc = tasksAfter
        let ec = evidenceAfter
        let lc = linksAfter
        let dc = decisionsAfter
        let sc = kind == .steps ? after : stepsAfter
        let dep = kind == .dependencies ? after : dependenciesAfter
        let rc = kind == .references ? after : referencesAfter
        let sid = snapshot?.selectedStep?.id
        if await perform(
            replaceEditor: false,
            { [session] in
                try await session.load(
                    selectedTaskID: task.id, tasksAfter: tc, evidenceAfter: ec, linksAfter: lc,
                    decisionsAfter: dc, selectedStepID: sid, stepsAfter: sc, dependenciesAfter: dep,
                    referencesAfter: rc)
            })
        {
            stepsAfter = sc
            dependenciesAfter = dep
            referencesAfter = rc
        }
    }
    private func changePlan(
        keepStep: Bool = true,
        _ operation: @escaping @Sendable (MemorySession, MemoryTask) async throws -> Void
    ) async {
        guard !hasUnsavedChanges, let task = snapshot?.selectedTask else { return }
        let plan = planPosition
        let tc = tasksAfter
        let sid = keepStep ? snapshot?.selectedStep?.id : nil
        await perform(replaceEditor: true) { [session] in
            try await operation(session, task)
            return try await session.load(
                selectedTaskID: task.id, tasksAfter: tc, selectedStepID: sid,
                stepsAfter: plan.steps, dependenciesAfter: plan.dependencies,
                referencesAfter: plan.references)
        }
    }
    public func confirmPlan() async {
        await changePlan { try await $0.setPlanCompleteness($1, value: .complete) }
    }
    public func toggleInitiative() async {
        await changePlan {
            try await $0.setInitiativeSuppressed($1, value: !$1.initiativeSuppressed)
        }
    }
    public func reopenTask() async {
        await changePlan(keepStep: false) { try await $0.reopenTask($1) }
    }
    public func completeStep(_ step: MemoryStep) async {
        let at: Int64? = step.completedAtUTC == nil ? Int64(Date().timeIntervalSince1970) : nil
        await changePlan { try await $0.setStepCompleted($1, id: step.id, at: at) }
    }
    public func deleteStep(_ id: UInt64) async {
        await changePlan(keepStep: false) { try await $0.deleteStep($1, id: id) }
    }
    public func deleteCheck(_ id: UInt64) async {
        await changePlan { try await $0.deleteCheck($1, id: id) }
    }
    public func deleteDependency(_ id: UInt64) async {
        await changePlan { try await $0.deleteDependency($1, id: id) }
    }
    public func setCheck(_ id: UInt64, result: MemoryCheckResult) async {
        let at: Int64? = result == .unknown ? nil : Int64(Date().timeIntervalSince1970)
        await changePlan { try await $0.setCheckResult($1, id: id, result: result, at: at) }
    }
    public func copy(_ text: String, to pasteboard: NSPasteboard = .general) {
        pasteboard.clearContents()
        copied = pasteboard.setString(text, forType: .string)
        if !copied { failure = .clipboard }
    }
}
