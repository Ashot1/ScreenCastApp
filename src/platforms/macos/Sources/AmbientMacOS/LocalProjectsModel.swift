import AppKit
import CAmbient
import Foundation
import Observation

@MainActor @Observable final class LocalProjectsModel {
    private let memory: MemorySession
    private let reader = LocalGitReader()
    private var operation: LocalSourceOperation?
    private var running: Task<Void, Never>?
    private(set) var generation: UInt64 = 0
    private(set) var busy = false
    private(set) var root: LocalRootSettings?
    private(set) var savedRoots: [LocalRootSettings]?
    private(set) var observation: LocalObservation?
    private(set) var preview: String?
    private(set) var selectedIndex: Int?
    private(set) var failureKey: String?
    private(set) var saved = false
    var exclusionsDraft = ""
    var exclusions: [String] { exclusionsDraft.split(separator: "\n").map(String.init) }
    var scopeSaved: Bool { root.map { $0.revision > 0 && $0.exclusions == exclusions } ?? false }
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
            (error as? LocalSourceFailure)?.key ?? (error as? ModelCheckFailure)?.key
            ?? "error.storage"
    }
    func choose() async {
        guard !busy else { return }
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.resolvesAliases = false
        panel.canCreateDirectories = false
        guard await panel.begin() == .OK, let url = panel.url else { return }
        await select(url)
    }
    func select(_ url: URL) async {
        guard !busy else { return }
        cancel()
        busy = true
        defer { busy = false }
        root = nil
        failureKey = nil
        do {
            let actual = try await reader.select(url)
            let setting = try await memory.localRoot(path: actual)
            root = setting ?? LocalRootSettings(path: actual, revision: 0, exclusions: [])
            exclusionsDraft = setting?.exclusions.joined(separator: "\n") ?? ""
        } catch { fail(error) }
    }
    func listSaved() async {
        do { savedRoots = try await memory.localRoots() } catch { fail(error) }
    }
    func saveScope(chatGPT: ChatGPTModel) async {
        guard let root else { return }
        cancel()
        await chatGPT.resetAssistance()
        do {
            self.root = try await memory.saveLocalRoot(
                path: root.path, expected: root.revision, exclusions: exclusions)
            if savedRoots != nil { savedRoots = try await memory.localRoots() }
            failureKey = nil
        } catch { fail(error) }
    }
    func deleteScope(_ setting: LocalRootSettings, chatGPT: ChatGPTModel) async {
        cancel()
        await chatGPT.resetAssistance()
        do {
            try await memory.deleteLocalRoot(setting)
            if root?.path == setting.path {
                root = LocalRootSettings(path: setting.path, revision: 0, exclusions: [])
                exclusionsDraft = ""
            }
            if savedRoots != nil { savedRoots = try await memory.localRoots() }
            failureKey = nil
        } catch { fail(error) }
    }
    func scan(task: MemoryTask, chatGPT: ChatGPTModel) async {
        guard !busy, scopeSaved, let root else { return }
        cancel()
        busy = true
        saved = false
        failureKey = nil
        let accepted = generation
        let work = Task { [reader] in
            var admitted: LocalSourceOperation?
            do {
                let op = try await chatGPT.beginLocalRead(task, root: root)
                admitted = op
                self.operation = op
                if accepted != self.generation { op.cancel() }
                let result = try await reader.read(op)
                if accepted == self.generation { self.observation = result }
            } catch {
                if accepted == self.generation { self.fail(error) }
            }
            if accepted != self.generation || self.observation == nil {
                if let admitted {
                    do { try await self.memory.releaseLocalRead(admitted) } catch {
                        self.fail(error)
                    }
                }
                self.operation = nil
            }
            self.busy = false
            self.running = nil
        }
        running = work
        await work.value
    }
    func selectBranch(_ branch: LocalBranch) async {
        guard !busy, let operation else { return }
        generation += 1
        let accepted = generation
        do {
            let fetched = try await memory.localPreview(operation, index: branch.index)
            #if AMBIENT_MODEL_TEST_SUPPORT
                testAfterPreview?()
            #endif
            guard accepted == generation, self.operation == operation else { return }
            preview = fetched
            selectedIndex = branch.index
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
            // The credential owner refreshes current native facts; a prior UI availability is not admission.
            _ = try await chatGPT.bindLocalBranch(operation, index: selectedIndex)
            try await memory.releaseLocalRead(operation)
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
        // A cancelled observation stays bounded until next begin or actor-owned disposal.
        observation = nil
        preview = nil
        selectedIndex = nil
        saved = false
    }
    func close() async {
        cancel()
        if let running { await running.value }
        if let operation {
            do { try await memory.releaseLocalRead(operation) } catch { fail(error) }
            self.operation = nil
        }
        await reader.clear()
    }
}
