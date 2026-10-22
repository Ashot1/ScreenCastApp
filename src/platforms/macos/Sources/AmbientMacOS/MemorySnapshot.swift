import Foundation

public struct MemoryTask: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let revision: UInt64
    public let text: String
    public let lifecycle: MemoryTaskLifecycle
    public let initiativeSuppressed: Bool
    public let planCompleteness: MemoryCompleteness
}
public struct MemoryPage<Item: Equatable & Sendable>: Equatable, Sendable {
    public let items: [Item]
    public let nextAfter: UInt64?
    public var taskRevision: UInt64? = nil
    public var planCompleteness: MemoryCompleteness? = nil
    static var empty: Self { Self(items: [], nextAfter: nil) }
    /// The number of items, or nil unless the page starts the list and nothing follows it.
    func wholeCount(fromStart: Bool) -> Int? { fromStart && nextAfter == nil ? items.count : nil }
}
public enum MemoryCompleteness: UInt32, Sendable { case unknown, partial, complete }
public enum MemoryAssertionKind: UInt32, Sendable { case sourceFact, assumption, userConfirmation }
public struct MemorySourceKey: Equatable, Sendable {
    public let namespace: String
    public let scope: String
    public let objectID: String
}
public enum MemoryOrigin: Equatable, Sendable {
    case user
    case external(key: MemorySourceKey, version: String?)
}
public struct MemorySourceEvidence: Equatable, Sendable {
    public let text: String
    public let observedAtUTC: Int64?
    public let completeness: MemoryCompleteness
    public let kind: MemoryAssertionKind
}
public struct MemoryEvidence: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let taskID: UInt64
    public let text: String
    public let origin: MemoryOrigin
    public let observedAtUTC: Int64?
    public let completeness: MemoryCompleteness
    public let kind: MemoryAssertionKind
}
public struct MemorySourceBinding: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let taskID: UInt64
    public let evidenceID: UInt64
    public let key: MemorySourceKey
    public let version: String?
}
public struct MemoryLink: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let otherTask: MemoryTask
    public var otherTaskID: UInt64 { otherTask.id }
    public let related: Bool
    public let actionAtUTC: Int64
}
public struct MemoryBindingDecision: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let evidenceID: UInt64
    public let accepted: Bool
    public let actionAtUTC: Int64
}
public enum MemoryIngestOutcome: Equatable, Sendable {
    case applied(UInt64)
    case forgotten
    case userProtected
}
public struct MemorySnapshot: Equatable, Sendable {
    public let tasks: MemoryPage<MemoryTask>
    public let selectedTask: MemoryTask?
    public let evidence: MemoryPage<MemoryEvidence>
    public let links: MemoryPage<MemoryLink>
    public let decisions: MemoryPage<MemoryBindingDecision>
    public let steps: MemoryPage<MemoryStep>
    public let selectedStep: MemoryStep?
    public let checks: MemoryPage<MemoryCheck>
    public let dependencies: MemoryPage<MemoryDependency>
    public let references: MemoryPage<MemoryReference>
    /// Groups only the loaded task page: Core pages by ID across both lifecycles.
    var openTasks: [MemoryTask] { tasks.items.filter { $0.lifecycle == .open } }
    var completedTasks: [MemoryTask] { tasks.items.filter { $0.lifecycle == .completed } }
    /// The first ready, unfinished step of the loaded page. Readiness comes only from Core, and
    /// later pages are not read for it (ADR-023 #19).
    var nextStep: MemoryStep? { steps.items.first { $0.ready && $0.completedAtUTC == nil } }
    /// Unfinished predecessors a waiting step depends on, or nil when that is unknown: every
    /// dependency (from the first page) and every predecessor step must be loaded.
    func waitingSteps(for step: MemoryStep, dependenciesFromStart: Bool) -> [UInt64]? {
        guard !step.ready, step.completedAtUTC == nil, dependenciesFromStart,
            dependencies.nextAfter == nil
        else { return nil }
        var waiting: [UInt64] = []
        for dependency in dependencies.items where dependency.dependent == step.id {
            guard let predecessor = steps.items.first(where: { $0.id == dependency.predecessor })
            else { return nil }
            if predecessor.completedAtUTC == nil { waiting.append(predecessor.id) }
        }
        return waiting
    }
    /// Step numbers shown to the user: 1-based positions by step ID, or nil unless the whole plan
    /// is loaded from its first page (ADR-024, 26C).
    func stepPositions(fromStart: Bool) -> [UInt64: Int]? {
        guard steps.wholeCount(fromStart: fromStart) != nil else { return nil }
        return Dictionary(
            uniqueKeysWithValues: steps.items.enumerated().map { ($1.id, $0 + 1) })
    }
    /// Unfinished steps that depend on an unfinished step, or nil when that is unknown: every
    /// dependency (from the first page) and every dependent step must be loaded.
    func waitingDependents(of step: MemoryStep, dependenciesFromStart: Bool) -> [UInt64]? {
        guard step.completedAtUTC == nil, dependenciesFromStart, dependencies.nextAfter == nil
        else { return nil }
        var waiting: [UInt64] = []
        for dependency in dependencies.items where dependency.predecessor == step.id {
            guard let dependent = steps.items.first(where: { $0.id == dependency.dependent })
            else { return nil }
            if dependent.completedAtUTC == nil { waiting.append(dependent.id) }
        }
        return waiting
    }
}
public enum MemoryFailure: Error, Equatable, Sendable {
    case opening, storage, schema, invalidInput, native, closed, stale, deleted, limit, completed,
        clipboard
    var message: String {
        switch self {
        case .completed: return String(localized: "error.completed", bundle: .module)
        case .clipboard: return String(localized: "error.clipboard", bundle: .module)
        case .opening: return String(localized: "error.open", bundle: .module)
        case .storage: return String(localized: "error.storage", bundle: .module)
        case .schema: return String(localized: "error.schema", bundle: .module)
        case .invalidInput: return String(localized: "error.input", bundle: .module)
        case .stale: return String(localized: "error.stale", bundle: .module)
        case .deleted: return String(localized: "error.deleted", bundle: .module)
        case .limit: return String(localized: "error.limit", bundle: .module)
        case .native, .closed: return String(localized: "error.bridge", bundle: .module)
        }
    }
}

public enum MemoryTaskLifecycle: UInt32, Sendable { case open, completed }
public enum MemoryCheckResult: UInt32, Sendable { case unknown, passed, failed }
public struct MemoryStepInput: Equatable, Sendable {
    public var description: String
    public var prompt: String?
    public var reference: String?
}
public struct MemoryStep: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let taskID: UInt64
    public let description: String
    public let prompt: String?
    public let reference: String?
    public let completedAtUTC: Int64?
    public let ready: Bool
}
public struct MemoryCheck: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let taskID: UInt64
    public let stepID: UInt64
    public let description: String
    public let result: MemoryCheckResult
    public let confirmedAtUTC: Int64?
}
public struct MemoryDependency: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let taskID: UInt64
    public let predecessor: UInt64
    public let dependent: UInt64
}
public struct MemoryReference: Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let taskID: UInt64
    public let text: String
}

extension MemoryEvidence {
    var assistanceSelectable: Bool {
        switch origin {
        case .user: true
        case .external(let key, _):
            key.namespace == "local-git-branch-v1" || key.namespace == "codex-local-jsonl-v1"
                || IntegrationKind(namespace: key.namespace) != nil
        }
    }
}
