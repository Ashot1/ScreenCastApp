import Foundation
import Observation

/// What the shared source skeleton needs from one adapter. Parsing types, metadata fields and
/// special settings (Cloud ID, email, allowlist) stay with the adapter.
@MainActor protocol IntegrationAdapter {
    associatedtype Object: Decodable & Equatable & Sendable & Identifiable
    associatedtype Coverage: Decodable & Equatable & Sendable & Identifiable
    associatedtype Metadata: Decodable & Equatable & Sendable
    /// Editable settings and secret fields; secret fields are never prefilled.
    associatedtype Draft: Equatable
    static var kind: IntegrationKind { get }
    static func draft(_ profile: IntegrationProfile?) -> Draft
    static func save(_ draft: Draft, over profile: IntegrationProfile?, memory: MemorySession)
        async throws -> IntegrationProfile
    static func secret(_ draft: Draft) -> IntegrationSecret
    static func clearingSecret(_ draft: Draft) -> Draft
    static func bindable(_ object: Object) -> Bool
    static func failureKey(_ failure: IntegrationFailure) -> String
}

@MainActor @Observable final class IntegrationSourcesModel<Adapter: IntegrationAdapter> {
    private let memory: MemorySession
    private let secrets: IntegrationSecretOwner
    private var operation: IntegrationOperation?
    private var running: Task<Void, Never>?
    // Every user action advances this; late responses for an older generation are dropped.
    private var generation: UInt64 = 0
    private(set) var profiles: [IntegrationProfile] = []
    private(set) var profile: IntegrationProfile?
    private(set) var exclusions: [String] = []
    var draft: Adapter.Draft
    var target = ""
    var scopeExcluded: Bool { profile?.scope.map { exclusions.contains($0) } ?? false }
    private(set) var secretAvailable: Bool?
    private(set) var window: IntegrationWindow<Adapter.Object, Adapter.Coverage>?
    private(set) var preview: IntegrationPreview<Adapter.Object>?
    private(set) var selected: Set<Int> = []
    private(set) var busy = false
    private(set) var changingCredential = false
    private(set) var preparingPreview = false
    private(set) var saved = false
    private(set) var failureKey: String?
    private var metadataTaskID: UInt64?
    private(set) var metadataNext: Int64?
    private(set) var savedMetadata: [IntegrationSavedMetadata<Adapter.Metadata>] = []
    init(memory: MemorySession, secrets: IntegrationSecretOwner? = nil) {
        self.memory = memory
        self.secrets = secrets ?? IntegrationSecretOwner(memory: memory)
        draft = Adapter.draft(nil)
    }
    private func fail(_ e: Error) {
        failureKey =
            (e as? IntegrationFailure).map { Adapter.failureKey($0) }
            ?? (e as? LocalSourceFailure)?.key
            ?? (e as? ModelCheckFailure)?.key ?? "error.storage"
    }
    func loadProfiles() async {
        do {
            let accepted = generation
            let settings = try await memory.integrationSettings(Adapter.kind)
            guard accepted == generation else { return }
            profiles = settings.profiles
            exclusions = settings.exclusions
            if let id = profile?.id { profile = profiles.first { $0.id == id } }
            if let profile {
                let available = try await secrets.hasCredential(profile)
                guard accepted == generation, self.profile == profile else { return }
                secretAvailable = available
            } else {
                secretAvailable = nil
            }
        } catch {
            secretAvailable = nil
            fail(error)
        }
    }
    func use(_ p: IntegrationProfile?) async {
        guard !changingCredential else { return }
        cancel()
        profile = p
        draft = Adapter.draft(p)
        failureKey = nil
        await loadProfiles()
    }
    /// Settings and secret changes cancel the read, reset assistance and reload afterwards.
    private func mutate(chatGPT: ChatGPTModel, _ change: () async throws -> Void) async {
        changingCredential = true
        defer { changingCredential = false }
        await chatGPT.resetAssistance()
        do {
            try await change()
            failureKey = nil
        } catch { fail(error) }
        await loadProfiles()
        await chatGPT.refreshFeed()
    }
    func saveProfile(chatGPT: ChatGPTModel) async {
        guard !changingCredential else { return }
        cancel()
        await mutate(chatGPT: chatGPT) {
            profile = try await Adapter.save(draft, over: profile, memory: memory)
        }
    }
    func replaceSecret(chatGPT: ChatGPTModel) async {
        guard !changingCredential, let profile else { return }
        cancel()
        let secret = Adapter.secret(draft)
        draft = Adapter.clearingSecret(draft)
        await mutate(chatGPT: chatGPT) {
            self.profile = try await secrets.replace(profile, secret: secret)
        }
    }
    func deleteProfile(chatGPT: ChatGPTModel) async {
        guard !changingCredential, let profile else { return }
        cancel()
        draft = Adapter.clearingSecret(draft)
        await mutate(chatGPT: chatGPT) {
            try await secrets.delete(profile)
            self.profile = nil
            draft = Adapter.draft(nil)
        }
    }
    func setScopeExcluded(_ scope: String, excluded: Bool, chatGPT: ChatGPTModel) async {
        guard !changingCredential else { return }
        cancel()
        await mutate(chatGPT: chatGPT) {
            try await memory.setIntegrationScopeExcluded(
                Adapter.kind, scope: scope, excluded: excluded)
        }
    }
    func read(task: MemoryTask, chatGPT: ChatGPTModel) async {
        guard !busy, !changingCredential, !scopeExcluded, let profile else { return }
        cancel()
        let accepted = generation
        busy = true
        failureKey = nil
        let prior = operation
        let target = target
        let work = Task {
            var admitted: IntegrationOperation?
            do {
                try prior?.release()
                operation = nil
                let op = try await chatGPT.beginIntegrationRead(
                    task, profile: profile, target: target)
                admitted = op
                operation = op
                if generation != accepted { op.cancel() }
                let result: IntegrationWindow<Adapter.Object, Adapter.Coverage> =
                    try await secrets.read(op)
                if generation == accepted { window = result }
            } catch { if generation == accepted { fail(error) } }
            if window == nil || generation != accepted {
                do { try admitted?.release() } catch { fail(error) }
                operation = nil
            }
            busy = false
            running = nil
            await loadProfiles()
            await chatGPT.refreshFeed()
        }
        running = work
        await work.value
    }
    func toggle(_ index: Int) {
        guard !busy, let window, window.objects.indices.contains(index),
            Adapter.bindable(window.objects[index])
        else { return }
        if selected.contains(index) {
            selected.remove(index)
        } else if selected.count < 32 {
            selected.insert(index)
        }
        generation += 1
        preview = nil
    }
    func makePreview() async {
        guard !busy, !preparingPreview, let operation, !selected.isEmpty else { return }
        preparingPreview = true
        defer { preparingPreview = false }
        let accepted = generation
        let indices = selected.sorted()
        do {
            let result: IntegrationPreview<Adapter.Object> = try await memory.integrationPreview(
                operation, indices: indices)
            guard accepted == generation, self.operation == operation else { return }
            preview = result
            failureKey = nil
        } catch {
            if accepted == generation {
                preview = nil
                fail(error)
            }
        }
    }
    func bind(chatGPT: ChatGPTModel) async -> Bool {
        guard !busy, !changingCredential, !preparingPreview, preview != nil, let operation else {
            return false
        }
        do {
            _ = try await chatGPT.bindIntegrationObjects(operation, indices: selected.sorted())
            try operation.release()
            self.operation = nil
            window = nil
            preview = nil
            selected = []
            saved = true
            await chatGPT.refreshFeed()
            return true
        } catch {
            fail(error)
            return false
        }
    }
    func loadMetadata(task: MemoryTask, after: Int64 = 0) async {
        metadataTaskID = task.id
        do {
            let page: IntegrationSavedPage<Adapter.Metadata> =
                try await memory
                .integrationSavedMetadata(Adapter.kind, taskID: task.id, after: after)
            guard metadataTaskID == task.id, !Task.isCancelled else { return }
            savedMetadata = page.rows
            metadataNext = page.nextAfter
        } catch {
            if metadataTaskID == task.id {
                savedMetadata = []
                metadataNext = nil
                fail(error)
            }
        }
    }
    func cancel() {
        generation += 1
        operation?.cancel()
        window = nil
        preview = nil
        selected = []
        saved = false
    }
    func close() async {
        cancel()
        if let running { await running.value }
        if let operation {
            do { try operation.release() } catch { fail(error) }
            self.operation = nil
        }
        draft = Adapter.clearingSecret(draft)
    }
}
