import AppKit
import CAmbient
import Foundation
import Testing

@testable import AmbientMacOS

private struct DatabaseDirectory {
    let url: URL
    init() throws {
        url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false)
    }
    var databaseURL: URL { url.appendingPathComponent("memory.sqlite3") }
    func remove() throws { try FileManager.default.removeItem(at: url) }
}
@Test func multipleTasksNotesLinksAndPauseSurviveRealNativeRestart() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let a = try await s.createTask(" A🌿\n")
    let b = try await s.createTask("B")
    let n = try await s.addNote(to: a, text: "note\0🌿", at: 1_700_000_000)
    let first = try await s.load(selectedTaskID: a.id)
    #expect(first.tasks.items.count == 2)
    #expect(first.evidence.items[0].origin == .user)
    try await s.moveEvidence(n, from: try #require(first.selectedTask), to: b, at: 1_700_000_001)
    let moved = try await s.load(selectedTaskID: b.id)
    #expect(moved.evidence.items[0].text == "note\0🌿")
    let currentA = try await s.task(a.id)
    try await s.setLink(
        from: try #require(moved.selectedTask), to: currentA, related: true, at: 1_700_000_002)
    try await s.setManualPause(true)
    try await s.close()
    let reopened = MemorySession(databaseURL: d.databaseURL)
    let loaded = try await reopened.load(selectedTaskID: b.id)
    #expect(try await reopened.manualPause())
    #expect(loaded.links.items[0].otherTaskID == a.id)
    try await reopened.deleteTask(try #require(loaded.selectedTask))
    let remaining = try await reopened.load(selectedTaskID: a.id)
    #expect(remaining.links.items.isEmpty)
    #expect(remaining.tasks.items.count == 1)
    try await reopened.close()
}
@Test func staleWritesByteLimitsAndClosedSessionPreserveMemory() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let a = try await s.createTask("Original")
    _ = try await s.editTask(a, text: String(repeating: "🌿", count: 4096))
    await #expect(throws: MemoryFailure.stale) { try await s.editTask(a, text: "late") }
    let current = try await s.task(a.id)
    await #expect(throws: MemoryFailure.invalidInput) { try await s.editTask(current, text: "") }
    await #expect(throws: MemoryFailure.invalidInput) {
        try await s.editTask(current, text: String(repeating: "x", count: 16385))
    }
    #expect(try await s.task(a.id).text.utf8.count == 16384)
    try await s.close()
    await #expect(throws: MemoryFailure.closed) { try await s.load() }
    try await s.close()
}
@MainActor @Test func draftSwitchSupportsSaveDiscardCancelAndPreservesFailedSave() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let session = MemorySession(databaseURL: d.databaseURL)
    let a = try await session.createTask("A")
    let b = try await session.createTask("B")
    let m = AssistantModel(session: session)
    await m.reload()
    m.draft = "Unfinished"
    await m.requestSelection(.task(b.id))
    #expect(m.pendingSelection != nil)
    #expect(m.snapshot?.selectedTask?.id == a.id)
    m.cancelSelection()
    #expect(m.draft == "Unfinished")
    await m.requestSelection(.task(b.id))
    await m.saveAndSwitch()
    #expect(m.snapshot?.selectedTask?.id == b.id)
    #expect(try await session.task(a.id).text == "Unfinished")
    m.draft = "Discard me"
    await m.requestSelection(.task(a.id))
    await m.discardAndSwitch()
    #expect(m.draft == "Unfinished")
    m.draft = ""
    await m.reload()
    #expect(m.draft.isEmpty)
    await m.save()
    #expect(m.failure == .invalidInput)
    #expect(m.draft.isEmpty)
    await m.requestSelection(.task(b.id))
    await m.saveAndSwitch()
    #expect(m.pendingSelection != nil)
    #expect(m.snapshot?.selectedTask?.id == a.id)
    #expect(await m.close())
}
@MainActor @Test func reloadDoesNotRebaseDirtyDraftAcrossNewRevision() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let a = try await s.createTask("\u{E9}")
    let m = AssistantModel(session: s)
    await m.reload()
    m.draft = "e\u{301}"
    #expect(m.hasUnsavedChanges)
    let other = MemorySession(databaseURL: d.databaseURL)
    _ = try await other.editTask(a, text: "External edit")
    await m.reload()
    await m.save()
    #expect(m.failure == .stale)
    #expect(Array(m.draft.utf8) == [101, 204, 129])
    #expect(m.snapshot?.selectedTask?.text == "External edit")
    m.discardChanges()
    #expect(m.draft == "External edit")
    #expect(await m.close())
    try await other.close()
}
@Test func externalSourceForgottenAndProtectedPathsCrossSwiftCRust() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let a = try await s.createTask("A")
    let b = try await s.createTask("B")
    let key = MemorySourceKey(namespace: "fixture", scope: "account", objectID: "exact")
    let input = MemorySourceEvidence(
        text: "old", observedAtUTC: nil, completeness: .unknown, kind: .sourceFact)
    let result = try await s.ingestSource(to: a, key: key, version: "v1", evidence: input)
    guard case .applied(let id) = result else {
        Issue.record("expected imported source")
        return
    }
    let loaded = try await s.load(selectedTaskID: a.id)
    #expect(loaded.evidence.items[0].observedAtUTC == nil)
    try await s.deleteEvidence(id, from: try #require(loaded.selectedTask))
    let current = try await s.task(a.id)
    #expect(
        try await s.ingestSource(to: current, key: key, version: "v2", evidence: input)
            == .forgotten)
    let fresh = MemorySourceEvidence(
        text: "new explicit", observedAtUTC: nil, completeness: .partial, kind: .sourceFact)
    let newID = try await s.bindSource(to: b, key: key, version: nil, evidence: fresh, at: 10)
    #expect(newID != id)
    let b2 = try await s.task(b.id)
    #expect(
        try await s.ingestSource(to: b2, key: key, version: "v3", evidence: input)
            == .applied(newID))
    let b3 = try await s.task(b.id)
    try await s.editEvidence(newID, from: b3, text: "User corrected text", at: 11)
    let b4 = try await s.task(b.id)
    #expect(
        try await s.ingestSource(to: b4, key: key, version: "v4", evidence: input) == .userProtected
    )
    #expect(try await s.load(selectedTaskID: b.id).evidence.items[0].text == "User corrected text")
    try await s.close()
}
@Test func corruptDatabaseStaysUnknownAndByteIdentical() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let bytes = Data("sanitized corrupt database".utf8)
    try bytes.write(to: d.databaseURL)
    let s = MemorySession(databaseURL: d.databaseURL)
    await #expect(throws: MemoryFailure.schema) { try await s.load() }
    #expect(try Data(contentsOf: d.databaseURL) == bytes)
    try await s.close()
}
@MainActor @Test func acceptedSwitchSurvivesAlertBindingDismissal() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    _ = try await s.createTask("A")
    let b = try await s.createTask("B")
    let m = AssistantModel(session: s)
    await m.reload()
    m.draft = "edit"
    await m.requestSelection(.task(b.id))
    let accepted = m.pendingSelection
    m.cancelSelection()
    await m.saveAndSwitch(to: accepted)
    #expect(m.snapshot?.selectedTask?.id == b.id)
    #expect(await m.close())
}

@MainActor @Test func saveAndSwitchFindsNoteBeyondCurrentPageByStableID() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let a = try await s.createTask("A")
    var current = a
    var last: UInt64 = 0
    for index in 1...51 {
        last = try await s.addNote(to: current, text: "Note \(index)", at: 1)
        current = try await s.task(a.id)
    }
    let m = AssistantModel(session: s)
    await m.reload()
    let next = try #require(m.snapshot?.evidence.nextAfter)
    await m.pageNotes(after: next)
    m.draft = "Changed task"
    await m.requestSelection(.note(last))
    await m.saveAndSwitch()
    #expect(m.pendingSelection == nil)
    #expect(m.draft == "Note 51")
    #expect(m.editorKind == .note)
    m.draft = "Corrected note"
    await m.save()
    #expect(try await s.evidence(taskID: a.id, after: last - 1).items[0].text == "Corrected note")
    #expect(await m.close())
}

@Test func manualPlanAndLifecycleCrossNativeBoundaryWithoutStartingAssistant() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let t = try await s.createTask("Affair")
    let id = try await s.addStep(
        to: t,
        input: MemoryStepInput(description: "Step", prompt: " e\u{301}\0🌿\n", reference: "exact:🌿"))
    var current = try await s.task(t.id)
    let checkID = try await s.addCheck(to: current, stepID: id, description: "Verified")
    current = try await s.task(t.id)
    try await s.setPlanCompleteness(current, value: .complete)
    var loaded = try await s.load(selectedTaskID: t.id, selectedStepID: id)
    #expect(loaded.steps.items[0].ready)
    #expect(Array(try #require(loaded.steps.items[0].prompt).utf8) == Array(" e\u{301}\0🌿\n".utf8))
    #expect(loaded.checks.items[0].result == .unknown)
    await #expect(throws: MemoryFailure.invalidInput) {
        try await s.setStepCompleted(try #require(loaded.selectedTask), id: id, at: 1)
    }
    try await s.setCheckResult(
        try #require(loaded.selectedTask), id: checkID, result: .passed, at: 2)
    try await s.setStepCompleted(try await s.task(t.id), id: id, at: 3)
    try await s.setInitiativeSuppressed(try await s.task(t.id), value: true)
    try await s.completeTask(
        try await s.task(t.id), summary: "Summary", keep: [], references: [" exact:🌿 "])
    loaded = try await s.load(selectedTaskID: t.id)
    #expect(loaded.selectedTask?.lifecycle == .completed)
    #expect(loaded.selectedTask?.initiativeSuppressed == true)
    #expect(loaded.steps.items.isEmpty)
    #expect(loaded.references.items[0].text == " exact:🌿 ")
    await #expect(throws: MemoryFailure.completed) {
        try await s.addNote(to: try #require(loaded.selectedTask), text: "Late", at: 4)
    }
    try await s.reopenTask(try #require(loaded.selectedTask))
    try await s.close()
    let restarted = MemorySession(databaseURL: d.databaseURL)
    loaded = try await restarted.load(selectedTaskID: t.id)
    #expect(loaded.selectedTask?.planCompleteness == .unknown)
    #expect(loaded.selectedTask?.initiativeSuppressed == true)
    #expect(loaded.evidence.items.isEmpty)
    #expect(loaded.references.items.count == 1)
    try await restarted.close()
}

@MainActor @Test func planDraftSurvivesPageChangesFailureAndFormSwitch() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let t = try await s.createTask("Affair")
    var current = t
    var last: UInt64 = 0
    for i in 1...51 {
        last = try await s.addStep(
            to: current,
            input: MemoryStepInput(description: "Step \(i)", prompt: nil, reference: nil))
        current = try await s.task(t.id)
    }
    let m = AssistantModel(session: s)
    await m.reload()
    await m.pagePlan(.steps, after: try #require(m.snapshot?.steps.nextAfter))
    await m.requestSelection(.step(last))
    m.draft = "Changed step"
    m.draftHasPrompt = true
    m.draftPrompt = " e\u{301}\0 "
    await m.pagePlan(.steps, after: 0)
    #expect(m.draft == "Changed step")
    await m.requestSelection(.newCheck(last))
    #expect(m.pendingSelection != nil)
    m.cancelSelection()
    #expect(m.draftPrompt == " e\u{301}\0 ")
    try await s.setPlanCompleteness(current, value: .complete)
    await m.reload()
    await m.save()
    #expect(m.failure == .stale)
    #expect(m.draft == "Changed step")
    await m.requestSelection(.task(t.id))
    await m.discardAndSwitch()
    #expect(m.draft == "Affair")
    await m.requestSelection(.step(last))
    m.draft = "Saved step"
    await m.requestSelection(.newCheck(last))
    await m.saveAndSwitch()
    #expect(m.pendingSelection == nil)
    m.draft = "Check"
    #expect(await m.save())
    #expect(try await s.checks(taskID: t.id, stepID: last).items[0].description == "Check")
    #expect(await m.close())
}

@MainActor @Test func explicitCopyPreservesExactTextWithoutChangingTask() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let task = try await s.createTask("Affair")
    let model = AssistantModel(session: s)
    await model.reload()
    let pasteboard = NSPasteboard(name: .init("ambient-test-" + UUID().uuidString))
    defer { pasteboard.releaseGlobally() }
    let text = " e\u{301}\0🌿\n"
    model.copy(text, to: pasteboard)
    #expect(model.copied)
    #expect(Array(try #require(pasteboard.string(forType: .string)).utf8) == Array(text.utf8))
    #expect(try await s.task(task.id) == task)
    #expect(await model.close())
}

@MainActor @Test func completingWhileSwitchingCannotOpenAnAddFormForCompletedTask() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let task = try await s.createTask("Affair")
    let model = AssistantModel(session: s)
    await model.reload()
    await model.requestSelection(.completion)
    model.draft = "Summary"
    await model.requestSelection(.newStep)
    await model.saveAndSwitch()
    #expect(model.snapshot?.selectedTask?.lifecycle == .completed)
    #expect(model.pendingSelection == nil)
    #expect(model.failure == .completed)
    #expect(model.editorKind == .task)
    #expect(model.draft == "Summary")
    await model.requestSelection(.newNote)
    #expect(model.editorKind == .task)
    #expect(try await s.task(task.id).lifecycle == .completed)
    #expect(await model.close())
}

@MainActor @Test func selectedPlanPageSurvivesReloadOtherPagesAndManualPause() async throws {
    let d = try DatabaseDirectory()
    defer { try? d.remove() }
    let s = MemorySession(databaseURL: d.databaseURL)
    let task = try await s.createTask("Affair")
    var current = task
    var last: UInt64 = 0
    for index in 1...51 {
        last = try await s.addStep(
            to: current,
            input: MemoryStepInput(description: "Step \(index)", prompt: nil, reference: nil))
        current = try await s.task(task.id)
    }
    _ = try await s.addCheck(to: current, stepID: last, description: "Final check")
    let model = AssistantModel(session: s)
    await model.reload()
    await model.requestSelection(.step(last))
    await model.reload()
    await model.reload()
    #expect(model.snapshot?.steps.items.first?.id == last)
    let refreshes: [() async -> Void] = [
        { await model.pageTasks(after: 0) }, { await model.pageNotes(after: 0) },
        { await model.pageLinks(after: 0) }, { await model.pageDecisions(after: 0) },
        { await model.toggleManualPause() },
    ]
    model.draft = "Unsaved task text"
    for refresh in refreshes {
        await refresh()
        #expect(model.snapshot?.steps.items.first?.id == last)
        #expect(model.snapshot?.selectedStep?.id == last)
        #expect(model.snapshot?.checks.items.first?.description == "Final check")
        #expect(model.draft == "Unsaved task text")
    }
    #expect(await model.close())
}

@MainActor @Test func manualPauseDoesNotReloadContentOrReplaceIndependentDraft() async throws {
    let directory = try DatabaseDirectory()
    defer { try? directory.remove() }
    let session = MemorySession(databaseURL: directory.databaseURL)
    let task = try await session.createTask("Saved task")
    let model = AssistantModel(session: session)
    await model.reload()
    let before = try #require(model.snapshot)
    model.draft = "Unsaved draft"
    _ = try await session.editTask(task, text: "External revision")

    await model.toggleManualPause()

    #expect(model.snapshot == before)
    await model.refreshManualPause()
    #expect(model.manualPause == true)
    #expect(model.snapshot == before)
    #expect(model.draft == "Unsaved draft")
    #expect(model.hasUnsavedChanges)
    #expect(model.failure == nil)
    #expect(await model.close())
}

@MainActor @Test func pauseOnlyModelPersistsWithoutLoadingTaskContent() async throws {
    let directory = try DatabaseDirectory()
    defer { try? directory.remove() }
    let session = MemorySession(databaseURL: directory.databaseURL)
    let model = AssistantModel(session: session)
    model.draft = "Independent draft"
    await model.toggleManualPause()
    #expect(model.manualPause == nil)
    #expect(!model.canToggleManualPause)
    await model.loadManualPause()
    #expect(model.manualPause == false)
    #expect(model.canToggleManualPause)
    await model.toggleManualPause()
    #expect(model.manualPause == true)
    #expect(model.snapshot == nil)
    #expect(model.draft == "Independent draft")
    #expect(await model.close())

    let reopened = MemorySession(databaseURL: directory.databaseURL)
    #expect(try await reopened.manualPause())
    #expect(try await reopened.tasks().items.isEmpty)
    try await reopened.close()
}

@MainActor @Test func pauseReadAndWriteErrorsPreserveUnknownOrPriorValue() async throws {
    let directory = try DatabaseDirectory()
    defer { try? directory.remove() }
    let bytes = Data("sanitized corrupt database".utf8)
    try bytes.write(to: directory.databaseURL)
    let damaged = AssistantModel(session: MemorySession(databaseURL: directory.databaseURL))
    await damaged.loadManualPause()
    #expect(damaged.manualPause == nil)
    #expect(damaged.failure == .schema)
    #expect(!damaged.canToggleManualPause)
    #expect(damaged.snapshot == nil)
    #expect(try Data(contentsOf: directory.databaseURL) == bytes)
    #expect(await damaged.close())

    let other = try DatabaseDirectory()
    defer { try? other.remove() }
    let session = MemorySession(databaseURL: other.databaseURL)
    try await session.setManualPause(true)
    let model = AssistantModel(session: session)
    await model.loadManualPause()
    model.draft = "Keep on failure"
    try await session.close()
    await model.toggleManualPause()
    #expect(model.manualPause == true)
    #expect(model.failure == .closed)
    await model.refreshManualPause()
    #expect(model.manualPause == true)
    #expect(model.failure == .closed)
    #expect(model.snapshot == nil)
    #expect(model.draft == "Keep on failure")
    #expect(await model.close())
}

// Hold the real storage actor to make accepted FFI work deterministic without production hooks.
private struct MemorySessionGate: Sendable {
    let entered: AsyncStream<Void>
    let signal: AsyncStream<Void>.Continuation
    let release = DispatchSemaphore(value: 0)
    init() { (entered, signal) = AsyncStream.makeStream() }
}
extension MemorySession {
    fileprivate func holdForTest(_ gate: MemorySessionGate) {
        gate.signal.yield(())
        _ = gate.release.wait(timeout: .now() + 10)
    }
}
@MainActor private func requireEventually(_ condition: () -> Bool) async throws {
    let deadline = ContinuousClock.now.advanced(by: .seconds(3))
    while !condition(), ContinuousClock.now < deadline { await Task.yield() }
    try #require(condition())
}

@MainActor @Test func simultaneousMenuAndWindowStartLoadsMemoryAfterPauseAndPreservesNewDraft()
    async throws
{
    let directory = try DatabaseDirectory()
    defer { try? directory.remove() }
    let session = MemorySession(databaseURL: directory.databaseURL)
    _ = try await session.createTask("Saved task")
    let model = AssistantModel(session: session)
    let gate = MemorySessionGate()
    let held = Task { await session.holdForTest(gate) }
    defer { gate.release.signal() }
    var entered = gate.entered.makeAsyncIterator()
    _ = await entered.next()
    let menu = Task { await model.loadManualPause() }
    try await requireEventually { model.busy }
    #expect(!model.canToggleManualPause)
    var windowStarted = false
    let window = Task {
        windowStarted = true
        await model.reload()
    }
    try await requireEventually { windowStarted }
    model.draft = "Typed while the saved pause is loading"
    gate.release.signal()
    await held.value
    await menu.value
    await window.value
    #expect(model.manualPause == false)
    #expect(model.snapshot?.selectedTask?.text == "Saved task")
    #expect(model.draft == "Typed while the saved pause is loading")
    #expect(!model.busy)
    #expect(model.canToggleManualPause)
    #expect(model.failure == nil)
    #expect(await model.close())
}

@MainActor @Test(arguments: [false, true])
func closeWaitsForAcceptedPauseAndSuppressesLatePublication(writing: Bool) async throws {
    let directory = try DatabaseDirectory()
    defer { try? directory.remove() }
    let session = MemorySession(databaseURL: directory.databaseURL)
    let model = AssistantModel(session: session)
    if writing { await model.loadManualPause() } else { try await session.setManualPause(true) }
    let prior = model.manualPause
    let gate = MemorySessionGate()
    let held = Task { await session.holdForTest(gate) }
    defer { gate.release.signal() }
    var entered = gate.entered.makeAsyncIterator()
    _ = await entered.next()
    let accepted = Task {
        if writing { await model.toggleManualPause() } else { await model.loadManualPause() }
    }
    try await requireEventually { model.busy }
    model.draft = "Do not save while busy"
    #expect(!(await model.save()))
    var closed = false
    let closing = Task {
        closed = await model.close()
    }
    try await requireEventually { model.closing }
    #expect(!closed)
    #expect(model.busy)
    #expect(!model.canToggleManualPause)
    await model.toggleManualPause()
    gate.release.signal()
    await held.value
    await accepted.value
    await closing.value
    #expect(closed)
    #expect(!model.busy)
    #expect(model.closing)
    #expect(model.manualPause == prior)
    #expect(model.snapshot == nil)
    #expect(model.draft == "Do not save while busy")
    #expect(model.failure == nil)
    let reopened = MemorySession(databaseURL: directory.databaseURL)
    #expect(try await reopened.manualPause())
    #expect(try await reopened.tasks().items.isEmpty)
    try await reopened.close()
}

@MainActor @Test func permissionRecheckReplacesOnlyItsLocalResults() async throws {
    var permissions = LocalPermissions()
    #expect(permissions.screenCapture == nil)
    #expect(permissions.accessibility == nil)
    let directory = try DatabaseDirectory()
    defer { try? directory.remove() }
    let session = MemorySession(databaseURL: directory.databaseURL)
    let model = AssistantModel(session: session)
    await model.loadManualPause()
    model.draft = "Keep this draft"
    permissions.check(screenCapture: { false }, accessibility: { true })
    #expect(permissions.screenCapture == false)
    #expect(permissions.accessibility == true)
    permissions.check(screenCapture: { true }, accessibility: { false })
    #expect(permissions.screenCapture == true)
    #expect(permissions.accessibility == false)
    #expect(model.manualPause == false)
    #expect(model.snapshot == nil)
    #expect(model.draft == "Keep this draft")
    #expect(model.failure == nil)
    #expect(await model.close())
}
