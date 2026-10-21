import CAmbient
import Foundation
import Security
import TestKeychainSupport
import Testing

@testable import AmbientMacOS

typealias JiraWindow = IntegrationWindow<JiraObject, JiraCoverage>
typealias JiraPreview = IntegrationPreview<JiraObject>
typealias TodoistWindow = IntegrationWindow<TodoistObject, TodoistCoverage>
typealias TodoistPreview = IntegrationPreview<TodoistObject>

/// An isolated file keychain; nothing touches the user's login keychain.
@MainActor final class IntegrationKeychainFixture {
    let directory: URL
    let keychain: SecKeychain
    init() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "ambient-integration-" + UUID().uuidString)
        self.directory = directory
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        var raw: Unmanaged<SecKeychain>?
        let password = Array(UUID().uuidString.utf8)
        let status = password.withUnsafeBytes { p in
            directory.appendingPathComponent("isolated.keychain").path.withCString { path in
                ambient_test_keychain_create(path, p.baseAddress, UInt32(p.count), &raw)
            }
        }
        guard status == errSecSuccess, let raw else { throw IntegrationFailure.storage }
        keychain = raw.takeRetainedValue()
    }
    func databaseURL(_ name: String = "memory") -> URL {
        directory.appendingPathComponent(name + ".sqlite3")
    }
    func remove() {
        #expect(ambient_test_keychain_delete(keychain) == errSecSuccess)
        try? FileManager.default.removeItem(at: directory)
    }
}
/// Per-kind fixtures for the shared secret-owner suite.
private func fixtureProfile(_ kind: IntegrationKind, _ memory: MemorySession) async throws
    -> IntegrationProfile
{
    switch kind {
    case .github:
        try await memory.saveGitHubProfile(
            id: 0, expected: 0, name: "Work",
            repositories: [GitHubRepository(owner: "Example", repository: "Project")])
    case .jira:
        try await memory.saveJiraProfile(id: 0, expected: 0, name: "Work", cloudID: "Site-ID")
    case .todoist:
        try await memory.saveTodoistProfile(id: 0, expected: 0, name: "Work")
    }
}
private func fixtureSecret(_ kind: IntegrationKind, _ token: String) -> IntegrationSecret {
    switch kind {
    case .github: .githubToken(token: token)
    case .jira: .jiraBasic(email: "fixture@example.invalid", token: token)
    case .todoist: .todoistToken(token: token)
    }
}
private func token(_ e: IntegrationEnvelope?) -> String? {
    switch e?.secret {
    case .githubToken(let token), .todoistToken(let token): token
    case .jiraBasic(_, let token): token
    case nil: nil
    }
}

@Test func integrationLayoutsKindsAndScopeSettingsUseTheExistingStore() async throws {
    #expect(MemoryLayout<AmbientIntegrationProfileRef>.size == 24)
    #expect(MemoryLayout<AmbientIntegrationRead>.size == 64)
    #expect(MemoryLayout<AmbientIntegrationBatch>.size == 280)
    #expect(MemoryLayout<AmbientGitHubRepository>.size == 32)
    #expect(IntegrationKind.github.abi == 3)
    #expect(IntegrationKind.jira.abi == 4)
    #expect(IntegrationKind.todoist.abi == 5)
    #expect(IntegrationKind(namespace: "jira-cloud-rest-v1") == .jira)
    #expect(IntegrationKind(namespace: "todoist-api-v1") == .todoist)
    #expect(IntegrationKind(namespace: "github-com-rest-v1") == .github)
    #expect(IntegrationKind(namespace: "local-git-branch-v1") == nil)
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-integration-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }
    let memory = MemorySession(databaseURL: directory.appendingPathComponent("memory.sqlite3"))
    let p = try await memory.saveJiraProfile(id: 0, expected: 0, name: "Site", cloudID: "Cloud-ID")
    #expect(p.scope == "Cloud-ID" && p.kind == .jira)
    try await memory.setIntegrationScopeExcluded(.jira, scope: "Cloud-ID", excluded: true)
    let excluded = try await memory.integrationProfile(p.id, kind: .jira)
    #expect(excluded.settingsRevision > p.settingsRevision)
    #expect(excluded.credentialGeneration == p.credentialGeneration)
    let settings = try await memory.integrationSettings(.jira)
    #expect(settings.exclusions == ["Cloud-ID"])
    #expect(settings.instance.count == 32)
    let repositories = [GitHubRepository(owner: "Example", repository: "Project")]
    let g = try await memory.saveGitHubProfile(
        id: 0, expected: 0, name: "Repo", repositories: repositories)
    #expect(g.kind == .github && g.scope == nil && g.config == .github(repositories))
    #expect(try await memory.integrationProfile(p.id, kind: .jira).config == .jira)
    await #expect(throws: IntegrationFailure.invalid) {
        try await memory.setIntegrationScopeExcluded(.github, scope: "10", excluded: true)
    }
    // A new Todoist profile has no account until a read writes it; renaming keeps that.
    let t = try await memory.saveTodoistProfile(id: 0, expected: 0, name: "Personal")
    #expect(t.kind == .todoist && t.scope == nil && t.config == .todoist)
    let renamed = try await memory.saveTodoistProfile(
        id: t.id, expected: t.settingsRevision, name: "Renamed")
    #expect(renamed.name == "Renamed" && renamed.scope == nil)
    await #expect(throws: IntegrationFailure.invalid) {
        _ = try await memory.saveTodoistProfile(
            id: p.id, expected: excluded.settingsRevision, name: "Other kind")
    }
    try await memory.close()
}

@Test func integrationEnvelopeUsesExplicitKeysAndBindsInstanceKindProfileAndGeneration()
    throws
{
    let envelope = IntegrationEnvelope(
        version: 1, instance: String(repeating: "0a", count: 16), kind: .jira, profileID: 7,
        generation: 9, secret: .jiraBasic(email: "fixture@example.invalid", token: "opaque"))
    try envelope.validate()
    let json = try #require(
        try JSONSerialization.jsonObject(with: JSONEncoder().encode(envelope)) as? [String: Any])
    #expect(
        Set(json.keys) == ["version", "instance", "kind", "profile_id", "generation", "secret"])
    #expect(json["kind"] as? String == "jira")
    let secret = try #require(json["secret"] as? [String: [String: String]])
    #expect(secret == ["jira_basic": ["email": "fixture@example.invalid", "token": "opaque"]])
    #expect(
        try JSONDecoder().decode(IntegrationEnvelope.self, from: JSONEncoder().encode(envelope))
            == envelope)
    for bad in [
        IntegrationEnvelope(
            version: 2, instance: envelope.instance, kind: .jira, profileID: 7, generation: 9,
            secret: envelope.secret),
        IntegrationEnvelope(
            version: 1, instance: String(repeating: "0A", count: 16), kind: .jira, profileID: 7,
            generation: 9, secret: envelope.secret),
        IntegrationEnvelope(
            version: 1, instance: "short", kind: .jira, profileID: 7, generation: 9,
            secret: envelope.secret),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .jira, profileID: 0, generation: 9,
            secret: envelope.secret),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .jira, profileID: 7,
            generation: UInt64(Int64.max) + 1, secret: envelope.secret),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .jira, profileID: 7, generation: 9,
            secret: .jiraBasic(email: "bad:email", token: "opaque")),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .jira, profileID: 7, generation: 9,
            secret: .jiraBasic(email: "fixture@example.invalid", token: "bad token")),
    ] {
        #expect(throws: IntegrationFailure.invalid) { try bad.validate() }
    }
    let github = IntegrationEnvelope(
        version: 1, instance: envelope.instance, kind: .github, profileID: 7, generation: 9,
        secret: .githubToken(token: "opaque"))
    try github.validate()
    let githubJSON = try #require(
        try JSONSerialization.jsonObject(with: JSONEncoder().encode(github)) as? [String: Any])
    #expect(githubJSON["kind"] as? String == "github")
    #expect(
        try #require(githubJSON["secret"] as? [String: [String: String]])
            == ["github_token": ["token": "opaque"]])
    #expect(
        try JSONDecoder().decode(IntegrationEnvelope.self, from: JSONEncoder().encode(github))
            == github)
    for bad in [
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .jira, profileID: 7, generation: 9,
            secret: .githubToken(token: "opaque")),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .github, profileID: 7, generation: 9,
            secret: .githubToken(token: "bad token")),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .github, profileID: 7, generation: 9,
            secret: .githubToken(token: String(repeating: "t", count: 16385))),
    ] {
        #expect(throws: IntegrationFailure.invalid) { try bad.validate() }
    }
    let todoist = IntegrationEnvelope(
        version: 1, instance: envelope.instance, kind: .todoist, profileID: 7, generation: 9,
        secret: .todoistToken(token: "opaque"))
    try todoist.validate()
    let todoistJSON = try #require(
        try JSONSerialization.jsonObject(with: JSONEncoder().encode(todoist)) as? [String: Any])
    #expect(todoistJSON["kind"] as? String == "todoist")
    #expect(
        try #require(todoistJSON["secret"] as? [String: [String: String]])
            == ["todoist_token": ["token": "opaque"]])
    #expect(
        try JSONDecoder().decode(IntegrationEnvelope.self, from: JSONEncoder().encode(todoist))
            == todoist)
    #expect(IntegrationSecret.todoistToken(token: "opaque").parts == ("opaque", nil))
    for bad in [
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .github, profileID: 7, generation: 9,
            secret: .todoistToken(token: "opaque")),
        IntegrationEnvelope(
            version: 1, instance: envelope.instance, kind: .todoist, profileID: 7, generation: 9,
            secret: .todoistToken(token: "bad token")),
    ] {
        #expect(throws: IntegrationFailure.invalid) { try bad.validate() }
    }
    let unknown = Data(#"{"other":{"token":"x"}}"#.utf8)
    #expect(throws: (any Error).self) {
        try JSONDecoder().decode(IntegrationSecret.self, from: unknown)
    }
}

@Test(arguments: [IntegrationKind.github, .jira, .todoist]) @MainActor
func secretOwnerReplacesCoreFirstAndSurvivesKeychainRestart(kind: IntegrationKind) async throws {
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let memory = MemorySession(databaseURL: fixture.databaseURL())
    let store = IntegrationKeychain(keychain: fixture.keychain)
    let owner = IntegrationSecretOwner(memory: memory, store: store)
    let instance = try await memory.integrationSettings(kind).instance
    let p = try await fixtureProfile(kind, memory)
    #expect(try await owner.hasCredential(p) == false)
    let replaced = try await owner.replace(p, secret: fixtureSecret(kind, "local-test-PAT"))
    #expect(replaced.credentialGeneration != p.credentialGeneration)
    #expect(try await owner.hasCredential(replaced))
    #expect(try await owner.hasCredential(p) == false)
    #expect(token(try store.load(instance: instance, profileID: p.id)) == "local-test-PAT")
    #expect(
        try IntegrationKeychain(keychain: fixture.keychain).load(
            instance: instance, profileID: p.id)?.generation == replaced.credentialGeneration)
    let escaped = try await owner.replace(
        replaced, secret: fixtureSecret(kind, String(repeating: "\"", count: 16384)))
    #expect(token(try store.load(instance: instance, profileID: p.id))?.utf8.count == 16384)
    #expect(try await owner.hasCredential(escaped))
    let before = try await memory.integrationSettings(kind).profiles
    await #expect(throws: IntegrationFailure.invalid) {
        _ = try await owner.replace(escaped, secret: fixtureSecret(kind, "bad token"))
    }
    #expect(try await memory.integrationSettings(kind).profiles == before)
    let revoked = try await memory.beginIntegrationCredentialReplacement(escaped)
    #expect(try await owner.hasCredential(revoked) == false)
    try await owner.delete(revoked)
    #expect(try await memory.integrationSettings(kind).profiles.isEmpty)
    #expect(try store.load(instance: instance, profileID: p.id) == nil)
    try await memory.close()
}

@Test(arguments: [IntegrationKind.github, .jira, .todoist]) @MainActor
func secretOfAnotherMemoryDatabaseNeverMatchesTheSameProfileAndGeneration(kind: IntegrationKind)
    async throws
{
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let store = IntegrationKeychain(keychain: fixture.keychain)
    let first = MemorySession(databaseURL: fixture.databaseURL("first"))
    let second = MemorySession(databaseURL: fixture.databaseURL("second"))
    let firstOwner = IntegrationSecretOwner(memory: first, store: store)
    let secondOwner = IntegrationSecretOwner(memory: second, store: store)
    let a = try await firstOwner.replace(
        try await fixtureProfile(kind, first), secret: fixtureSecret(kind, "first-PAT"))
    // A fresh database restarts the clock, so the same profile ID and generation recur there.
    let b = try await second.beginIntegrationCredentialReplacement(
        try await fixtureProfile(kind, second))
    #expect(a.id == b.id && a.credentialGeneration == b.credentialGeneration)
    let firstInstance = try await first.integrationSettings(kind).instance
    let secondInstance = try await second.integrationSettings(kind).instance
    #expect(firstInstance != secondInstance)
    #expect(try await firstOwner.hasCredential(a))
    #expect(try await secondOwner.hasCredential(b) == false)
    #expect(try store.load(instance: secondInstance, profileID: b.id) == nil)
    #expect(token(try store.load(instance: firstInstance, profileID: a.id)) == "first-PAT")
    try await first.close()
    try await second.close()
}

@Test @MainActor func keychainRejectsAnOversizedItemOnRead() async throws {
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let instance = String(repeating: "ab", count: 16)
    let q: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "dev.ambient.foundation.integrations",
        kSecAttrAccount as String: "\(instance)/1", kSecAttrSynchronizable as String: false,
        kSecUseKeychain as String: fixture.keychain,
        kSecValueData as String: Data(repeating: 0x20, count: 65537),
    ]
    #expect(SecItemAdd(q as CFDictionary, nil) == errSecSuccess)
    #expect(throws: IntegrationFailure.storage) {
        _ = try IntegrationKeychain(keychain: fixture.keychain).load(
            instance: instance, profileID: 1)
    }
}

#if AMBIENT_MODEL_TEST_SUPPORT
    private final class IntegrationFaultStore: IntegrationSecretStore, @unchecked Sendable {
        // Test-only fault/gate state; the real isolated Keychain still owns successful effects.
        let base: IntegrationKeychain
        private let condition = NSCondition()
        private var failSave = false
        private var failDelete = false
        private var hold = false
        private var holdLoad = false
        private var entered = false
        init(_ base: IntegrationKeychain) { self.base = base }
        func configure(
            save: Bool = false, delete: Bool = false, hold: Bool = false, holdLoad: Bool = false
        ) {
            condition.withLock {
                failSave = save
                failDelete = delete
                self.hold = hold
                self.holdLoad = holdLoad
                entered = false
            }
        }
        func load(instance: String, profileID: UInt64) throws -> IntegrationEnvelope? {
            condition.lock()
            if holdLoad { entered = true }
            while holdLoad { condition.wait() }
            condition.unlock()
            return try base.load(instance: instance, profileID: profileID)
        }
        func save(_ e: IntegrationEnvelope) throws {
            condition.lock()
            entered = true
            while hold { condition.wait() }
            let fail = failSave
            condition.unlock()
            if fail { throw IntegrationFailure.storage }
            try base.save(e)
        }
        func delete(instance: String, profileID: UInt64) throws {
            if condition.withLock({ failDelete }) { throw IntegrationFailure.storage }
            try base.delete(instance: instance, profileID: profileID)
        }
        func waitUntilEntered() async throws {
            let deadline = ContinuousClock.now + .seconds(5)
            while !condition.withLock({ entered }) {
                guard ContinuousClock.now < deadline else { throw ModelCheckFailure.native }
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func release() {
            condition.lock()
            hold = false
            holdLoad = false
            condition.broadcast()
            condition.unlock()
        }
    }
    private enum GuardAdapter<A: IntegrationAdapter> {
        /// A profile load still waiting for Keychain when the user switches profile must not
        /// publish its old secret state.
        @MainActor static func run(_ kind: IntegrationKind) async throws {
            let fixture = try IntegrationKeychainFixture()
            defer { fixture.remove() }
            let memory = MemorySession(databaseURL: fixture.databaseURL())
            let store = IntegrationFaultStore(IntegrationKeychain(keychain: fixture.keychain))
            let owner = IntegrationSecretOwner(memory: memory, store: store)
            let p = try await owner.replace(
                try await fixtureProfile(kind, memory),
                secret: fixtureSecret(kind, "local-test-PAT"))
            let model = IntegrationSourcesModel<A>(memory: memory, secrets: owner)
            await model.use(p)
            #expect(model.secretAvailable == true)
            store.configure(holdLoad: true)
            let stale = Task { await model.loadProfiles() }
            try await store.waitUntilEntered()
            await model.use(nil)
            #expect(model.profile == nil && model.secretAvailable == nil)
            store.release()
            await stale.value
            #expect(model.profile == nil && model.secretAvailable == nil)
            try await memory.close()
        }
    }
    @Test(arguments: [IntegrationKind.github, .jira, .todoist]) @MainActor
    func profileLoadIgnoresALateSecretAnswerForAnEarlierProfile(kind: IntegrationKind)
        async throws
    {
        switch kind {
        case .github: try await GuardAdapter<GitHubAdapter>.run(kind)
        case .jira: try await GuardAdapter<JiraAdapter>.run(kind)
        case .todoist: try await GuardAdapter<TodoistAdapter>.run(kind)
        }
    }
    @Test(arguments: [IntegrationKind.github, .jira, .todoist]) @MainActor
    func failedSecretWriteAndDeleteNeverReactivateTheOldEnvelope(kind: IntegrationKind)
        async throws
    {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let store = IntegrationFaultStore(IntegrationKeychain(keychain: fixture.keychain))
        let owner = IntegrationSecretOwner(memory: memory, store: store)
        let instance = try await memory.integrationSettings(kind).instance
        let original = try await fixtureProfile(kind, memory)
        let active = try await owner.replace(
            original, secret: fixtureSecret(kind, "local-test-PAT"))
        store.configure(save: true)
        await #expect(throws: IntegrationFailure.storage) {
            _ = try await owner.replace(active, secret: fixtureSecret(kind, "new-test-PAT"))
        }
        let revoked = try await memory.integrationProfile(active.id, kind: kind)
        #expect(revoked.credentialGeneration != active.credentialGeneration)
        #expect(
            try store.load(instance: instance, profileID: active.id)?.generation
                == active.credentialGeneration)
        #expect(try await owner.hasCredential(revoked) == false)
        #expect(try await owner.hasCredential(active) == false)
        store.configure(delete: true)
        await #expect(throws: IntegrationFailure.deleteIncomplete) {
            try await owner.delete(revoked)
        }
        let pending = try await memory.integrationProfile(active.id, kind: kind)
        #expect(try store.load(instance: instance, profileID: active.id) != nil)
        #expect(try await owner.hasCredential(pending) == false)
        try await memory.close()
        let reopened = MemorySession(databaseURL: fixture.databaseURL())
        let restarted = IntegrationSecretOwner(memory: reopened, store: store)
        #expect(try await restarted.hasCredential(pending) == false)
        store.configure()
        try await restarted.delete(pending)
        #expect(try await reopened.integrationSettings(kind).profiles.isEmpty)
        #expect(try store.load(instance: instance, profileID: active.id) == nil)
        try await reopened.close()
    }
    @Test(arguments: [IntegrationKind.github, .jira, .todoist]) @MainActor
    func queuedOldDeletionCannotEraseAnActuallyLateSecretWrite(kind: IntegrationKind) async throws {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let store = IntegrationFaultStore(IntegrationKeychain(keychain: fixture.keychain))
        let owner = IntegrationSecretOwner(memory: memory, store: store)
        let instance = try await memory.integrationSettings(kind).instance
        let p = try await fixtureProfile(kind, memory)
        let active = try await owner.replace(p, secret: fixtureSecret(kind, "local-test-PAT"))
        store.configure(hold: true)
        let write = Task {
            try await owner.replace(active, secret: fixtureSecret(kind, "replacement-test-PAT"))
        }
        try await store.waitUntilEntered()
        var deletionReturned = false
        let delete = Task {
            defer { deletionReturned = true }
            do {
                try await owner.delete(active)
                Issue.record("old deletion unexpectedly succeeded")
            } catch {
                #expect(
                    (error as? ModelCheckFailure) == .busy
                        || (error as? ModelCheckFailure) == .stale)
            }
        }
        try await Task.sleep(for: .milliseconds(20))
        #expect(!deletionReturned)
        let core = try await memory.integrationProfile(active.id, kind: kind)
        #expect(core.credentialGeneration != active.credentialGeneration)
        #expect(
            try store.load(instance: instance, profileID: active.id)?.generation
                == active.credentialGeneration)
        store.release()
        let current = try await write.value
        await delete.value
        #expect(
            try store.load(instance: instance, profileID: current.id)?.generation
                == current.credentialGeneration)
        #expect(try await owner.hasCredential(current))
        try await memory.close()
    }
    /// A loopback Python HTTP fixture; each adapter supplies its own handler script.
    @MainActor final class IntegrationHTTPServer {
        let process = Process()
        let url: String
        let requests: URL
        init(script: String, mode: String, directory: URL) throws {
            requests = directory.appendingPathComponent("http-" + UUID().uuidString + ".jsonl")
            let pipe = Pipe()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
            process.arguments = ["-u", "-c", script, mode, requests.path]
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            try process.run()
            guard
                let value = String(data: pipe.fileHandleForReading.availableData, encoding: .utf8)?
                    .trimmingCharacters(in: .whitespacesAndNewlines),
                value.hasPrefix("http://127.0.0.1:")
            else { throw ModelCheckFailure.native }
            url = value
        }
        func paths() throws -> [String] {
            guard FileManager.default.fileExists(atPath: requests.path) else { return [] }
            return try String(contentsOf: requests, encoding: .utf8).split(separator: "\n").map {
                try JSONDecoder().decode(String.self, from: Data($0.utf8))
            }
        }
        func waitForRequest() async throws {
            let deadline = ContinuousClock.now + .seconds(5)
            while try paths().isEmpty {
                guard ContinuousClock.now < deadline else { throw ModelCheckFailure.native }
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func stop() {
            if process.isRunning {
                process.terminate()
                process.waitUntilExit()
            }
        }
    }
    @MainActor func integrationOwner(
        _ memory: MemorySession, _ fixture: IntegrationKeychainFixture,
        _ server: IntegrationHTTPServer
    ) -> IntegrationSecretOwner {
        IntegrationSecretOwner(
            memory: memory, store: IntegrationKeychain(keychain: fixture.keychain),
            transport: IntegrationTransportBridge(testURL: server.url))
    }
    private let jiraHTTPScript = #"""
        import json,sys,time,os,base64
        from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
        from urllib.parse import urlsplit,parse_qs
        mode,capture=sys.argv[1:]
        detail=0
        def adf(text):return {'type':'doc','version':1,'content':[{'type':'paragraph','content':[{'type':'text','text':text}]}]}
        def change(id,text):return {'id':id,'created':'2021-01-17T12:34:00.123+0000','items':[{'field':'status','fieldtype':'jira','from':None,'to':text}]}
        class Handler(BaseHTTPRequestHandler):
         def log_message(self,*args):pass
         def do_GET(self):
          global detail
          assert self.headers['Authorization']=='Basic '+base64.b64encode(b'fixture@example.invalid:local-test-PAT').decode()
          assert self.headers['Accept']=='application/json'
          assert self.headers['User-Agent']=='Ambient/0.1.0'
          with open(capture,'a') as f:f.write(json.dumps(self.path)+'\n')
          path=urlsplit(self.path).path;q=parse_qs(urlsplit(self.path).query)
          if mode=='hold':
           while not os.path.exists(capture+'.release'):time.sleep(.01)
          if mode in ['401','403','404','429','503']:
           self.send_response(int(mode));self.send_header('Retry-After','120');self.send_header('Content-Length','0');self.end_headers();return
          prefix='/ex/jira/Site-ID/rest/api/3/issue/'
          if path in [prefix+'OLD-1',prefix+'issue%2Fopaque']:
           detail+=1
           assert q['updateHistory']==['false'] and q['failFast']==['false']
           assert q['fields']==['summary,description,status,issuetype,project,created,updated,parent,subtasks,issuelinks']
           body={'id':'other' if mode=='final-change' and detail>1 else 'issue/opaque','key':'NEW-2','fields':{'summary':'Initial title' if detail==1 else 'Final title','description':None,'status':{'id':'state','name':'Custom'},'parent':{'id':'same','key':'PARENT-1'},'subtasks':[{'id':'same','key':'CHILD-1'}],'issuelinks':[{'id':'same','type':{'id':'type','name':'Blocks','outward':'blocks'},'outwardIssue':{'id':'target','key':'LINK-1'}}]}}
          elif path==prefix+'issue%2Fopaque/comment':
           assert q=={'startAt':['0'],'maxResults':['50'],'orderBy':['-created']}
           if mode in ['category403','category404']:
            self.send_response(int(mode[-3:]));self.send_header('Content-Length','0');self.end_headers();return
           body={'startAt':0,'maxResults':50,'total':2,'comments':[{'id':'comment-id','body':adf('Newest exact 🌿'),'created':'2021-01-17T13:00:00+0000','updated':'2021-01-17T13:00:00+0000'},{'id':'empty-id','body':None}]}
          elif path==prefix+'issue%2Fopaque/changelog':
           if q=={'startAt':['0'],'maxResults':['1']}:body={'startAt':0,'maxResults':1,'total':251,'values':[change('old','old')]}
           else:
            assert q=={'startAt':['1'],'maxResults':['50']}
            body={'startAt':1,'maxResults':50,'total':3,'values':[change('change-1','In progress'),change('change-2','Done')]}
          else:raise AssertionError('unexpected Jira path')
          data=json.dumps(body,ensure_ascii=False).encode()
          self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers()
          try:self.wfile.write(data)
          except BrokenPipeError:pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        print('http://127.0.0.1:'+str(server.server_port),flush=True);server.serve_forever()
        """#
    private let jiraSecret = IntegrationSecret.jiraBasic(
        email: "fixture@example.invalid", token: "local-test-PAT")
    @Test @MainActor func jiraKeychainHTTPBatchAnd08BShareOneStore() async throws {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let server = try IntegrationHTTPServer(
            script: jiraHTTPScript, mode: "standard", directory: fixture.directory)
        defer { server.stop() }
        let model = try AssistanceHTTPServer(mode: "jira")
        defer { model.stop() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let coordinator = try await localReady(memory: memory, server: model)
        let owner = integrationOwner(memory, fixture, server)
        let original = try await memory.saveJiraProfile(
            id: 0, expected: 0, name: "Work", cloudID: "Site-ID")
        let profile = try await owner.replace(original, secret: jiraSecret)
        let task = try await memory.createTask("Discuss selected issue objects")
        let op = try await coordinator.beginIntegrationRead(task, profile: profile, target: "OLD-1")
        let window: JiraWindow = try await owner.read(op)
        #expect(window.objects.count == 7)
        #expect(window.categories[2].omittedPrefix)
        #expect(window.categories[2].reasons.contains("total_changed"))
        #expect(
            window.objects.first(where: { $0.metadata.content.tag == "issue" })?.text
                == "Final title")
        let relations = window.objects.filter {
            ["parent", "subtask", "issue_link"].contains($0.metadata.content.tag)
        }
        #expect(Set(relations.map(\.id)).count == 3)
        #expect(try server.paths().count == 5)
        let selected = ["issue", "comment", "changelog"].compactMap { tag in
            window.objects.firstIndex(where: { $0.metadata.content.tag == tag })
        }
        #expect(selected.count == 3)
        await #expect(throws: IntegrationFailure.invalid) {
            _ = try await memory.bindIntegrationObjects(op, indices: selected)
        }
        let preview: JiraPreview = try await memory.integrationPreview(op, indices: selected)
        #expect(preview.objects.count == 3 && preview.json.contains("Final title"))
        let ids = try await coordinator.bindIntegrationObjects(op, indices: selected)
        try op.release()
        let current = try await memory.task(task.id)
        let evidence = try await memory.evidence(taskID: task.id)
        #expect(evidence.items.allSatisfy { $0.assistanceSelectable })
        let context = try await coordinator.prepareAssistance(current, notes: ids)
        #expect(context.context.contains("jira-cloud-rest-v1"))
        #expect(
            !context.context.contains("local-test-PAT")
                && !context.context.contains("fixture@example.invalid"))
        _ = try await coordinator.runAssistance(context.requestID)
        let proposal = try #require(try await coordinator.proposalFeed().first)
        let details = try await coordinator.proposalDetails(proposal.target)
        guard case .jira(let cited) = details.citations.first?.integration else {
            Issue.record("cited Jira metadata missing")
            return
        }
        #expect(cited.cloudID == "Site-ID")
        #expect(try await coordinator.copyProposal(proposal.target) == "Copy exact 🌿 saved text")
        let metadata: IntegrationSavedPage<JiraMetadata> =
            try await memory
            .integrationSavedMetadata(.jira, taskID: task.id)
        #expect(metadata.rows.count == 3)
        try await memory.setIntegrationScopeExcluded(.jira, scope: "Site-ID", excluded: true)
        #expect(try await coordinator.proposalFeed().isEmpty)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.copyProposal(proposal.target)
        }
        try await memory.setIntegrationScopeExcluded(.jira, scope: "Site-ID", excluded: false)
        #expect(try await coordinator.proposalFeed().isEmpty)
        for value in ["fixture@example.invalid", "local-test-PAT"] {
            #expect(try Data(contentsOf: fixture.databaseURL()).range(of: Data(value.utf8)) == nil)
        }
        try await coordinator.shutdown()
        try await memory.close()
        let reopened = MemorySession(databaseURL: fixture.databaseURL())
        let again: IntegrationSavedPage<JiraMetadata> =
            try await reopened
            .integrationSavedMetadata(.jira, taskID: task.id)
        #expect(again == metadata)
        #expect(try await reopened.proposalFeed().isEmpty)
        try await reopened.close()
    }
    @Test @MainActor func jiraSwitchClosesReadingAndAssistanceThenRestoresSavedFactsWithoutReread()
        async throws
    {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let server = try IntegrationHTTPServer(
            script: jiraHTTPScript, mode: "standard", directory: fixture.directory)
        defer { server.stop() }
        let model = try AssistanceHTTPServer(mode: "jira")
        defer { model.stop() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let coordinator = try await localReady(memory: memory, server: model)
        let owner = integrationOwner(memory, fixture, server)
        let original = try await memory.saveJiraProfile(
            id: 0, expected: 0, name: "Work", cloudID: "Site-ID")
        let profile = try await owner.replace(original, secret: jiraSecret)
        let task = try await memory.createTask("Discuss selected issue objects")
        let op = try await coordinator.beginIntegrationRead(task, profile: profile, target: "OLD-1")
        let window: JiraWindow = try await owner.read(op)
        let index = try #require(
            window.objects.firstIndex(where: { $0.metadata.content.tag == "issue" }))
        _ = try await memory.integrationPreview(op, indices: [index]) as JiraPreview
        let ids = try await coordinator.bindIntegrationObjects(op, indices: [index])
        try op.release()
        let requests = try server.paths().count
        let before = try await memory.task(task.id)
        let pending = try await coordinator.prepareAssistance(before, notes: ids)
        try await memory.setIntegrationEnabled(.jira, enabled: false)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.runAssistance(pending.requestID)
        }
        let current = try await memory.task(task.id)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.prepareAssistance(current, notes: ids)
        }
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.beginIntegrationRead(
                current, profile: profile, target: "OLD-1")
        }
        #expect(try await memory.evidence(taskID: task.id).items.count == 1)
        try await memory.setIntegrationEnabled(.jira, enabled: true)
        // Saved facts and the stored key are usable again without a new read or key entry.
        let restored = try await coordinator.prepareAssistance(current, notes: ids)
        #expect(restored.context.contains("jira-cloud-rest-v1"))
        #expect(try await owner.hasCredential(profile))
        #expect(try server.paths().count == requests)
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func jiraNativeHTTPAccessMatrixKeepsCategoryLossSeparateFromTerminalFailure()
        async throws
    {
        for mode in [
            "401", "403", "404", "429", "503", "category403", "category404", "final-change",
        ] {
            let fixture = try IntegrationKeychainFixture()
            defer { fixture.remove() }
            let server = try IntegrationHTTPServer(
                script: jiraHTTPScript, mode: mode, directory: fixture.directory)
            defer { server.stop() }
            let model = try AssistanceHTTPServer(mode: "jira")
            defer { model.stop() }
            let memory = MemorySession(databaseURL: fixture.databaseURL())
            let coordinator = try await localReady(memory: memory, server: model)
            let owner = integrationOwner(memory, fixture, server)
            let original = try await memory.saveJiraProfile(
                id: 0, expected: 0, name: "Work", cloudID: "Site-ID")
            let p = try await owner.replace(original, secret: jiraSecret)
            let task = try await memory.createTask("Selected issue")
            let op = try await coordinator.beginIntegrationRead(task, profile: p, target: "OLD-1")
            do {
                let window: JiraWindow = try await owner.read(op)
                #expect(mode.hasPrefix("category"))
                #expect(window.categories[1].status == "partial")
                #expect(!window.objects.contains { $0.metadata.content.tag == "comment" })
                #expect(window.objects.contains { $0.metadata.content.tag == "issue" })
            } catch let error as IntegrationFailure {
                let expected = mode == "final-change" ? UInt32(409) : UInt32(mode)
                #expect(error == .response(expected ?? 0))
            }
            try op.release()
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            let current = try await memory.integrationProfile(p.id, kind: .jira)
            if mode == "401" {
                #expect(current.credentialGeneration != p.credentialGeneration)
                #expect(try await owner.hasCredential(current) == false)
            } else {
                #expect(current.credentialGeneration == p.credentialGeneration)
            }
            if mode == "429" || mode == "503" {
                #expect(current.notBeforeUTC != nil && current.serverDeadlineKnown)
                await #expect(throws: ModelCheckFailure.busy) {
                    _ = try await coordinator.beginIntegrationRead(
                        task, profile: current, target: "OLD-1")
                }
            }
            #expect(
                try server.paths().count
                    == (mode.hasPrefix("category") || mode == "final-change" ? 5 : 1))
            try await coordinator.shutdown()
            try await memory.close()
        }
    }
    @Test @MainActor func jiraNativeCancellationAndSiteABARejectLateHTTPAndAllowExplicitFreshRead()
        async throws
    {
        for mode in ["cancel", "site-aba"] {
            let fixture = try IntegrationKeychainFixture()
            defer { fixture.remove() }
            let server = try IntegrationHTTPServer(
                script: jiraHTTPScript, mode: "hold", directory: fixture.directory)
            defer { server.stop() }
            let model = try AssistanceHTTPServer(mode: "jira")
            defer { model.stop() }
            let memory = MemorySession(databaseURL: fixture.databaseURL())
            let coordinator = try await localReady(memory: memory, server: model)
            let owner = integrationOwner(memory, fixture, server)
            let original = try await memory.saveJiraProfile(
                id: 0, expected: 0, name: "Work", cloudID: "Site-ID")
            let p = try await owner.replace(original, secret: jiraSecret)
            let task = try await memory.createTask("Selected issue")
            let op = try await coordinator.beginIntegrationRead(task, profile: p, target: "OLD-1")
            let pending = Task { () -> JiraWindow in try await owner.read(op) }
            try await server.waitForRequest()
            // The in-flight dispatch keeps the job until its transport has actually finished.
            #expect(throws: ModelCheckFailure.busy) { try op.release() }
            if mode == "cancel" {
                op.cancel()
            } else {
                try await memory.setIntegrationScopeExcluded(
                    .jira, scope: "Site-ID", excluded: true)
                try await memory.setIntegrationScopeExcluded(
                    .jira, scope: "Site-ID", excluded: false)
            }
            try Data().write(to: URL(fileURLWithPath: server.requests.path + ".release"))
            do {
                _ = try await pending.value
                Issue.record("late Jira result revived")
            } catch {}
            await #expect(throws: (any Error).self) {
                let _: JiraPreview = try await memory.integrationPreview(op, indices: [0])
            }
            try op.release()
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            let current = try await memory.integrationProfile(p.id, kind: .jira)
            let fresh = try await coordinator.beginIntegrationRead(
                task, profile: current, target: "OLD-1")
            let window: JiraWindow = try await owner.read(fresh)
            #expect(window.objects.count == 7)
            try fresh.release()
            try await coordinator.shutdown()
            try await memory.close()
        }
    }
    private let todoistHTTPScript = #"""
        import json,sys
        from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
        mode,capture=sys.argv[1:]
        task={'id':'task-1','user_id':'owner-9','project_id':'project-1','content':'Buy **milk** [x](https://example.invalid)','description':'','checked':False,'is_deleted':False,'added_at':'2026-10-20T10:00:00Z','updated_at':'2026-10-20T11:00:00Z','completed_at':None,'due':{'date':'2026-10-21','timezone':None,'is_recurring':False,'string':'tomorrow','lang':'en'},'deadline':None,'duration':None}
        class Handler(BaseHTTPRequestHandler):
         def log_message(self,*args):pass
         def do_GET(self):
          assert self.headers['Authorization']=='Bearer local-test-PAT'
          assert self.headers['Accept']=='application/json'
          assert self.headers['User-Agent']=='Ambient/0.1.0'
          with open(capture,'a') as f:f.write(json.dumps(self.path)+'\n')
          if self.path=='/api/v1/user':body={'id':'account-1','email':'CANARY-EMAIL@example.invalid','token':'CANARY-TOKEN','websocket_url':'wss://canary.invalid'}
          elif mode=='429':
           data=json.dumps({'error':'CANARY-429','error_extra':{'retry_after':120}}).encode()
           self.send_response(429);self.send_header('Retry-After','1');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data);return
          elif self.path=='/api/v1/tasks/task-1':body=task
          elif self.path=='/api/v1/projects/project-1':body={'id':'project-1','name':'Home','description':'','parent_id':None,'is_archived':False,'is_deleted':False,'created_at':None,'updated_at':None}
          elif self.path=='/api/v1/comments?task_id=task-1&limit=50':body={'results':[{'id':'c-1','content':'Exact 🌿 note','posted_uid':'owner-9','posted_at':'2026-10-20T12:00:00Z','is_deleted':False,'file_attachment':None}],'next_cursor':None}
          else:raise AssertionError('unexpected Todoist path')
          data=json.dumps(body,ensure_ascii=False).encode()
          self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        print('http://127.0.0.1:'+str(server.server_port),flush=True);server.serve_forever()
        """#
    @Test @MainActor func todoistKeychainHTTPPinBatchAnd08BShareOneStore() async throws {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let server = try IntegrationHTTPServer(
            script: todoistHTTPScript, mode: "standard", directory: fixture.directory)
        defer { server.stop() }
        let model = try AssistanceHTTPServer(mode: "todoist")
        defer { model.stop() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let coordinator = try await localReady(memory: memory, server: model)
        let owner = integrationOwner(memory, fixture, server)
        let original = try await memory.saveTodoistProfile(id: 0, expected: 0, name: "Personal")
        let profile = try await owner.replace(
            original, secret: .todoistToken(token: "local-test-PAT"))
        #expect(profile.scope == nil)
        let task = try await memory.createTask("Plan the week")
        let op = try await coordinator.beginIntegrationRead(
            task, profile: profile, target: "task-1")
        let window: TodoistWindow = try await owner.read(op)
        #expect(
            try server.paths() == [
                "/api/v1/user", "/api/v1/tasks/task-1", "/api/v1/projects/project-1",
                "/api/v1/comments?task_id=task-1&limit=50", "/api/v1/tasks/task-1",
            ])
        #expect(
            window.objects.map(\.metadata.content.fields.kind).sorted()
                == ["comment", "project", "task"])
        #expect(window.objects.allSatisfy { $0.key.scope == "account-1" })
        let pinned = try await memory.integrationProfile(profile.id, kind: .todoist)
        #expect(pinned.scope == "account-1")
        #expect(pinned.settingsRevision == profile.settingsRevision)
        #expect(pinned.credentialGeneration == profile.credentialGeneration)
        #expect(
            window.objects.first { $0.metadata.content.fields.kind == "task" }?.text
                == "Buy **milk** [x](https://example.invalid)")
        let preview: TodoistPreview = try await memory.integrationPreview(op, indices: [0, 1, 2])
        #expect(preview.objects.count == 3)
        let ids = try await coordinator.bindIntegrationObjects(op, indices: [0, 1, 2])
        try op.release()
        let current = try await memory.task(task.id)
        let context = try await coordinator.prepareAssistance(current, notes: ids)
        #expect(context.context.contains("todoist-api-v1"))
        _ = try await coordinator.runAssistance(context.requestID)
        let proposal = try #require(try await coordinator.proposalFeed().first)
        let details = try await coordinator.proposalDetails(proposal.target)
        guard case .todoist(let cited) = details.citations.first?.integration else {
            Issue.record("cited Todoist metadata missing")
            return
        }
        #expect(cited.account == "account-1")
        let saved: IntegrationSavedPage<TodoistMetadata> =
            try await memory.integrationSavedMetadata(.todoist, taskID: task.id)
        #expect(saved.rows.count == 3)
        try await memory.setIntegrationScopeExcluded(.todoist, scope: "account-1", excluded: true)
        #expect(try await coordinator.proposalFeed().isEmpty)
        // Saving the profile keeps the account Core wrote; native never sends one.
        let latest = try await memory.integrationProfile(profile.id, kind: .todoist)
        let renamed = try await memory.saveTodoistProfile(
            id: latest.id, expected: latest.settingsRevision, name: "Renamed")
        #expect(renamed.scope == "account-1" && renamed.name == "Renamed")
        try await coordinator.shutdown()
        try await memory.close()
        let bytes = try Data(contentsOf: fixture.databaseURL())
        for value in ["local-test-PAT", "CANARY-EMAIL", "CANARY-TOKEN", "wss://canary"] {
            #expect(bytes.range(of: Data(value.utf8)) == nil, "\(value)")
        }
    }
    @Test @MainActor func todoistRateLimitTakesTheLaterDeadlineAndKeepsNoErrorBody() async throws {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let server = try IntegrationHTTPServer(
            script: todoistHTTPScript, mode: "429", directory: fixture.directory)
        defer { server.stop() }
        let model = try AssistanceHTTPServer(mode: "todoist")
        defer { model.stop() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let coordinator = try await localReady(memory: memory, server: model)
        let owner = integrationOwner(memory, fixture, server)
        let original = try await memory.saveTodoistProfile(id: 0, expected: 0, name: "Personal")
        let profile = try await owner.replace(
            original, secret: .todoistToken(token: "local-test-PAT"))
        let task = try await memory.createTask("Plan the week")
        let started = Int64(Date().timeIntervalSince1970)
        let op = try await coordinator.beginIntegrationRead(
            task, profile: profile, target: "task-1")
        await #expect(throws: IntegrationFailure.response(429)) {
            let _: TodoistWindow = try await owner.read(op)
        }
        try op.release()
        let paused = try await memory.integrationProfile(profile.id, kind: .todoist)
        // The body's 120 s wins over the header's 1 s.
        #expect(paused.serverDeadlineKnown)
        #expect((paused.notBeforeUTC ?? 0) >= started + 119)
        await #expect(throws: ModelCheckFailure.busy) {
            _ = try await coordinator.beginIntegrationRead(task, profile: paused, target: "task-1")
        }
        try await coordinator.shutdown()
        try await memory.close()
        #expect(
            try Data(contentsOf: fixture.databaseURL()).range(of: Data("CANARY-429".utf8)) == nil)
    }
#endif

@Test @MainActor func jiraNestedPresenceKeepsNullMissingAndInvalidSeparateInPresentation() {
    let value: JiraPresence<JiraNamed> = .value(JiraNamed(id: .missing, name: .null, key: .missing))
    #expect(JiraLossView(reason: "null:parent").stateKey == "jira.null")
    #expect(value.flatMap { $0.name } == .null)
    let states: [JiraPresence<JiraNamed>] = [.missing, .null, .invalid]
    #expect(states.map { $0.flatMap { $0.name } } == [.missing, .null, .invalid])
}

@Test @MainActor func todoistPresentationKeepsLossKindsAndCopyKeys() {
    #expect(TodoistAdapter.failureKey(.response(409)) == "todoist.error.changed")
    #expect(TodoistAdapter.failureKey(.response(503)) == "todoist.error.rate")
    #expect(TodoistAdapter.secret(TodoistDraft(name: "n", token: "t")) == .todoistToken(token: "t"))
    #expect(TodoistAdapter.clearingSecret(TodoistDraft(name: "n", token: "t")).token.isEmpty)
    let states: [TodoistPresence<TodoistDue>] = [.missing, .null, .invalid]
    #expect(states.map { $0.flatMap { $0.date } } == [.missing, .null, .invalid])
}
