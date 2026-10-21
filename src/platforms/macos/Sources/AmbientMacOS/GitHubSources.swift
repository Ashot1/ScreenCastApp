import Foundation

typealias GitHubSourcesModel = IntegrationSourcesModel<GitHubAdapter>
struct GitHubRepository: Codable, Equatable, Sendable {
    let owner: String
    let repository: String
}
struct GitHubDraft: Equatable {
    var name = ""
    /// One `owner/repository` per line, in the user's spelling.
    var repositories = ""
    var token = ""
}
/// GitHub.com parts of the shared source skeleton: allowlist settings, token and error copy.
enum GitHubAdapter: IntegrationAdapter {
    typealias Object = GitHubObject
    typealias Coverage = GitHubCoverage
    typealias Metadata = GitHubMetadata
    typealias Draft = GitHubDraft
    static let kind = IntegrationKind.github
    static func draft(_ profile: IntegrationProfile?) -> GitHubDraft {
        guard let profile, case .github(let repositories) = profile.config else {
            return GitHubDraft()
        }
        return GitHubDraft(
            name: profile.name,
            repositories: repositories.map { "\($0.owner)/\($0.repository)" }.joined(
                separator: "\n"))
    }
    static func save(_ draft: GitHubDraft, over profile: IntegrationProfile?, memory: MemorySession)
        async throws -> IntegrationProfile
    {
        let repositories = try draft.repositories.split(
            separator: "\n", omittingEmptySubsequences: true
        ).map { line -> GitHubRepository in
            let fields = line.split(separator: "/", omittingEmptySubsequences: false)
            guard fields.count == 2 else { throw IntegrationFailure.invalid }
            return GitHubRepository(owner: String(fields[0]), repository: String(fields[1]))
        }
        return try await memory.saveGitHubProfile(
            id: profile?.id ?? 0, expected: profile?.settingsRevision ?? 0, name: draft.name,
            repositories: repositories)
    }
    static func secret(_ draft: GitHubDraft) -> IntegrationSecret {
        .githubToken(token: draft.token)
    }
    static func clearingSecret(_ draft: GitHubDraft) -> GitHubDraft {
        GitHubDraft(name: draft.name, repositories: draft.repositories)
    }
    static func bindable(_ object: GitHubObject) -> Bool { object.bindable }
    static func failureKey(_ failure: IntegrationFailure) -> String {
        switch failure {
        case .invalid: "github.error.invalid"
        case .storage: "github.error.storage"
        case .deleteIncomplete: "github.deleteIncomplete"
        case .response(let status):
            switch status {
            case 401: "github.error.unauthorized"
            case 403: "github.error.forbidden"
            case 429: "github.error.rate"
            case 404: "github.error.unavailable"
            case 409: "github.error.changed"
            case 408, 413: "integration.error.limit"
            case 499: "github.error.cancelled"
            default: "github.error.transport"
            }
        }
    }
}
struct GitHubOpenValue: Decodable, Equatable, Sendable {
    let raw: String
    let unknown: Bool
    private enum Keys: String, CodingKey {
        case known = "Known"
        case unknown = "Unknown"
    }
    init(from decoder: any Decoder) throws {
        if let raw = try? decoder.singleValueContainer().decode(String.self) {
            self.raw = raw
            unknown = false
        } else {
            let c = try decoder.container(keyedBy: Keys.self)
            if let known = try c.decodeIfPresent(String.self, forKey: .known) {
                raw = known
                unknown = false
            } else {
                raw = try c.decode(String.self, forKey: .unknown)
                unknown = true
            }
        }
    }
}
struct GitHubFields: Decodable, Equatable, Sendable {
    let kind: String
    let state: GitHubOpenValue?
    let author: String?
    let createdAt: Int64?
    let merged: Bool?
    let bodyProvided: Bool?
    let emptyBody: Bool?
    let headSHA: String?
    let baseSHA: String?
    let commitID: String?
    let originalCommitID: String?
    let reviewID: Int64?
    let replyID: Int64?
    let path: String?
    let line: Int64?
    let originalLine: Int64?
    let side: GitHubOpenValue?
    let subjectType: GitHubOpenValue?
    let startLine: Int64?
    let originalStartLine: Int64?
    let startSide: GitHubOpenValue?
    enum CodingKeys: String, CodingKey {
        case kind, state, author, merged, path, line, side
        case createdAt = "created_at"
        case bodyProvided = "body_provided"
        case emptyBody = "empty_body"
        case headSHA = "head_sha"
        case baseSHA = "base_sha"
        case commitID = "commit_id"
        case originalCommitID = "original_commit_id"
        case reviewID = "review_id"
        case replyID = "reply_id"
        case originalLine = "original_line"
        case subjectType = "subject_type"
        case startLine = "start_line"
        case originalStartLine = "original_start_line"
        case startSide = "start_side"
    }
}
struct GitHubMetadata: Decodable, Equatable, Sendable {
    let profileID: UInt64
    let settingsRevision: UInt64
    let credentialGeneration: UInt64
    let repositoryFullName: String
    let parentPRID: Int64
    let prNumber: Int64
    let intervalStart: Int64
    let intervalEnd: Int64
    let coverage: String
    let observation: String
    let currentState: String
    let appProjection: Bool
    let fields: GitHubFields
    enum CodingKeys: String, CodingKey {
        case coverage, observation, fields
        case profileID = "profile_id"
        case settingsRevision = "settings_revision"
        case credentialGeneration = "credential_generation"
        case repositoryFullName = "repository_full_name"
        case parentPRID = "parent_pr_id"
        case prNumber = "pr_number"
        case intervalStart = "interval_start"
        case intervalEnd = "interval_end"
        case currentState = "current_state"
        case appProjection = "app_projection"
    }
}
struct GitHubObject: Decodable, Equatable, Sendable, Identifiable {
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
    let metadata: GitHubMetadata
    let bindable: Bool
    var id: String { key.objectID }
    enum CodingKeys: String, CodingKey {
        case key, version, text, metadata, bindable
        case observedAtUTC = "observed_at_utc"
    }
}
struct GitHubCoverage: Decodable, Equatable, Sendable, Identifiable {
    let category: Int
    let status: String
    let pages: Int
    let objects: Int
    let malformed: Int
    let empty: Int
    let oversized: Int
    let ambiguous: Int
    var id: Int { category }
    var kind: String { [1: "review", 2: "inline", 3: "general"][category] ?? "pr" }
    var losses: Int { malformed + empty + oversized + ambiguous }
}
