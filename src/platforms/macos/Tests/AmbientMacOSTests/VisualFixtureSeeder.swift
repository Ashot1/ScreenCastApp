import Foundation
import Testing

@testable import AmbientMacOS

/// Writes the template roots of the «Ambient Visual» build (ADR-024) into `AMBIENT_VISUAL_SEED_DIR`:
/// `empty/` (first launch), `corrupt/` (memory that fails to open) and `cases/` (cases like the
/// mockup). Runs only when the variable is set; each screenshot session copies a template.
@Test(.enabled(if: ProcessInfo.processInfo.environment["AMBIENT_VISUAL_SEED_DIR"] != nil))
func seedVisualFixtureTemplates() async throws {
    let seed = URL(
        filePath: try #require(ProcessInfo.processInfo.environment["AMBIENT_VISUAL_SEED_DIR"]),
        directoryHint: .isDirectory)
    let fm = FileManager.default
    for name in ["empty", "corrupt", "cases"] {
        let root = seed.appending(path: name, directoryHint: .isDirectory)
        try? fm.removeItem(at: root)
        try fm.createDirectory(at: root, withIntermediateDirectories: true)
    }
    try Data(repeating: 0x5A, count: 4096).write(
        to: seed.appending(path: "corrupt/memory.sqlite3"))
    let s = MemorySession(databaseURL: seed.appending(path: "cases/memory.sqlite3"))
    try await seedCases(s)
    try await s.close()
}

private let october21: Int64 = 1_792_560_000  // 2026-10-21 00:00 UTC

private func seedCases(_ s: MemorySession) async throws {
    // Sidebar order follows creation; the selected case of the mockup comes first.
    let main = try await s.createTask("Починить повторный вход ChatGPT после истечения сессии")
    let review = try await s.createTask("Ревью PR #482: лента предложений в краевой панели")
    let reply = try await s.createTask("Ответить в AMB-311 про пределы контекста и размер запроса")
    let storage = try await s.createTask("Перенести хранилище дел на новую схему миграций")
    _ = try await s.createTask("Собрать заметки к выпуску для Windows")
    let done = try await s.createTask("Проверить исключения папок при сканировании Git")

    // Plan: step 1 done, step 2 next with a prompt, a reference and two checks, step 3 waits.
    let reproduce = try await s.addStep(
        to: try await s.task(main.id),
        input: MemoryStepInput(
            description: "Воспроизвести истечение сессии на тестовом профиле", prompt: nil,
            reference: nil))
    let find = try await s.addStep(
        to: try await s.task(main.id),
        input: MemoryStepInput(
            description: "Найти, где теряется результат обновления сессии",
            prompt:
                "Проследи путь результата «Обновить сессию» до статуса в Настройках и найди место, где он становится «неизвестно».",
            reference: "https://github.com/owner/ambient-mac/pull/482"))
    let confirm = try await s.addStep(
        to: try await s.task(main.id),
        input: MemoryStepInput(
            description: "Добавить подтверждение «Я не завершил вход»", prompt: nil,
            reference: nil))
    try await s.addDependency(try await s.task(main.id), predecessor: find, dependent: confirm)
    try await s.setStepCompleted(try await s.task(main.id), id: reproduce, at: october21 + 36_000)
    _ = try await s.addCheck(
        to: try await s.task(main.id), stepID: find, description: "Статус после обновления верный")
    let kept = try await s.addCheck(
        to: try await s.task(main.id), stepID: find,
        description: "Итог не теряется при закрытии окна")
    try await s.setCheckResult(
        try await s.task(main.id), id: kept, result: .passed, at: october21 + 40_000)
    try await s.setPlanCompleteness(try await s.task(main.id), value: .partial)

    // Notes: a user note, an assumption and source facts. Integration namespaces are reserved for
    // the integrations themselves, so the fixture uses its own namespace.
    _ = try await s.addNote(
        to: try await s.task(main.id),
        text: "Повторный вход нужен и после смены профиля проверки.", at: october21 - 25_000)
    _ = try await s.ingestSource(
        to: try await s.task(main.id),
        key: MemorySourceKey(
            namespace: "visual-fixture", scope: "github.com/owner/ambient-mac", objectID: "pull/482"
        ),
        version: "a3f9c21",
        evidence: MemorySourceEvidence(
            text: "PR #482 меняет обработку ответа входа; ревью ещё не завершено.",
            observedAtUTC: october21 + 52_320, completeness: .partial, kind: .sourceFact))
    _ = try await s.ingestSource(
        to: try await s.task(main.id),
        key: MemorySourceKey(
            namespace: "visual-fixture", scope: "team.atlassian.net", objectID: "AMB-311"),
        version: "1",
        evidence: MemorySourceEvidence(
            text: "Похоже, статус сбрасывается при закрытии окна настроек.", observedAtUTC: nil,
            completeness: .unknown, kind: .assumption))
    _ = try await s.ingestSource(
        to: try await s.task(main.id),
        key: MemorySourceKey(
            namespace: "visual-fixture", scope: "github.com/owner/ambient-mac",
            objectID: "issue/479"),
        version: "7c1e0b4",
        evidence: MemorySourceEvidence(
            text: "Результат обновления не сохраняется, если окно настроек закрыто.",
            observedAtUTC: october21 + 52_560, completeness: .partial, kind: .sourceFact))

    // Links: two open cases and the completed one.
    try await s.setLink(
        from: try await s.task(main.id), to: try await s.task(review.id), related: true,
        at: october21 - 86_400)
    try await s.setLink(
        from: try await s.task(main.id), to: try await s.task(storage.id), related: true,
        at: october21 - 259_200)
    try await s.setLink(
        from: try await s.task(main.id), to: try await s.task(done.id), related: true,
        at: october21 - 604_800)

    // Short plans for other cases, so the list and the step page have variety.
    _ = try await s.addStep(
        to: try await s.task(review.id),
        input: MemoryStepInput(
            description: "Прочитать изменения ленты в краевой панели", prompt: nil,
            reference: "https://github.com/owner/ambient-mac/pull/482"))
    _ = try await s.addNote(
        to: try await s.task(reply.id), text: "Ответ ждут до конца недели.", at: october21)

    // A completed case with its summary.
    let doneStep = try await s.addStep(
        to: try await s.task(done.id),
        input: MemoryStepInput(
            description: "Исключить node_modules и .build", prompt: nil, reference: nil))
    try await s.setStepCompleted(try await s.task(done.id), id: doneStep, at: october21 - 600_000)
    _ = try await s.addNote(
        to: try await s.task(done.id), text: "Сканирование пропускает папки сборки.",
        at: october21 - 600_000)
    // Completion keeps the link to the main case and one useful reference.
    try await s.completeTask(
        try await s.task(done.id), summary: "Проверить исключения папок при сканировании Git",
        keep: [main.id], references: ["https://github.com/owner/ambient-mac/pull/455"])

    let open = try await s.tasks().items
    #expect(open.count == 6)
    #expect(try await s.steps(taskID: main.id).items.count == 3)
    #expect(try await s.evidence(taskID: main.id).items.count == 4)
    #expect(try await s.links(taskID: main.id).items.count == 3)
}
