import AppKit
import Foundation
import Observation

@MainActor @Observable final class CodexSourcesModel {
    private let memory: MemorySession
    private let reader = CodexReader()
    private var operation: CodexOperation?
    private var running: Task<Void, Never>?
    private(set) var generation: UInt64 = 0
    private(set) var selection: CodexSelection?
    private(set) var chosenPath: String?
    private(set) var settings: [CodexSelection]?
    private(set) var observation: CodexObservation?
    private(set) var preview: String?
    private(set) var selectedIndex: Int?
    private(set) var busy = false
    private(set) var saved = false
    private(set) var failureKey: String?
    private(set) var savedMetadata: [CodexSavedMetadata] = []
    var scopeSaved: Bool { selection.map { $0.path == chosenPath } ?? false }
    #if AMBIENT_MODEL_TEST_SUPPORT
        private var testAfterPreview: (@MainActor () -> Void)?
        init(memory: MemorySession, testAfterPreview: @escaping @MainActor () -> Void) {
            self.memory = memory
            self.testAfterPreview = testAfterPreview
        }
    #endif
    init(memory: MemorySession) { self.memory = memory }
    private func fail(_ error: Error) {
        failureKey =
            (error as? CodexSourceFailure)?.key ?? (error as? LocalSourceFailure)?.key
            ?? (error as? ModelCheckFailure)?.key ?? "error.storage"
    }
    func choose() async {
        guard !busy else { return }
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.resolvesAliases = false
        panel.canCreateDirectories = false
        guard await panel.begin() == .OK, let url = panel.url else { return }
        cancel()
        do {
            chosenPath = try await reader.select(url)
            failureKey = nil
        } catch {
            chosenPath = nil
            fail(error)
        }
    }
    func use(_ setting: CodexSelection) {
        guard !busy else { return }
        cancel()
        selection = setting
        chosenPath = setting.path
        failureKey = nil
    }
    func listSaved() async {
        do { settings = try await memory.codexSelections() } catch { fail(error) }
    }
    func loadMetadata(task: MemoryTask) async {
        do { savedMetadata = try await memory.codexMetadata(taskID: task.id) } catch {
            savedMetadata = []
            fail(error)
        }
    }
    func saveScope(chatGPT: ChatGPTModel) async {
        guard !busy, let path = chosenPath else { return }
        cancel()
        await chatGPT.resetAssistance()
        do {
            selection = try await memory.saveCodexSelection(
                id: selection?.id ?? 0, expected: selection?.revision ?? 0, path: path)
            failureKey = nil
            if settings != nil { settings = try await memory.codexSelections() }
        } catch { fail(error) }
    }
    func deleteScope(_ setting: CodexSelection, chatGPT: ChatGPTModel) async {
        cancel()
        await chatGPT.resetAssistance()
        do {
            try await memory.deleteCodexSelection(setting)
            if selection?.id == setting.id {
                selection = nil
                chosenPath = nil
            }
            if settings != nil { settings = try await memory.codexSelections() }
            failureKey = nil
        } catch { fail(error) }
    }
    func read(task: MemoryTask, chatGPT: ChatGPTModel, earlier: Bool = false) async {
        guard !busy, scopeSaved, let selection else { return }
        let cursor = earlier ? observation?.cursor ?? 0 : 0
        guard !earlier || cursor > 0 else { return }
        generation += 1
        let accepted = generation
        busy = true
        failureKey = nil
        saved = false
        preview = nil
        selectedIndex = nil
        observation = nil
        let previous = operation
        let work = Task { [reader] in
            var admitted: CodexOperation?
            do {
                if cursor == 0 {
                    try previous?.release()
                    self.operation = nil
                }
                let op = try await chatGPT.beginCodexRead(
                    task, selection: selection, cursor: cursor)
                admitted = op
                self.operation = op
                if accepted != self.generation { op.cancel() }
                let observed = try await reader.read(op)
                if accepted == self.generation { self.observation = observed }
            } catch { if accepted == self.generation { self.fail(error) } }
            if self.observation == nil || accepted != self.generation {
                do { try (admitted ?? previous)?.release() } catch { self.fail(error) }
                self.operation = nil
            }
            self.busy = false
            self.running = nil
        }
        running = work
        await work.value
    }
    func selectEvent(_ event: CodexEvent) async {
        guard !busy, let operation else { return }
        generation += 1
        let accepted = generation
        do {
            let fetched = try await memory.codexPreview(operation, index: event.id)
            #if AMBIENT_MODEL_TEST_SUPPORT
                testAfterPreview?()
            #endif
            guard accepted == generation, self.operation == operation else { return }
            preview = fetched
            selectedIndex = event.id
            failureKey = nil
        } catch {
            guard accepted == generation, self.operation == operation else { return }
            preview = nil
            selectedIndex = nil
            fail(error)
        }
    }
    func bind(chatGPT: ChatGPTModel) async -> Bool {
        guard !busy, preview != nil, let selectedIndex, let operation else { return false }
        do {
            _ = try await chatGPT.bindCodexEvent(operation, index: selectedIndex)
            try operation.release()
            self.operation = nil
            observation = nil
            preview = nil
            self.selectedIndex = nil
            saved = true
            return true
        } catch {
            fail(error)
            return false
        }
    }
    func cancel() {
        generation += 1
        operation?.cancel()
        observation = nil
        preview = nil
        selectedIndex = nil
        saved = false
    }
    func close() async {
        cancel()
        if let running { await running.value }
        if let operation {
            do { try operation.release() } catch { fail(error) }
            self.operation = nil
        }
        await reader.clear()
    }
}
