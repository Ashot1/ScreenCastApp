import Foundation

typealias TodoistSourcesModel = IntegrationSourcesModel<TodoistAdapter>
struct TodoistDraft: Equatable {
    var name = ""
    var token = ""
}
/// Todoist parts of the shared source skeleton: settings, secret form and error copy. The
/// account is never entered: Core writes it into the profile once, on the first read.
enum TodoistAdapter: IntegrationAdapter {
    typealias Object = TodoistObject
    typealias Coverage = TodoistCoverage
    typealias Metadata = TodoistMetadata
    typealias Draft = TodoistDraft
    static let kind = IntegrationKind.todoist
    static func draft(_ profile: IntegrationProfile?) -> TodoistDraft {
        TodoistDraft(name: profile?.name ?? "")
    }
    static func save(
        _ draft: TodoistDraft, over profile: IntegrationProfile?, memory: MemorySession
    ) async throws -> IntegrationProfile {
        try await memory.saveTodoistProfile(
            id: profile?.id ?? 0, expected: profile?.settingsRevision ?? 0, name: draft.name)
    }
    static func secret(_ draft: TodoistDraft) -> IntegrationSecret {
        .todoistToken(token: draft.token)
    }
    static func clearingSecret(_ draft: TodoistDraft) -> TodoistDraft {
        TodoistDraft(name: draft.name)
    }
    static func bindable(_ object: TodoistObject) -> Bool { object.bindable }
    static func failureKey(_ failure: IntegrationFailure) -> String {
        switch failure {
        case .invalid: "todoist.error.invalid"
        case .storage: "todoist.error.storage"
        case .deleteIncomplete: "todoist.deleteIncomplete"
        case .response(let status):
            switch status {
            case 401: "todoist.error.unauthorized"
            case 403: "todoist.error.forbidden"
            case 429, 503: "todoist.error.rate"
            case 404: "todoist.error.unavailable"
            case 409: "todoist.error.changed"
            case 408, 413: "integration.error.limit"
            case 499: "todoist.error.cancelled"
            default: "todoist.error.transport"
            }
        }
    }
}
enum TodoistPresence<T: Decodable & Equatable & Sendable>: Decodable, Equatable, Sendable {
    case missing, null, invalid
    case value(T)
    private enum Keys: String, CodingKey { case presence, value }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .presence) {
        case "missing": self = .missing
        case "null": self = .null
        case "invalid": self = .invalid
        case "value": self = .value(try c.decode(T.self, forKey: .value))
        default: throw IntegrationFailure.invalid
        }
    }
    func flatMap<U>(_ field: (T) -> TodoistPresence<U>) -> TodoistPresence<U> {
        switch self {
        case .missing: .missing
        case .null: .null
        case .invalid: .invalid
        case .value(let value): field(value)
        }
    }
}
struct TodoistTime: Decodable, Equatable, Sendable {
    let utcSeconds: Int64
    let precisionLost: Bool
    enum CodingKeys: String, CodingKey {
        case utcSeconds = "utc_seconds"
        case precisionLost = "precision_lost"
    }
}
/// Observed due subfields; the date stays the literal text Todoist sent.
struct TodoistDue: Decodable, Equatable, Sendable {
    let date: TodoistPresence<String>
    let timezone: TodoistPresence<String>
    let isRecurring: TodoistPresence<Bool>
    let string: TodoistPresence<String>
    let lang: TodoistPresence<String>
    enum CodingKeys: String, CodingKey {
        case date, timezone, string, lang
        case isRecurring = "is_recurring"
    }
}
struct TodoistDeadline: Decodable, Equatable, Sendable {
    let date: TodoistPresence<String>
    let lang: TodoistPresence<String>
}
struct TodoistDuration: Decodable, Equatable, Sendable {
    let amount: TodoistPresence<Int64>
    let unit: TodoistPresence<String>
}
/// Fields of a task, project or comment, keyed by `kind`; a field absent from a kind is nil.
struct TodoistFields: Decodable, Equatable, Sendable {
    let kind: String
    let userID: TodoistPresence<String>?
    let projectID: String?
    let content: TodoistPresence<String>?
    let description: TodoistPresence<String>?
    let parentID: TodoistPresence<String>?
    let sectionID: TodoistPresence<String>?
    let addedByUID: TodoistPresence<String>?
    let assignedByUID: TodoistPresence<String>?
    let responsibleUID: TodoistPresence<String>?
    let completedByUID: TodoistPresence<String>?
    let checked: TodoistPresence<Bool>?
    let isDeleted: TodoistPresence<Bool>?
    let addedAt: TodoistPresence<TodoistTime>?
    let updatedAt: TodoistPresence<TodoistTime>?
    let completedAt: TodoistPresence<TodoistTime>?
    let due: TodoistPresence<TodoistDue>?
    let deadline: TodoistPresence<TodoistDeadline>?
    let duration: TodoistPresence<TodoistDuration>?
    let name: TodoistPresence<String>?
    let workspaceID: TodoistPresence<String>?
    let isArchived: TodoistPresence<Bool>?
    let createdAt: TodoistPresence<TodoistTime>?
    let postedUID: TodoistPresence<String>?
    let postedAt: TodoistPresence<TodoistTime>?
    let fileAttachment: Bool?
    enum CodingKeys: String, CodingKey {
        case kind, content, description, checked, due, deadline, duration, name
        case userID = "user_id"
        case projectID = "project_id"
        case parentID = "parent_id"
        case sectionID = "section_id"
        case addedByUID = "added_by_uid"
        case assignedByUID = "assigned_by_uid"
        case responsibleUID = "responsible_uid"
        case completedByUID = "completed_by_uid"
        case isDeleted = "is_deleted"
        case addedAt = "added_at"
        case updatedAt = "updated_at"
        case completedAt = "completed_at"
        case workspaceID = "workspace_id"
        case isArchived = "is_archived"
        case createdAt = "created_at"
        case postedUID = "posted_uid"
        case postedAt = "posted_at"
        case fileAttachment = "file_attachment"
    }
}
struct TodoistContent: Decodable, Equatable, Sendable {
    let parentTaskID: String
    let objectID: String
    let fields: TodoistFields
    let losses: [String]
    enum CodingKeys: String, CodingKey {
        case fields, losses
        case parentTaskID = "parent_task_id"
        case objectID = "object_id"
    }
}
struct TodoistMetadata: Decodable, Equatable, Sendable {
    let profileID: UInt64
    let settingsRevision: UInt64
    let credentialGeneration: UInt64
    let account: String
    let intervalStart: Int64
    let intervalEnd: Int64
    let coverage: [TodoistCoverage]
    let observation: String
    let currentState: String
    let content: TodoistContent
    enum CodingKeys: String, CodingKey {
        case account, coverage, observation, content
        case profileID = "profile_id"
        case settingsRevision = "settings_revision"
        case credentialGeneration = "credential_generation"
        case intervalStart = "interval_start"
        case intervalEnd = "interval_end"
        case currentState = "current_state"
    }
}
struct TodoistObject: Decodable, Equatable, Sendable, Identifiable {
    struct Key: Decodable, Equatable, Sendable {
        let namespace: String
        let scope: String
        let objectID: String
        enum CodingKeys: String, CodingKey {
            case namespace, scope
            case objectID = "object_id"
        }
    }
    let key: Key
    let version: String
    let text: String
    let observedAtUTC: Int64?
    let metadata: TodoistMetadata
    let bindable: Bool
    var id: String { key.objectID }
    enum CodingKeys: String, CodingKey {
        case key, version, text, metadata, bindable
        case observedAtUTC = "observed_at_utc"
    }
}
struct TodoistCoverage: Decodable, Equatable, Sendable, Identifiable {
    let category: Int
    let status: String
    let pages: Int
    let objects: Int
    let unavailable: Int
    let reasons: [String]
    var id: Int { category }
    var kind: String { [0: "task", 1: "project", 2: "comments"][category] ?? "task" }
}
