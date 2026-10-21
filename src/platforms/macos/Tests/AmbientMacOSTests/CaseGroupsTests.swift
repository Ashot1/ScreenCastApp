import Testing

@testable import AmbientMacOS

/// Core pages tasks by ID with open and completed cases interleaved, so the sidebar groups only
/// the loaded page.
@Test func sidebarSplitsOnlyTheLoadedPageIntoOpenAndCompletedCases() {
    let first = snapshot(
        [task(1, .open), task(2, .completed), task(3, .open), task(4, .completed)], next: 4)
    #expect(first.openTasks.map(\.id) == [1, 3])
    #expect(first.completedTasks.map(\.id) == [2, 4])

    let completedOnly = snapshot([task(5, .completed), task(6, .completed)], next: 6)
    #expect(completedOnly.openTasks.isEmpty)
    #expect(completedOnly.completedTasks.map(\.id) == [5, 6])

    let last = snapshot([task(7, .open)], next: nil)
    #expect(last.openTasks.map(\.id) == [7])
    #expect(last.completedTasks.isEmpty)

    let empty = snapshot([], next: nil)
    #expect(empty.openTasks.isEmpty && empty.completedTasks.isEmpty)
}

private func task(_ id: UInt64, _ lifecycle: MemoryTaskLifecycle) -> MemoryTask {
    MemoryTask(
        id: id, revision: 1, text: "Task \(id)", lifecycle: lifecycle,
        initiativeSuppressed: false, planCompleteness: .unknown)
}

private func snapshot(_ tasks: [MemoryTask], next: UInt64?) -> MemorySnapshot {
    MemorySnapshot(
        tasks: MemoryPage(items: tasks, nextAfter: next), selectedTask: tasks.first,
        evidence: .empty, links: .empty, decisions: .empty, steps: .empty, selectedStep: nil,
        checks: .empty, dependencies: .empty, references: .empty)
}
