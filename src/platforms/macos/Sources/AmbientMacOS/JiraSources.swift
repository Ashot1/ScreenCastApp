import Foundation

typealias JiraSourcesModel = IntegrationSourcesModel<JiraAdapter>
struct JiraDraft: Equatable {
    var name = ""
    var cloudID = ""
    var email = ""
    var token = ""
}
/// Jira Cloud parts of the shared source skeleton: settings, secret form and error copy.
enum JiraAdapter: IntegrationAdapter {
    typealias Object = JiraObject
    typealias Coverage = JiraCoverage
    typealias Metadata = JiraMetadata
    typealias Draft = JiraDraft
    static let kind = IntegrationKind.jira
    static func draft(_ profile: IntegrationProfile?) -> JiraDraft {
        JiraDraft(name: profile?.name ?? "", cloudID: profile?.scope ?? "")
    }
    static func save(_ draft: JiraDraft, over profile: IntegrationProfile?, memory: MemorySession)
        async throws -> IntegrationProfile
    {
        try await memory.saveJiraProfile(
            id: profile?.id ?? 0, expected: profile?.settingsRevision ?? 0, name: draft.name,
            cloudID: draft.cloudID)
    }
    static func secret(_ draft: JiraDraft) -> IntegrationSecret {
        .jiraBasic(email: draft.email, token: draft.token)
    }
    static func clearingSecret(_ draft: JiraDraft) -> JiraDraft {
        JiraDraft(name: draft.name, cloudID: draft.cloudID)
    }
    static func bindable(_ object: JiraObject) -> Bool { object.bindable }
    static func failureKey(_ failure: IntegrationFailure) -> String {
        switch failure {
        case .invalid: "jira.error.invalid"
        case .storage: "jira.error.storage"
        case .deleteIncomplete: "jira.deleteIncomplete"
        case .response(let status):
            switch status {
            case 401: "jira.error.unauthorized"
            case 403: "jira.error.forbidden"
            case 429, 503: "jira.error.rate"
            case 404: "jira.error.unavailable"
            case 409: "jira.error.changed"
            case 408, 413: "integration.error.limit"
            case 499: "jira.error.cancelled"
            default: "jira.error.transport"
            }
        }
    }
}
enum JiraPresence<T: Decodable & Equatable & Sendable>: Decodable, Equatable, Sendable {
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
    var value: T? { if case .value(let value) = self { value } else { nil } }
    func flatMap<U>(_ field: (T) -> JiraPresence<U>) -> JiraPresence<U> {
        switch self {
        case .missing: .missing
        case .null: .null
        case .invalid: .invalid
        case .value(let value): field(value)
        }
    }
}
struct JiraDate: Decodable, Equatable, Sendable {
    let utcSeconds: Int64
    let precisionLost: Bool
    enum CodingKeys: String, CodingKey {
        case utcSeconds = "utc_seconds"
        case precisionLost = "precision_lost"
    }
}
struct JiraNamed: Decodable, Equatable, Sendable {
    let id: JiraPresence<String>
    let name: JiraPresence<String>
    let key: JiraPresence<String>
}
struct JiraAuthor: Decodable, Equatable, Sendable {
    let accountID: JiraPresence<String>
    let displayName: JiraPresence<String>
    enum CodingKeys: String, CodingKey {
        case accountID = "account_id"
        case displayName = "display_name"
    }
}
struct JiraBody: Decodable, Equatable, Sendable {
    let text: JiraPresence<String>
    let losses: [String]
}
struct JiraChange: Decodable, Equatable, Sendable {
    let field: JiraPresence<String>
    let fieldID: JiraPresence<String>
    let fieldType: JiraPresence<String>
    let from: JiraPresence<String>
    let to: JiraPresence<String>
    let fromString: JiraPresence<String>
    let toString: JiraPresence<String>
    enum CodingKeys: String, CodingKey {
        case field, from, to
        case fieldID = "field_id"
        case fieldType = "field_type"
        case fromString = "from_string"
        case toString = "to_string"
    }
}
struct JiraFields: Decodable, Equatable, Sendable {
    let kind: String
    let summary: JiraPresence<String>?
    let description: JiraBody?
    let status: JiraPresence<JiraNamed>?
    let issueType: JiraPresence<JiraNamed>?
    let project: JiraPresence<JiraNamed>?
    let created: JiraPresence<JiraDate>?
    let updated: JiraPresence<JiraDate>?
    let body: JiraBody?
    let author: JiraPresence<JiraAuthor>?
    let items: [JiraChange]?
    let targetID: String?
    let targetKey: JiraPresence<String>?
    let direction: String?
    let label: String?
    let linkType: JiraPresence<JiraNamed>?
    let inward: JiraPresence<String>?
    let outward: JiraPresence<String>?
    enum CodingKeys: String, CodingKey {
        case kind, summary, description, status, project, created, updated, body, author, items,
            direction, label, inward, outward
        case issueType = "issue_type"
        case targetID = "target_id"
        case targetKey = "target_key"
        case linkType = "link_type"
    }
}
struct JiraContent: Decodable, Equatable, Sendable {
    let tag: String
    let parentIssueID: String
    let objectID: String
    let observedIssueKey: String
    let fields: JiraFields
    let losses: [String]
    enum CodingKeys: String, CodingKey {
        case tag, fields, losses
        case parentIssueID = "parent_issue_id"
        case objectID = "object_id"
        case observedIssueKey = "observed_issue_key"
    }
}
struct JiraMetadata: Decodable, Equatable, Sendable {
    let profileID: UInt64
    let settingsRevision: UInt64
    let credentialGeneration: UInt64
    let cloudID: String
    let intervalStart: Int64
    let intervalEnd: Int64
    let coverage: [JiraCoverage]
    let observation: String
    let currentState: String
    let content: JiraContent
    enum CodingKeys: String, CodingKey {
        case coverage, observation, content
        case profileID = "profile_id"
        case settingsRevision = "settings_revision"
        case credentialGeneration = "credential_generation"
        case cloudID = "cloud_id"
        case intervalStart = "interval_start"
        case intervalEnd = "interval_end"
        case currentState = "current_state"
    }
}
struct JiraObject: Decodable, Equatable, Sendable, Identifiable {
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
    let metadata: JiraMetadata
    let bindable: Bool
    var id: String { key.objectID }
    enum CodingKeys: String, CodingKey {
        case key, version, text, metadata, bindable
        case observedAtUTC = "observed_at_utc"
    }
}
struct JiraCoverage: Decodable, Equatable, Sendable, Identifiable {
    let category: Int
    let status: String
    let pages: Int
    let objects: Int
    let unavailable: Int
    let reasons: [String]
    let observedTotal: Int64?
    let omittedPrefix: Bool
    var id: Int { category }
    var kind: String {
        [0: "issue", 1: "comments", 2: "changelog", 3: "relations"][category] ?? "issue"
    }
    var losses: Int { unavailable }
    enum CodingKeys: String, CodingKey {
        case category, status, pages, objects, unavailable, reasons
        case observedTotal = "observed_total"
        case omittedPrefix = "omitted_prefix"
    }
}
