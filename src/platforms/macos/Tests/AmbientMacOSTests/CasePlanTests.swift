import Foundation
import Testing

@testable import AmbientMacOS

/// «Следующий шаг» is the first ready, unfinished step of the loaded page; readiness comes only
/// from Core, so a later page is never guessed.
@Test func nextStepIsTheFirstReadyUnfinishedStepOfTheLoadedPage() {
    let page = plan([step(1, done: true), step(2), step(3, ready: true), step(4, ready: true)])
    #expect(page.nextStep?.id == 3)

    // A step Core still marks ready but that is already done is not next.
    let finished = plan([step(5, done: true, ready: true), step(6, ready: true)])
    #expect(finished.nextStep?.id == 6)

    // None on this page while more pages exist: the block hides instead of reading further.
    let boundary = plan([step(7, done: true), step(8)], next: 8)
    #expect(boundary.nextStep == nil)

    #expect(plan([step(9)]).nextStep == nil)
    #expect(plan([]).nextStep == nil)
}

/// «ждёт шаг N» names unfinished predecessors only when every dependency and every predecessor
/// of the step are loaded.
@Test func waitingStepsAreShownOnlyWithLoadedDependenciesAndPredecessors() {
    let steps = [step(1, done: true), step(2), step(3), step(4)]
    let deps = [dependency(1, 1, 3), dependency(2, 2, 3), dependency(3, 3, 4)]
    let loaded = plan(steps, dependencies: deps)
    #expect(loaded.waitingSteps(for: steps[2], dependenciesFromStart: true) == [2])
    #expect(loaded.waitingSteps(for: steps[3], dependenciesFromStart: true) == [3])
    #expect(loaded.waitingSteps(for: steps[1], dependenciesFromStart: true) == [])
    // A ready or finished step waits for nothing.
    #expect(loaded.waitingSteps(for: steps[0], dependenciesFromStart: true) == nil)
    let ready = step(2, ready: true)
    #expect(loaded.waitingSteps(for: ready, dependenciesFromStart: true) == nil)

    // Dependencies not loaded from their first page, or with more pages: unknown.
    #expect(loaded.waitingSteps(for: steps[2], dependenciesFromStart: false) == nil)
    let morePages = plan(steps, dependencies: deps, dependenciesNext: 3)
    #expect(morePages.waitingSteps(for: steps[2], dependenciesFromStart: true) == nil)

    // A predecessor outside the loaded step page: unknown whether it is finished.
    let otherPage = plan([step(3), step(4)], next: 4, dependencies: deps)
    #expect(otherPage.waitingSteps(for: step(3), dependenciesFromStart: true) == nil)
    #expect(otherPage.waitingSteps(for: step(4), dependenciesFromStart: true) == [3])
}

/// D76: a source read belongs to one case revision and to the unpaused state. The model cancels
/// all five sources on a case change (ID or revision) and on a pause change, and opening a step
/// or an editor keeps the selected case and the read.
@MainActor @Test func caseChangesAndPauseCancelAllSourcesButStepsAndEditorsDoNot() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-case-plan-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }
    let s = MemorySession(databaseURL: directory.appendingPathComponent("memory.sqlite3"))
    let a = try await s.createTask("First")
    let b = try await s.createTask("Second")
    let stepID = try await s.addStep(
        to: try await s.task(b.id),
        input: MemoryStepInput(description: "Step", prompt: nil, reference: nil))
    let model = AssistantModel(session: s)
    await model.reload()
    await model.requestSelection(.task(a.id))
    #expect(model.snapshot?.selectedTask?.id == a.id)

    var before = generations(model)
    await model.requestSelection(.task(b.id))
    #expect(model.snapshot?.selectedTask?.id == b.id)
    #expect(allAdvanced(before, generations(model)))

    // Same case, new revision: a note saved on another tab.
    before = generations(model)
    await model.requestSelection(.newNote)
    model.draft = "Note"
    await model.save()
    #expect(model.snapshot?.selectedTask?.id == b.id && model.failure == nil)
    #expect(allAdvanced(before, generations(model)))

    before = generations(model)
    await model.toggleManualPause()
    #expect(model.manualPause == true)
    #expect(allAdvanced(before, generations(model)))
    await model.toggleManualPause()
    #expect(model.manualPause == false)

    // Opening a step or an editor reloads the snapshot but keeps the selected case and the read.
    let selected = model.snapshot?.selectedTask
    before = generations(model)
    for selection in [
        MemorySelection.step(stepID), .newCheck(stepID), .newStep, .newDependency, .newNote,
        .completion,
    ] {
        await model.requestSelection(selection)
        #expect(model.pendingSelection == nil && model.failure == nil)
        #expect(model.snapshot?.selectedTask == selected)
    }
    #expect(generations(model) == before)
    model.discardChanges()
    await model.reload()
    #expect(generations(model) == before)
    #expect(await model.close())
}

@MainActor private func generations(_ model: AssistantModel) -> [UInt64] {
    [
        model.localProjects.generation, model.codexSources.generation,
        model.githubSources.generation, model.jiraSources.generation,
        model.todoistSources.generation,
    ]
}

private func allAdvanced(_ before: [UInt64], _ after: [UInt64]) -> Bool {
    zip(before, after).allSatisfy { $1 > $0 }
}

private func step(_ id: UInt64, done: Bool = false, ready: Bool = false) -> MemoryStep {
    MemoryStep(
        id: id, taskID: 1, description: "Step \(id)", prompt: nil, reference: nil,
        completedAtUTC: done ? 1 : nil, ready: ready)
}

private func dependency(_ id: UInt64, _ predecessor: UInt64, _ dependent: UInt64)
    -> MemoryDependency
{
    MemoryDependency(id: id, taskID: 1, predecessor: predecessor, dependent: dependent)
}

private func plan(
    _ steps: [MemoryStep], next: UInt64? = nil, dependencies: [MemoryDependency] = [],
    dependenciesNext: UInt64? = nil
) -> MemorySnapshot {
    let task = MemoryTask(
        id: 1, revision: 1, text: "Task", lifecycle: .open, initiativeSuppressed: false,
        planCompleteness: .complete)
    return MemorySnapshot(
        tasks: MemoryPage(items: [task], nextAfter: nil), selectedTask: task, evidence: .empty,
        links: .empty, decisions: .empty, steps: MemoryPage(items: steps, nextAfter: next),
        selectedStep: nil, checks: .empty,
        dependencies: MemoryPage(items: dependencies, nextAfter: dependenciesNext),
        references: .empty)
}
