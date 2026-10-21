import CAmbient
import Foundation
import Network
import Security
import TestKeychainSupport
import Testing

@testable import AmbientMacOS

@Test func chatGPTNativeAuthRecordRoundTripsAndCancellationDoesNotReachNetwork() async throws {
    #expect(MemoryLayout<AmbientAuthRecord>.size == 616)
    #expect(MemoryLayout<AmbientAuthRecord>.alignment == 8)
    let bridge = ChatGPTBridge()
    let handle = try await bridge.begin(
        authFixture(), redirect: "http://127.0.0.1:12345/auth/callback", refresh: false)
    bridge.cancel(handle)
    await #expect(throws: ChatGPTFailure.cancelled) { try await bridge.prepare(handle) }
    try await bridge.close(handle)
    await #expect(throws: ChatGPTFailure.closed) { try await bridge.prepare(handle) }
}

private final class AuthTestStore: ChatGPTCredentialStore, @unchecked Sendable {
    // Test-only fault injection; the lock keeps synchronous persistence observable across actors.
    private let lock = NSLock()
    private var envelope: ChatGPTEnvelope?
    private var failSave = false
    private var readFailure: ChatGPTFailure?
    private var saves = 0
    init(_ envelope: ChatGPTEnvelope?, readFailure: ChatGPTFailure? = nil) {
        self.envelope = envelope
        self.readFailure = readFailure
    }
    var saveCount: Int { lock.withLock { saves } }
    func rejectWrites() { lock.withLock { failSave = true } }
    func setReadFailure(_ value: ChatGPTFailure?) { lock.withLock { readFailure = value } }
    func load() throws -> ChatGPTEnvelope? {
        try lock.withLock {
            if let readFailure { throw readFailure }
            return envelope
        }
    }
    func save(_ e: ChatGPTEnvelope) throws {
        try lock.withLock {
            if failSave { throw ChatGPTFailure.storage }
            envelope = e
            saves += 1
        }
    }
}
private actor AuthTestProtocol: ChatGPTProtocol {
    var posts = 0
    var begins = 0
    private let result: ChatGPTEnvelope
    private let store: AuthTestStore
    private var refreshing = false
    private var prepareFailure = false
    private var hold = false
    private var releaseWaiter: CheckedContinuation<Void, Never>?
    private var dispatchWaiter: CheckedContinuation<Void, Never>?
    private var dispatchFailure = false
    private var closeFailure = false
    init(_ result: ChatGPTEnvelope, store: AuthTestStore) {
        self.result = result
        self.store = store
    }
    func configure(prepareFailure: Bool = false, hold: Bool = false, dispatchFailure: Bool = false)
    {
        self.prepareFailure = prepareFailure
        self.hold = hold
        self.dispatchFailure = dispatchFailure
    }
    func waitForDispatch() async {
        if posts == 0 { await withCheckedContinuation { dispatchWaiter = $0 } }
    }
    func release() {
        releaseWaiter?.resume()
        releaseWaiter = nil
    }
    func rejectClose(_ value: Bool) { closeFailure = value }
    func begin(_ envelope: ChatGPTEnvelope, redirect: String?, refresh: Bool) throws -> UInt64 {
        begins += 1
        refreshing = refresh
        return 1
    }
    func prepare(_ handle: UInt64) throws -> String? {
        if prepareFailure { throw ChatGPTFailure.network }
        return refreshing ? nil : "https://auth.openai.com/api/accounts/authorize?fixture=1"
    }
    func callback(_ handle: UInt64, url: String) -> String { "issued_1" }
    func dispatch(_ handle: UInt64, clientID: String) async throws -> ChatGPTEnvelope {
        #expect(try store.load()?.credentials == nil)
        #expect(try store.load()?.clientID == "issued_1")
        posts += 1
        dispatchWaiter?.resume()
        dispatchWaiter = nil
        if hold { await withCheckedContinuation { releaseWaiter = $0 } }
        if dispatchFailure { throw ChatGPTFailure.network }
        return result
    }
    func close(_ handle: UInt64) throws { if closeFailure { throw ChatGPTFailure.response } }
    nonisolated func cancel(_ handle: UInt64) {}
}
private actor AuthTestListener: ChatGPTCallbackListener {
    func start() -> String { "http://127.0.0.1:12345/auth/callback" }
    func next() -> String { "http://127.0.0.1:12345/auth/callback?fixture=1" }
    func respond() {}
    func close() {}
}
private func authFixture() -> ChatGPTEnvelope {
    ChatGPTEnvelope(
        host: "urn:uuid:12345678-1234-4234-9234-123456789abc", registration: 3,
        clientID: "issued_1", issuer: "https://auth.openai.com", subject: "fixture-subject",
        hint: "fixture-hint", nonce: "fixture-nonce",
        credentials: ChatGPTCredentials(
            access: "fixture-access", refresh: "fixture-refresh",
            scopes: ["openid", "resource.invoke", "chatgpt.tokens.use.direct"],
            expiresAt: 2_000_000_000))
}
@Test func chatGPTCachedRestartDoesNotStartNetworkAndRefreshRemovesOldCredentialsBeforePost()
    async throws
{
    let store = AuthTestStore(authFixture())
    let bridge = AuthTestProtocol(authFixture(), store: store)
    let coordinator = ChatGPTCoordinator(
        store: store, bridge: bridge,
        openBrowser: { _ in
            Issue.record("unexpected browser")
            return false
        })
    #expect(try await coordinator.load() == .saved)
    #expect(await bridge.begins == 0)
    try await coordinator.run(refresh: true)
    #expect(await bridge.posts == 1)
    #expect(try store.load()?.credentials != nil)
    try await coordinator.signOut()
    #expect(try store.load()?.credentials == nil)
    #expect(try store.load()?.subject == "fixture-subject")
}

@Test func chatGPTRefreshPreparationAndStorageFailureNeverConsumeRotatingToken() async throws {
    for prepareFailure in [false, true] {
        let original = authFixture()
        let store = AuthTestStore(original)
        let bridge = AuthTestProtocol(original, store: store)
        await bridge.configure(prepareFailure: prepareFailure)
        if !prepareFailure { store.rejectWrites() }
        let coordinator = ChatGPTCoordinator(store: store, bridge: bridge)
        do {
            try await coordinator.run(refresh: true)
            Issue.record("expected failure")
        } catch {}
        #expect(await bridge.posts == 0)
        #expect(try store.load()?.credentials?.refresh == "fixture-refresh")
    }
    let store = AuthTestStore(authFixture())
    let bridge = AuthTestProtocol(authFixture(), store: store)
    await bridge.configure(dispatchFailure: true)
    let coordinator = ChatGPTCoordinator(store: store, bridge: bridge)
    await #expect(throws: ChatGPTFailure.network) { try await coordinator.run(refresh: true) }
    #expect(try store.load()?.credentials == nil)
    await #expect(throws: ChatGPTFailure.invalid) { try await coordinator.run(refresh: true) }
    #expect(await bridge.posts == 1)
}

@Test func chatGPTLogoutAndShutdownFenceLateSuccessAndWaitForActualReturn() async throws {
    let store = AuthTestStore(authFixture())
    let bridge = AuthTestProtocol(authFixture(), store: store)
    await bridge.configure(hold: true)
    let coordinator = ChatGPTCoordinator(store: store, bridge: bridge)
    let operation = Task { try await coordinator.run(refresh: true) }
    await bridge.waitForDispatch()
    try await coordinator.signOut()
    #expect(try await coordinator.load() == .cancelling)
    await #expect(throws: ChatGPTFailure.busy) { try await coordinator.run(refresh: true) }
    let shutdown = Task { try await coordinator.shutdown() }
    #expect(try store.load()?.credentials == nil)
    await bridge.release()
    await #expect(throws: ChatGPTFailure.cancelled) { try await operation.value }
    try await shutdown.value
    #expect(try store.load()?.credentials == nil)
    #expect(try store.load()?.hint == nil)
    await #expect(throws: ChatGPTFailure.closed) { try await coordinator.run(refresh: true) }
}

@Test func chatGPTExpiryKeepsBusyUntilLateCallReturnsAndCannotCommit() async throws {
    let store = AuthTestStore(authFixture())
    let bridge = AuthTestProtocol(authFixture(), store: store)
    await bridge.configure(hold: true)
    let coordinator = ChatGPTCoordinator(
        store: store, bridge: bridge, operationTimeout: .milliseconds(200))
    let operation = Task { try await coordinator.run(refresh: true) }
    await bridge.waitForDispatch()
    try await Task.sleep(for: .milliseconds(260))
    #expect(try await coordinator.load() == .cancelling)
    await bridge.release()
    await #expect(throws: ChatGPTFailure.expired) { try await operation.value }
    #expect(try store.load()?.credentials == nil)
}

@Test func chatGPTIdleBrowserDeadlineReportsExpiryAndClosesListenerWithoutPost() async throws {
    let store = AuthTestStore(
        ChatGPTEnvelope(host: "urn:uuid:12345678-1234-4234-9234-123456789abc"))
    let bridge = AuthTestProtocol(authFixture(), store: store)
    let coordinator = ChatGPTCoordinator(
        store: store, bridge: bridge, operationTimeout: .milliseconds(500),
        openBrowser: { _ in true })
    await #expect(throws: ChatGPTFailure.expired) { try await coordinator.run(refresh: false) }
    #expect(await bridge.posts == 0)
    #expect(try store.load()?.credentials == nil)
    #expect(try await coordinator.load() == .unknown)
}

@Test func chatGPTDynamicRegistrationIsMarkedBeforeBrowserAndRequiresExplicitRecovery() async throws
{
    var original = authFixture()
    original.registration = 0
    original.clientID = nil
    original.issuer = nil
    original.subject = nil
    original.localSignOut()
    let store = AuthTestStore(original)
    let bridge = AuthTestProtocol(authFixture(), store: store)
    let coordinator = ChatGPTCoordinator(
        store: store, bridge: bridge, makeListener: { AuthTestListener() },
        openBrowser: { _ in
            let registration = try? store.load()?.registration
            #expect(registration == 1)
            return false
        })
    await #expect(throws: ChatGPTFailure.browser) { try await coordinator.run(refresh: false) }
    #expect(try store.load()?.registration == 0)
    var unknown = original
    unknown.registration = 1
    try store.save(unknown)
    #expect(try await coordinator.load() == .unknown)
    await #expect(throws: ChatGPTFailure.registrationUnknown) {
        try await coordinator.run(refresh: false)
    }
    try await coordinator.recoverRegistration()
    #expect(try store.load()?.host == original.host)
    #expect(try store.load()?.registration == 0)
}

@Test func chatGPTStorageErrorsAndCorruptionCannotCreateAnotherInstallation() async throws {
    for failure in [ChatGPTFailure.storage, .corrupt] {
        let store = AuthTestStore(nil, readFailure: failure)
        let bridge = AuthTestProtocol(authFixture(), store: store)
        let coordinator = ChatGPTCoordinator(
            store: store, bridge: bridge,
            openBrowser: { _ in
                Issue.record("unexpected browser")
                return false
            })
        await #expect(throws: failure) { try await coordinator.load() }
        await #expect(throws: failure) { try await coordinator.run(refresh: false) }
        #expect(store.saveCount == 0)
        #expect(await bridge.begins == 0)
    }
    let store = AuthTestStore(nil)
    let coordinator = ChatGPTCoordinator(store: store)
    #expect(try await coordinator.load() == .signedOut)
    #expect(store.saveCount == 0)
}

@Test func chatGPTShutdownCannotReportSuccessWhenHandleCloseFails() async throws {
    let store = AuthTestStore(authFixture())
    let bridge = AuthTestProtocol(authFixture(), store: store)
    await bridge.rejectClose(true)
    let coordinator = ChatGPTCoordinator(store: store, bridge: bridge)
    await #expect(throws: ChatGPTFailure.response) { try await coordinator.run(refresh: true) }
    await #expect(throws: ChatGPTFailure.response) { try await coordinator.shutdown() }
    await bridge.rejectClose(false)
    try await coordinator.shutdown()
}

@MainActor @Test func chatGPTModelLeavesBusyAfterStorageFailureAndCanRetry() async throws {
    let store = AuthTestStore(authFixture(), readFailure: .storage)
    let bridge = AuthTestProtocol(authFixture(), store: store)
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    // Saved-feed reloads use this test-owned database, including auth failure paths.
    let model = ChatGPTModel(
        coordinator: ChatGPTCoordinator(store: store, bridge: bridge, memory: memory))
    await model.connect()
    #expect(!model.busy)
    #expect(model.failure == .storage)
    store.setReadFailure(nil)
    await model.reload()
    #expect(model.state == .saved)
    await model.connect(refresh: true)
    #expect(model.state == .saved)
    #expect(model.failure == nil)
    #expect(await model.shutdown())
    try await memory.close()
}

@Test func chatGPTLoopbackClosesActualListenerAndReturnsStaticResponse() async throws {
    let listener = ChatGPTLoopback()
    let redirect = try await listener.start()
    let url = try #require(URL(string: redirect + "?state=fixture&code=fixture"))
    let request = Task { try await URLSession(configuration: .ephemeral).data(from: url) }
    #expect(try await listener.next() == url.absoluteString)
    await listener.respond()
    await listener.close()
    let (data, response) = try await request.value
    #expect(String(data: data, encoding: .utf8) == "You may return to Ambient.")
    #expect(
        (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Cache-Control") == "no-store")
    await listener.close()
    await listener.close()
}

@Test func chatGPTCallbackUsesSystemParserAndRejectsBodyHostAndOversize() throws {
    let expected = try #require(URL(string: "http://127.0.0.1:12345/auth/callback"))
    var p = ChatGPTCallbackParser(expected: expected)
    #expect(
        try p.append(
            Data("GET /auth/callback?state=s&code=c HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n".utf8))
            == nil)
    #expect(
        try p.append(Data("\r\n".utf8)) == "http://127.0.0.1:12345/auth/callback?state=s&code=c")
    for request in [
        "POST /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: localhost:12345\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\nContent-Length: 1\r\n\r\nx",
        "GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\nTransfer-Encoding: chunked\r\n\r\n",
        "GET http://other/auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\nx",
        "GET /auth/callback HTTP/1.1\r\nHost: other:12345\r\nHost: 127.0.0.1:12345\r\n\r\n",
        "GET /x/../auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
        "GET auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n",
        "GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:12345\r\nContent-Length: 1\r\nContent-Length: 0\r\n\r\n",
    ] {
        var invalid = ChatGPTCallbackParser(expected: expected)
        #expect(throws: ChatGPTFailure.invalid) { try invalid.append(Data(request.utf8)) }
    }
    var oversized = ChatGPTCallbackParser(expected: expected)
    #expect(throws: ChatGPTFailure.invalid) {
        try oversized.append(Data(repeating: 65, count: 8193))
    }
}

@Test func chatGPTKeychainUsesOnePrivateRecordAndSurvivesRestart() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-keychain-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }
    var keychain: Unmanaged<SecKeychain>?
    let password = Array(UUID().uuidString.utf8)
    let status = password.withUnsafeBytes { p in
        directory.appendingPathComponent("isolated.keychain").path.withCString { path in
            ambient_test_keychain_create(path, p.baseAddress, UInt32(p.count), &keychain)
        }
    }
    #expect(status == errSecSuccess)
    let privateKeychain = try #require(keychain).takeRetainedValue()
    defer { #expect(ambient_test_keychain_delete(privateKeychain) == errSecSuccess) }
    let store = ChatGPTKeychain(keychain: privateKeychain)
    #expect(try store.load() == nil)
    let e = ChatGPTEnvelope(host: "urn:uuid:12345678-1234-4234-9234-123456789abc")
    try store.save(e)
    #expect(try ChatGPTKeychain(keychain: privateKeychain).load()?.host == e.host)
    var pending = e
    pending.registration = 2
    pending.clientID = "issued_1"
    try store.save(pending)
    #expect(try store.load()?.clientID == "issued_1")
    let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "dev.ambient.foundation.chatgpt",
        kSecAttrAccount as String: "installation", kSecMatchSearchList as String: [privateKeychain],
        kSecAttrSynchronizable as String: false,
    ]
    #expect(
        SecItemUpdate(
            query as CFDictionary,
            [kSecValueData as String: Data("invalid fixture".utf8)] as CFDictionary)
            == errSecSuccess)
    #expect(throws: ChatGPTFailure.corrupt) { try store.load() }
    try store.save(pending)
    #expect(try store.load()?.host == e.host)
    try store.save(authFixture())
    let reopened = ChatGPTKeychain(keychain: privateKeychain)
    #expect(try reopened.load()?.credentials?.refresh == "fixture-refresh")
    let coordinator = ChatGPTCoordinator(
        store: reopened,
        openBrowser: { _ in
            Issue.record("unexpected browser after cached restart")
            return false
        })
    #expect(try await coordinator.load() == .saved)
    var all = query
    all[kSecMatchLimit as String] = kSecMatchLimitAll
    all[kSecReturnAttributes as String] = true
    var found: CFTypeRef?
    #expect(SecItemCopyMatching(all as CFDictionary, &found) == errSecSuccess)
    #expect((found as? [Any])?.count == 1)
}

/// The «Ambient Visual» build names its own ChatGPT service, so it never reads the daily record.
@Test @MainActor func chatGPTKeychainServiceSeparatesRecords() throws {
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let privateKeychain = fixture.keychain
    let daily = ChatGPTKeychain(keychain: privateKeychain)
    try daily.save(ChatGPTEnvelope(host: "urn:uuid:12345678-1234-4234-9234-123456789abc"))
    let visual = ChatGPTKeychain(
        service: "dev.ambient.foundation.visual.chatgpt", keychain: privateKeychain)
    #expect(try visual.load() == nil)
    let e = ChatGPTEnvelope(host: "urn:uuid:87654321-4321-4321-8321-cba987654321")
    try visual.save(e)
    #expect(try visual.load()?.host == e.host)
    #expect(try daily.load()?.host == "urn:uuid:12345678-1234-4234-9234-123456789abc")
}

@Test func chatGPTEnvelopeRejectsCorruptionAndRetainsRegistrationAfterLocalSignOut() throws {
    var e = ChatGPTEnvelope(host: "urn:uuid:12345678-1234-4234-9234-123456789abc")
    let decoded = try ChatGPTEnvelope.decode(e.encoded())
    #expect(decoded.host == e.host)
    e.registration = 3
    #expect(throws: ChatGPTFailure.corrupt) { try e.validate() }
    e.clientID = "issued_1"
    e.issuer = "https://auth.openai.com"
    e.subject = "fixture-subject"
    e.hint = "fixture-hint"
    e.nonce = "fixture-nonce"
    e.credentials = ChatGPTCredentials(
        access: "fixture-access", refresh: "fixture-refresh",
        scopes: ["openid", "resource.invoke", "chatgpt.tokens.use.direct"], expiresAt: 2_000_000_000
    )
    try e.validate()
    e.localSignOut()
    try e.validate()
    #expect(e.registration == 3)
    #expect(e.subject == "fixture-subject")
    #expect(e.credentials == nil)
    #expect(e.hint == nil)
    #expect(throws: ChatGPTFailure.corrupt) {
        try ChatGPTEnvelope.decode(Data(repeating: 0, count: 65_537))
    }
}

@Test func creditReportSurvivesOnlyVerifiedSameIdentityRefreshAndClearsOnNewLogin() async throws {
    let store = AuthTestStore(authFixture())
    let bridge = AuthTestProtocol(authFixture(), store: store)
    let coordinator = ChatGPTCoordinator(
        store: store, bridge: bridge, makeListener: { AuthTestListener() },
        openBrowser: { _ in false })
    try await coordinator.setCreditReport(true)
    try await coordinator.run(refresh: true)
    #expect(await coordinator.creditReport())
    await #expect(throws: ChatGPTFailure.browser) { try await coordinator.run(refresh: false) }
    #expect(await coordinator.creditReport() == false)
    var different = authFixture()
    different.subject = "other-subject"
    let changed = AuthTestProtocol(different, store: store)
    let another = ChatGPTCoordinator(store: store, bridge: changed)
    try await another.setCreditReport(true)
    await #expect(throws: ChatGPTFailure.identity) { try await another.run(refresh: true) }
    #expect(await another.creditReport() == false)
    #expect(try store.load()?.credentials == nil)
}
