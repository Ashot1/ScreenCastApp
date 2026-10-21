import CAmbient
import Dispatch
import Foundation

/// Built-in HTTP integrations on the shared engine.
enum IntegrationKind: String, Codable, Sendable {
    case github, jira, todoist
    var abi: UInt32 {
        switch self {
        case .github: UInt32(AMBIENT_INTEGRATION_GITHUB)
        case .jira: UInt32(AMBIENT_INTEGRATION_JIRA)
        case .todoist: UInt32(AMBIENT_INTEGRATION_TODOIST)
        }
    }
    /// Saved evidence is mapped to its integration only through the binding namespace.
    init?(namespace: String) {
        switch namespace {
        case "github-com-rest-v1": self = .github
        case "jira-cloud-rest-v1": self = .jira
        case "todoist-api-v1": self = .todoist
        default: return nil
        }
    }
}
/// Built-in integrations that the user switches on and off as a whole (D61).
enum BuiltInIntegration: CaseIterable, Sendable {
    case localProjects, codex, github, jira, todoist
    var abi: UInt32 {
        switch self {
        case .localProjects: UInt32(AMBIENT_INTEGRATION_LOCAL_PROJECTS)
        case .codex: UInt32(AMBIENT_INTEGRATION_CODEX)
        case .github: UInt32(AMBIENT_INTEGRATION_GITHUB)
        case .jira: UInt32(AMBIENT_INTEGRATION_JIRA)
        case .todoist: UInt32(AMBIENT_INTEGRATION_TODOIST)
        }
    }
}
enum IntegrationFailure: Error, Equatable, Sendable {
    case invalid, storage, deleteIncomplete
    case response(UInt32)
}
/// Adapter settings nested in a profile, decoded by the profile's kind.
enum IntegrationConfig: Equatable, Sendable {
    case github([GitHubRepository])
    case jira, todoist
}
struct IntegrationProfile: Decodable, Equatable, Sendable, Identifiable {
    let id: UInt64
    let kind: IntegrationKind
    let settingsRevision: UInt64
    let credentialGeneration: UInt64
    let name: String
    let scope: String?
    let config: IntegrationConfig
    let notBeforeUTC: Int64?
    let serverDeadlineKnown: Bool
    enum CodingKeys: String, CodingKey {
        case id, kind, name, scope, config
        case settingsRevision = "settings_revision"
        case credentialGeneration = "credential_generation"
        case notBeforeUTC = "not_before_utc"
        case serverDeadlineKnown = "server_deadline_known"
    }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UInt64.self, forKey: .id)
        kind = try c.decode(IntegrationKind.self, forKey: .kind)
        settingsRevision = try c.decode(UInt64.self, forKey: .settingsRevision)
        credentialGeneration = try c.decode(UInt64.self, forKey: .credentialGeneration)
        name = try c.decode(String.self, forKey: .name)
        scope = try c.decodeIfPresent(String.self, forKey: .scope)
        // Core validated the stored config; empty objects (Jira, Todoist) carry nothing for native.
        config =
            switch kind {
            case .github: .github(try c.decode([GitHubRepository].self, forKey: .config))
            case .jira: .jira
            case .todoist: .todoist
            }
        notBeforeUTC = try c.decodeIfPresent(Int64.self, forKey: .notBeforeUTC)
        serverDeadlineKnown = try c.decode(Bool.self, forKey: .serverDeadlineKnown)
    }
    var native: AmbientIntegrationProfileRef {
        AmbientIntegrationProfileRef(
            id: id, settings_revision: settingsRevision, credential_generation: credentialGeneration
        )
    }
}
struct IntegrationSettings: Decodable, Equatable, Sendable {
    let kind: IntegrationKind
    let instance: String
    let profiles: [IntegrationProfile]
    let exclusions: [String]
}
struct IntegrationOperation: Equatable, Sendable {
    let memory: UInt64
    let id: UInt64
    let profile: IntegrationProfile
    func cancel() { _ = ambient_integration_v1_cancel(memory, id) }
    func abort() throws { try IntegrationBridge.check(ambient_integration_v1_abort(memory, id)) }
    func release() throws {
        try IntegrationBridge.check(ambient_integration_v1_release(memory, id))
    }
}
struct IntegrationWindow<
    Object: Decodable & Equatable & Sendable, Coverage: Decodable & Equatable & Sendable
>:
    Equatable, Sendable
{
    let observationID: UInt64
    let intervalStart: Int64
    let intervalEnd: Int64
    let categories: [Coverage]
    let objects: [Object]
}
private struct IntegrationWindowPage<Object: Decodable, Coverage: Decodable & Equatable>: Decodable
{
    let observationID: UInt64
    let intervalStart: Int64
    let intervalEnd: Int64
    let categories: [Coverage]
    let objects: [Object]
    let nextAfter: Int?
    enum CodingKeys: String, CodingKey {
        case observationID = "observation_id"
        case intervalStart = "interval_start"
        case intervalEnd = "interval_end"
        case categories, objects
        case nextAfter = "next_after"
    }
}
struct IntegrationPreview<Object: Decodable & Equatable & Sendable>: Decodable, Equatable, Sendable
{
    let observationID: UInt64
    let objects: [Object]
    var json = ""
    enum CodingKeys: String, CodingKey {
        case observationID = "observation_id"
        case objects
    }
}
struct IntegrationSavedMetadata<Metadata: Decodable & Equatable & Sendable>: Decodable, Equatable,
    Sendable, Identifiable
{
    let bindingID: UInt64
    let evidenceID: UInt64
    let metadata: Metadata
    var id: UInt64 { bindingID }
    enum CodingKeys: String, CodingKey {
        case bindingID = "binding_id"
        case evidenceID = "evidence_id"
        case metadata
    }
}
struct IntegrationSavedPage<Metadata: Decodable & Equatable & Sendable>: Decodable, Equatable,
    Sendable
{
    let taskRevision: UInt64
    let rows: [IntegrationSavedMetadata<Metadata>]
    let nextAfter: Int64?
    enum CodingKeys: String, CodingKey {
        case taskRevision = "task_revision"
        case nextAfter = "next_after"
        case rows
    }
}
enum IntegrationBridge {
    static func check(_ status: UInt32) throws {
        if status == 1 { throw IntegrationFailure.invalid }
        try LocalSourceBridge.check(status)
    }
    static func json(
        capacity: Int,
        _ read: (UnsafeMutablePointer<UInt8>?, UInt32, UnsafeMutablePointer<UInt32>) -> UInt32
    ) throws -> Data {
        var bytes = [UInt8](repeating: 0, count: capacity)
        var used: UInt32 = 0
        try bytes.withUnsafeMutableBufferPointer {
            try check(read($0.baseAddress, UInt32(capacity), &used))
        }
        guard used <= capacity else { throw ModelCheckFailure.native }
        return Data(bytes.prefix(Int(used)))
    }
    static func window<Object, Coverage>(_ op: IntegrationOperation) throws
        -> IntegrationWindow<Object, Coverage>
    {
        var after = 0
        var objects: [Object] = []
        var first: IntegrationWindowPage<Object, Coverage>?
        while true {
            let data = try json(capacity: 262144) {
                ambient_integration_v1_window(op.memory, op.id, UInt32(after), $0, $1, $2)
            }
            let page = try JSONDecoder().decode(
                IntegrationWindowPage<Object, Coverage>.self, from: data)
            guard page.observationID == op.id, page.objects.count <= 50,
                objects.count + page.objects.count <= 512
            else { throw ModelCheckFailure.native }
            if let first {
                guard first.intervalStart == page.intervalStart,
                    first.intervalEnd == page.intervalEnd, first.categories == page.categories
                else { throw ModelCheckFailure.native }
            } else {
                first = page
            }
            objects.append(contentsOf: page.objects)
            guard let next = page.nextAfter else { break }
            guard next > after, next == objects.count, next <= 512 else {
                throw ModelCheckFailure.native
            }
            after = next
        }
        guard let first else { throw ModelCheckFailure.native }
        return IntegrationWindow(
            observationID: op.id, intervalStart: first.intervalStart,
            intervalEnd: first.intervalEnd, categories: first.categories, objects: objects)
    }
    static func preview<Object>(_ op: IntegrationOperation, indices: [Int]) throws
        -> IntegrationPreview<Object>
    {
        guard indices.allSatisfy({ (0..<512).contains($0) }) else {
            throw IntegrationFailure.invalid
        }
        let ids = indices.map(UInt32.init)
        let data = try ids.withUnsafeBufferPointer { ids in
            try json(capacity: 524288) {
                ambient_integration_v1_preview(
                    op.memory, op.id, ids.baseAddress, UInt32(ids.count), $0, $1, $2)
            }
        }
        var preview = try JSONDecoder().decode(IntegrationPreview<Object>.self, from: data)
        guard preview.observationID == op.id, preview.objects.count == ids.count,
            let text = String(data: data, encoding: .utf8)
        else { throw ModelCheckFailure.native }
        preview.json = text
        return preview
    }
}
/// Dispatch runs on its own serial queue; the call returns only after transport cleanup.
actor IntegrationTransportBridge {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.integration.http")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    #if AMBIENT_MODEL_TEST_SUPPORT
        private let testURL: String?
        init(testURL: String? = nil) { self.testURL = testURL }
    #endif
    func dispatch(_ op: IntegrationOperation, secret: IntegrationSecret) throws {
        var outcome: UInt32 = 0
        let (first, second) = secret.parts
        try LocalSourceBridge.withText(first) { first in
            try Self.optionalText(second) { second in
                #if AMBIENT_MODEL_TEST_SUPPORT
                    if let testURL {
                        try LocalSourceBridge.withText(testURL) { route in
                            try IntegrationBridge.check(
                                ambient_integration_v1_test_dispatch(
                                    op.memory, op.id, first, second, route, &outcome))
                        }
                        return
                    }
                #endif
                try IntegrationBridge.check(
                    ambient_integration_v1_dispatch(op.memory, op.id, first, second, &outcome))
            }
        }
        guard outcome == 200 else { throw IntegrationFailure.response(outcome) }
    }
    private static func optionalText(_ text: String?, _ body: (AmbientText) throws -> Void)
        rethrows
    {
        if let text {
            try LocalSourceBridge.withText(text, body)
        } else {
            try body(AmbientText(bytes: nil, length: 0))
        }
    }
}
extension MemorySession {
    func integrationSettings(_ kind: IntegrationKind) throws -> IntegrationSettings {
        let h = try sourceMemoryHandle()
        let settings = try JSONDecoder().decode(
            IntegrationSettings.self,
            from: IntegrationBridge.json(capacity: 1_048_576) {
                ambient_integration_v1_profiles(h, kind.abi, $0, $1, $2)
            })
        guard settings.kind == kind, settings.profiles.allSatisfy({ $0.kind == kind }) else {
            throw ModelCheckFailure.native
        }
        return settings
    }
    func integrationProfile(_ id: UInt64, kind: IntegrationKind) throws -> IntegrationProfile {
        guard let p = try integrationSettings(kind).profiles.first(where: { $0.id == id }) else {
            throw IntegrationFailure.invalid
        }
        return p
    }
    func integrationEnabled(_ kind: BuiltInIntegration) throws -> Bool {
        let h = try sourceMemoryHandle()
        var enabled: UInt32 = 0
        try IntegrationBridge.check(ambient_integration_v1_enabled(h, kind.abi, &enabled))
        // The C boundary supplies an integer, not a Swift Bool.
        guard enabled <= 1 else { throw ModelCheckFailure.native }
        return enabled == 1
    }
    func setIntegrationEnabled(_ kind: BuiltInIntegration, enabled: Bool) throws {
        let h = try sourceMemoryHandle()
        try IntegrationBridge.check(
            ambient_integration_v1_set_enabled(h, kind.abi, enabled ? 1 : 0))
    }
    func setIntegrationScopeExcluded(_ kind: IntegrationKind, scope: String, excluded: Bool)
        throws
    {
        let h = try sourceMemoryHandle()
        try LocalSourceBridge.withText(scope) {
            try IntegrationBridge.check(
                ambient_integration_v1_scope_excluded(h, kind.abi, $0, excluded ? 1 : 0))
        }
    }
    func saveGitHubProfile(
        id: UInt64, expected: UInt64, name: String, repositories: [GitHubRepository]
    ) throws -> IntegrationProfile {
        let h = try sourceMemoryHandle()
        guard repositories.count <= 32 else { throw IntegrationFailure.invalid }
        var raw: [AmbientGitHubRepository] = []
        var out = AmbientIntegrationProfileRef()
        // Borrowed texts stay valid only inside their closures, so the array is built by nesting.
        func append(_ index: Int) throws {
            if index == repositories.count {
                try raw.withUnsafeBufferPointer { r in
                    try LocalSourceBridge.withText(name) { name in
                        try IntegrationBridge.check(
                            ambient_integration_v1_github_profile_save(
                                h, id, expected, name, r.baseAddress, UInt32(r.count), &out))
                    }
                }
                return
            }
            try LocalSourceBridge.withText(repositories[index].owner) { owner in
                try LocalSourceBridge.withText(repositories[index].repository) { repo in
                    raw.append(AmbientGitHubRepository(owner: owner, repository: repo))
                    defer { raw.removeLast() }
                    try append(index + 1)
                }
            }
        }
        try append(0)
        return try integrationProfile(out.id, kind: .github)
    }
    func saveJiraProfile(id: UInt64, expected: UInt64, name: String, cloudID: String) throws
        -> IntegrationProfile
    {
        let h = try sourceMemoryHandle()
        var out = AmbientIntegrationProfileRef()
        try LocalSourceBridge.withText(name) { name in
            try LocalSourceBridge.withText(cloudID) { cloud in
                try IntegrationBridge.check(
                    ambient_integration_v1_jira_profile_save(h, id, expected, name, cloud, &out))
            }
        }
        return try integrationProfile(out.id, kind: .jira)
    }
    /// Core keeps the account it wrote during a read; native never passes one.
    func saveTodoistProfile(id: UInt64, expected: UInt64, name: String) throws
        -> IntegrationProfile
    {
        let h = try sourceMemoryHandle()
        var out = AmbientIntegrationProfileRef()
        try LocalSourceBridge.withText(name) { name in
            try IntegrationBridge.check(
                ambient_integration_v1_todoist_profile_save(h, id, expected, name, &out))
        }
        return try integrationProfile(out.id, kind: .todoist)
    }
    func beginIntegrationCredentialReplacement(_ p: IntegrationProfile) throws
        -> IntegrationProfile
    {
        var out = AmbientIntegrationProfileRef()
        try IntegrationBridge.check(
            ambient_integration_v1_credential_begin(
                sourceMemoryHandle(), p.id, p.settingsRevision, &out))
        return try integrationProfile(out.id, kind: p.kind)
    }
    func finishIntegrationProfileDeletion(_ p: IntegrationProfile) throws {
        try IntegrationBridge.check(
            ambient_integration_v1_profile_delete_finish(sourceMemoryHandle(), p.native))
    }
    func beginIntegrationRead(
        _ task: MemoryTask, profile: IntegrationProfile, target: String, expiresAt: Int64
    ) throws -> IntegrationOperation {
        let h = try sourceMemoryHandle()
        var id: UInt64 = 0
        try LocalSourceBridge.withText(target) { target in
            var read = AmbientIntegrationRead(
                task_id: task.id, task_revision: task.revision, profile: profile.native,
                target: target, session_expires_at: expiresAt)
            try IntegrationBridge.check(ambient_integration_v1_begin(h, &read, &id))
        }
        return IntegrationOperation(memory: h, id: id, profile: profile)
    }
    func integrationPreview<Object>(_ op: IntegrationOperation, indices: [Int]) throws
        -> IntegrationPreview<Object>
    {
        guard try sourceMemoryHandle() == op.memory else { throw ModelCheckFailure.closed }
        return try IntegrationBridge.preview(op, indices: indices)
    }
    func bindIntegrationObjects(_ op: IntegrationOperation, indices: [Int]) throws -> [UInt64] {
        guard try sourceMemoryHandle() == op.memory, indices.allSatisfy({ (0..<512).contains($0) })
        else { throw IntegrationFailure.invalid }
        let ids = indices.map(UInt32.init)
        var out = AmbientIntegrationBatch()
        try ids.withUnsafeBufferPointer { ids in
            try IntegrationBridge.check(
                ambient_integration_v1_bind(
                    op.memory, op.id, ids.baseAddress, UInt32(ids.count),
                    Int64(Date().timeIntervalSince1970), &out))
        }
        guard out.task_id > 0, out.task_revision > 0, out.count == ids.count, out.reserved == 0
        else { throw ModelCheckFailure.native }
        return withUnsafeBytes(of: out.evidence_ids) {
            Array($0.bindMemory(to: UInt64.self).prefix(Int(out.count)))
        }
    }
    func integrationSavedMetadata<Metadata>(
        _ kind: IntegrationKind, taskID: UInt64, after: Int64 = 0
    ) throws -> IntegrationSavedPage<Metadata> {
        let h = try sourceMemoryHandle()
        let data = try IntegrationBridge.json(capacity: 262144) {
            ambient_integration_v1_saved_metadata(h, kind.abi, taskID, after, $0, $1, $2)
        }
        let page = try JSONDecoder().decode(IntegrationSavedPage<Metadata>.self, from: data)
        guard page.rows.count <= 50,
            page.nextAfter == nil || page.nextAfter.map({ $0 > after }) == true
        else { throw ModelCheckFailure.native }
        return page
    }
}
