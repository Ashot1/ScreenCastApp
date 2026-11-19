import CAmbient
import Foundation

enum ModelProfile: UInt32, CaseIterable, Sendable {
    case low = 1
    case high
    var effort: String { self == .high ? "high" : "low" }
}
enum ModelPurpose: UInt32, Sendable {
    case connectivity = 1
    case assistance, observation
    var key: String? {
        switch self {
        case .connectivity: "check.purpose.connectivity"
        case .assistance: "check.purpose.assistance"
        case .observation: nil
        }
    }
}
enum ModelEffort: UInt32, Sendable {
    case missing = 0
    case low, other, high
}
enum AssistanceMode: UInt32, Sendable {
    case `continue` = 1
    case newContext
}
struct AssistancePreview: Equatable, Sendable {
    let requestID: UInt64
    let taskID: UInt64
    let revision: UInt64
    let workspaceRevision: UInt64
    let mode: AssistanceMode
    let context: String
}
struct AssistanceTaskVersion: Equatable, Sendable, Decodable {
    let taskID: UInt64
    let revision: UInt64
    enum CodingKeys: String, CodingKey {
        case taskID = "task_id"
        case revision
    }
}
struct ProposalTarget: Equatable, Sendable {
    let id: UInt64
    let contentRevision: UInt64
    var native: AmbientProposalTarget {
        AmbientProposalTarget(proposal_id: id, content_revision: contentRevision)
    }
}
enum AssistanceKind: String, Sendable, Decodable {
    case suggestion, clarification
    var key: String { self == .suggestion ? "assistance.suggestion" : "assistance.clarification" }
}
struct ProposalCitationReference: Equatable, Sendable, Decodable {
    let kind: String
    let id: String
    enum CodingKeys: String, CodingKey { case kind, id }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(String.self, forKey: .kind)
        if kind == "reply" {
            id = try c.decode(String.self, forKey: .id)
        } else {
            let number = try c.decode(UInt64.self, forKey: .id)
            guard number > 0, number <= UInt64(Int64.max),
                ["task", "note", "source", "step", "check"].contains(kind)
            else { throw ModelCheckFailure.native }
            id = "\(kind):\(number)"
        }
        guard !id.isEmpty, id.utf8.count <= 64 else { throw ModelCheckFailure.native }
    }
    var key: String { "assistance.ground.\(kind)" }
}
struct Proposal: Equatable, Sendable, Decodable, Identifiable {
    let id: UInt64
    let task: AssistanceTaskVersion
    let workspaceRevision: UInt64
    let contentRevision: UInt64
    let kind: AssistanceKind
    let message: String
    let rationale: String
    let citations: [ProposalCitationReference]
    let stepID: UInt64?
    let visible: Bool
    let stale: Bool
    var target: ProposalTarget { ProposalTarget(id: id, contentRevision: contentRevision) }
    enum CodingKeys: String, CodingKey {
        case id, task, kind, message, rationale, citations, visible, stale
        case workspaceRevision = "workspace_revision"
        case contentRevision = "content_revision"
        case stepID = "step_id"
    }
}
extension [Proposal] {
    /// The one expanded feed row (ADR-023 #20): the reply target, otherwise the row the window's
    /// user clicked, otherwise the first fresh row. Each window passes its own rows and click.
    func expandedID(reply: ProposalTarget?, clicked: UInt64?) -> UInt64? {
        first { $0.target == reply }?.id ?? first { $0.id == clicked }?.id
            ?? first { !$0.stale }?.id
    }
}
struct AssistanceCitation: Equatable, Sendable, Decodable, Identifiable {
    let reference: ProposalCitationReference
    let text: String
    let modelQuestion: String?
    let integration: IntegrationCitation?
    let plugin: PluginCitation?
    let sourceObservedAtUTC: Int64?
    var id: String { reference.id }
    var key: String { reference.key }
    enum CodingKeys: String, CodingKey {
        case reference, text, integration, plugin
        case modelQuestion = "model_question"
        case sourceObservedAtUTC = "source_observed_at_utc"
    }
}
/// Provenance of a saved plugin tool result: plugin, server, tool and call time.
struct PluginCitation: Equatable, Sendable, Decodable {
    let plugin: String
    let server: String
    let tool: String
    let calledAtUTC: Int64
    let complete: Bool
    enum CodingKeys: String, CodingKey {
        case plugin, server, tool, complete
        case calledAtUTC = "called_at_utc"
    }
}
/// Integration detail of a cited saved fact; the metadata is decoded by its explicit kind.
enum IntegrationCitation: Equatable, Sendable, Decodable {
    case github(GitHubMetadata)
    case jira(JiraMetadata)
    case todoist(TodoistMetadata)
    private enum CodingKeys: String, CodingKey { case kind, metadata }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(IntegrationKind.self, forKey: .kind) {
        case .github: self = .github(try c.decode(GitHubMetadata.self, forKey: .metadata))
        case .jira: self = .jira(try c.decode(JiraMetadata.self, forKey: .metadata))
        case .todoist: self = .todoist(try c.decode(TodoistMetadata.self, forKey: .metadata))
        }
    }
}
struct ProposalDetails: Equatable, Sendable, Decodable {
    let proposal: Proposal
    let citations: [AssistanceCitation]
}
struct ProposalFeed: Equatable, Sendable, Decodable { let proposals: [Proposal] }
struct ModelOperationResult: Equatable, Sendable {
    let attempt: ModelCheckAttempt
    let publishedTaskID: UInt64?
    let workspaceRevision: UInt64?
}
extension ModelCheckBridge {
    nonisolated static func text(_ span: AmbientModelSpan, _ bytes: [UInt8], used: Int) throws
        -> String
    {
        let start = Int(span.offset)
        let end = start + Int(span.length)
        guard start <= end, end <= used, used <= bytes.count,
            let value = String(bytes: bytes[start..<end], encoding: .utf8)
        else { throw ModelCheckFailure.native }
        return value
    }
    nonisolated static func decodeCompletion(_ r: AmbientModelCompletion) throws
        -> ModelOperationResult
    {
        let attempt = try decode(r.attempt)
        let published = r.published_task != 0
        guard published == (r.workspace_revision != 0), r.published_task <= UInt64(Int64.max),
            r.workspace_revision <= UInt64(Int64.max),
            !published
                || (attempt.purpose == .assistance && attempt.profile == .high
                    && attempt.outcome == .confirmed),
            attempt.purpose != .assistance || attempt.outcome != .confirmed || published
        else { throw ModelCheckFailure.native }
        return ModelOperationResult(
            attempt: attempt, publishedTaskID: published ? r.published_task : nil,
            workspaceRevision: published ? r.workspace_revision : nil)
    }
    nonisolated static func decodeProjection<T: Decodable>(
        _ type: T.Type, bytes: [UInt8], length: UInt32, limit: Int
    ) throws -> T {
        guard Int(length) <= limit, Int(length) <= bytes.count else {
            throw ModelCheckFailure.native
        }
        do { return try JSONDecoder().decode(type, from: Data(bytes.prefix(Int(length)))) } catch {
            throw ModelCheckFailure.native
        }
    }
    nonisolated static func validate(_ p: Proposal) throws {
        guard
            [p.id, p.task.taskID, p.task.revision, p.workspaceRevision, p.contentRevision]
                .allSatisfy({ $0 > 0 && $0 <= UInt64(Int64.max) }),
            !p.message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            p.message.utf8.count <= 16384,
            !p.rationale.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            p.rationale.utf8.count <= 8192,
            p.citations.count <= 32, Set(p.citations.map(\.id)).count == p.citations.count,
            p.stepID.map({ $0 > 0 && $0 <= UInt64(Int64.max) }) ?? true
        else { throw ModelCheckFailure.native }
    }
}
