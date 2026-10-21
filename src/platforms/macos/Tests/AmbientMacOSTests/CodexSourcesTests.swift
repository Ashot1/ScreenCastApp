import CAmbient
import Darwin
import Foundation
import Testing

@testable import AmbientMacOS

struct CodexFixture: Sendable {
    let directory: URL
    let file: URL
    init() throws {
        directory = URL(fileURLWithPath: "/private/tmp/ambient-codex-" + UUID().uuidString)
        file = directory.appendingPathComponent("selected.jsonl")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try Data(Self.header.utf8).write(to: file)
    }
    static let header =
        "{\"timestamp\":\"2026-10-20T08:00:00.000Z\",\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{\"id\":\"thread-one\",\"cli_version\":\"0.163.0-alpha.10.2\",\"history_mode\":\"paginated\"}}\n"
    static func item(_ ordinal: Int, text: String = "exact prompt") throws -> Data {
        var bytes = try JSONSerialization.data(withJSONObject: [
            "timestamp": "2026-10-20T08:00:01.000Z", "ordinal": ordinal, "type": "event_msg",
            "payload": [
                "type": "item_completed", "thread_id": "thread-one", "turn_id": "turn-one",
                "item": [
                    "type": "UserMessage", "id": "item-\(ordinal)",
                    "content": [["type": "text", "text": text]],
                ],
            ],
        ])
        bytes.append(10)
        return bytes
    }
    func remove() { try? FileManager.default.removeItem(at: directory) }
}
@Test func codexSelectionSettingsAndClosedGateNeverReadContent() async throws {
    let fixture = try CodexFixture()
    defer { fixture.remove() }
    let before = try Data(contentsOf: fixture.file)
    let memory = MemorySession(
        databaseURL: fixture.directory.appendingPathComponent("memory.sqlite3"))
    let reader = CodexReader()
    let path = try await reader.select(fixture.file)
    let selection = try await memory.saveCodexSelection(id: 0, expected: 0, path: path)
    let task = try await memory.createTask("intent")
    await #expect(throws: ModelCheckFailure.session) {
        _ = try await memory.beginCodexRead(
            task, selection: selection, cursor: 0, expiresAt: Int64.max)
    }
    #expect(try Data(contentsOf: fixture.file) == before)
    #expect(try await memory.codexSelections() == [selection])
    let link = fixture.directory.appendingPathComponent("link.jsonl")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: fixture.file)
    await #expect(throws: CodexSourceFailure.changed) { _ = try await reader.select(link) }
    await #expect(throws: CodexSourceFailure.changed) {
        _ = try await reader.select(fixture.directory)
    }
    try await memory.close()
}
#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func sourceSettingsClearBothWindowsAndPreserveIndependentUserReply()
        async throws
    {
        let codex = try CodexFixture()
        defer { codex.remove() }
        let git = try LocalGitFixture()
        defer { git.remove() }
        var data = Data(CodexFixture.header.utf8)
        data.append(try CodexFixture.item(1))
        try data.write(to: codex.file)
        let memory = MemorySession(
            databaseURL: codex.directory.appendingPathComponent("memory.sqlite3"))
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let coordinator = try await localReady(memory: memory, server: server)
        let chat = ChatGPTModel(coordinator: coordinator)
        let task = try await memory.createTask("intent")
        let editor = AssistantModel(session: memory)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        chat.updateReply("keep my user text")
        await chat.waitForReplyPreview()
        #expect(chat.canSendReply)
        await editor.localProjects.select(git.root)
        let setting = try await memory.saveCodexSelection(id: 0, expected: 0, path: codex.file.path)
        editor.codexSources.use(setting)
        await editor.codexSources.read(task: task, chatGPT: chat)
        let event = try #require(editor.codexSources.observation?.events.first)
        await editor.codexSources.selectEvent(event)
        #expect(editor.codexSources.preview != nil)
        await editor.saveLocalSourceSettings(chatGPT: chat)
        #expect(editor.codexSources.observation == nil && editor.codexSources.preview == nil)
        #expect(chat.replyDraft == "keep my user text" && !chat.canSendReply)
        await editor.localProjects.scan(task: task, chatGPT: chat)
        let branch = try #require(editor.localProjects.observation?.branches.first)
        await editor.localProjects.selectBranch(branch)
        #expect(editor.localProjects.preview != nil)
        await editor.saveCodexSourceSettings(chatGPT: chat)
        #expect(editor.localProjects.observation == nil && editor.localProjects.preview == nil)
        #expect(chat.replyDraft == "keep my user text")
        try await coordinator.shutdown()
        #expect(await editor.close())
    }
    @Test @MainActor func lateSourcePreviewsCannotRestoreCancelledWindows() async throws {
        let fixture = try CodexFixture()
        defer { fixture.remove() }
        let git = try LocalGitFixture()
        defer { git.remove() }
        var data = Data(CodexFixture.header.utf8)
        data.append(try CodexFixture.item(1))
        try data.write(to: fixture.file)
        let memory = MemorySession(
            databaseURL: fixture.directory.appendingPathComponent("memory.sqlite3"))
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let coordinator = try await localReady(memory: memory, server: server)
        let chat = ChatGPTModel(coordinator: coordinator)
        let task = try await memory.createTask("intent")
        let setting = try await memory.saveCodexSelection(
            id: 0, expected: 0, path: fixture.file.path)
        var codex: CodexSourcesModel?
        let source = CodexSourcesModel(memory: memory, testAfterPreview: { codex?.cancel() })
        codex = source
        source.use(setting)
        await source.read(task: task, chatGPT: chat)
        let event = try #require(source.observation?.events.first)
        await source.selectEvent(event)
        #expect(source.preview == nil && source.selectedIndex == nil && source.observation == nil)
        var local: LocalProjectsModel?
        let projects = LocalProjectsModel(memory: memory, testAfterPreview: { local?.cancel() })
        local = projects
        await projects.select(git.root)
        await projects.saveScope(chatGPT: chat)
        await projects.scan(task: task, chatGPT: chat)
        let branch = try #require(projects.observation?.branches.first)
        await projects.selectBranch(branch)
        #expect(
            projects.preview == nil && projects.selectedIndex == nil && projects.observation == nil)
        codex = nil
        local = nil
        await source.close()
        await projects.close()
        try await coordinator.shutdown()
        try await memory.close()
    }
    final class CodexReadProbe: @unchecked Sendable {
        private let condition = NSCondition()
        private var counts: [String: Int] = [:]
        private var blocked = false
        private var opened = false
        private var descriptor: Int32 = -1
        private var operation: CodexOperation?
        private var closed: [Int32] = []
        private var released: [UInt32] = []
        let hold: Bool
        init(hold: Bool = false) { self.hold = hold }
        func read(_ fd: Int32, _ stage: String, _ count: Int) {
            condition.lock()
            defer { condition.unlock() }
            counts[stage, default: 0] += count
            if hold && stage == "window" && !opened {
                descriptor = fd
                blocked = true
                condition.broadcast()
                while !opened { condition.wait() }
            }
        }
        func total(_ stage: String) -> Int { condition.withLock { counts[stage, default: 0] } }
        var fd: Int32 { condition.withLock { descriptor } }
        func attach(_ op: CodexOperation) { condition.withLock { operation = op } }
        func close(_ result: Int32) {
            let op = condition.withLock { operation }
            let release = op.map { ambient_codex_v1_release($0.memory, $0.id) } ?? 99
            condition.withLock {
                closed.append(result)
                released.append(release)
            }
        }
        var closeResults: [Int32] { condition.withLock { closed } }
        var releaseResults: [UInt32] { condition.withLock { released } }
        func waitUntilBlocked() async throws {
            let deadline = ContinuousClock.now + .seconds(5)
            while !condition.withLock({ blocked }) {
                guard ContinuousClock.now < deadline else { throw ModelCheckFailure.native }
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func release() {
            condition.lock()
            opened = true
            condition.broadcast()
            condition.unlock()
        }
    }
    @Test @MainActor func codexNativeFDStoreHTTPResultCopyAndRevocationUseExactSelectedEvidence()
        async throws
    {
        let fixture = try CodexFixture()
        defer { fixture.remove() }
        var bytes = Data(CodexFixture.header.utf8)
        bytes.append(try CodexFixture.item(1, text: "exact\nCodex 🌿 prompt"))
        try bytes.write(to: fixture.file)
        let server = try AssistanceHTTPServer(mode: "codex")
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.directory.appendingPathComponent("memory.sqlite3"))
        let reader = CodexReader()
        let selection = try await memory.saveCodexSelection(
            id: 0, expected: 0, path: reader.select(fixture.file))
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("intent")
        let op = try await coordinator.beginCodexRead(task, selection: selection, cursor: 0)
        let window = try await reader.read(op)
        #expect(window.events.count == 1 && window.reasons == 0 && window.cursor == 0)
        #expect(window.events[0].kind == 1 && window.events[0].text == "exact\nCodex 🌿 prompt")
        #expect(try await memory.codexPreview(op, index: 0) == "exact\nCodex 🌿 prompt")
        let evidenceID = try await coordinator.bindCodexEvent(op, index: 0)
        try op.release()
        let current = try await memory.task(task.id)
        let evidence = try #require(try await memory.evidence(taskID: task.id).items.first)
        #expect(evidence.assistanceSelectable)
        let preview = try await coordinator.prepareAssistance(current, notes: [evidenceID])
        let publicationResult = try await coordinator.runAssistance(preview.requestID)
        let result = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == publicationResult.publishedTaskID
            })
        #expect(result.citations.map(\.id) == ["source:\(evidenceID)"])
        #expect(try await coordinator.copyProposal(result.target) == "Copy exact 🌿 saved text")
        let metadata = try await memory.codexMetadata(taskID: task.id)
        #expect(
            metadata.count == 1 && metadata[0].evidenceID == evidenceID && metadata[0].kind == 1)
        #expect(try Data(contentsOf: fixture.file) == bytes)
        try await memory.deleteCodexSelection(selection)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.copyProposal(result.target)
        }
        #expect(try await memory.evidence(taskID: task.id).items[0].text == "exact\nCodex 🌿 prompt")
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func codexLargeFilePagesPhysicalLinesAndRefreshesOnlyObservedBytes()
        async throws
    {
        let fixture = try CodexFixture()
        defer { fixture.remove() }
        var bytes = Data(CodexFixture.header.utf8)
        for n in 1...20000 {
            bytes.append(
                Data(
                    "{\"ordinal\":\(n),\"type\":\"world_state\",\"payload\":{\"ignored\":\"\(String(repeating:"x",count:80))\"}}\n"
                        .utf8))
        }
        bytes.append(try CodexFixture.item(20001))
        try bytes.write(to: fixture.file)
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.directory.appendingPathComponent("memory.sqlite3"))
        let probe = CodexReadProbe()
        let reader = CodexReader(
            testAfterRead: { probe.read($0, $1, $2) }, testAfterClose: { probe.close($0) })
        let setting = try await memory.saveCodexSelection(
            id: 0, expected: 0, path: reader.select(fixture.file))
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("intent")
        let op = try await coordinator.beginCodexRead(task, selection: setting, cursor: 0)
        let first = try await reader.read(op)
        #expect(first.events.map(\.ordinal) == [20001])
        #expect(first.cursor > 0 && first.physicalLines == 4096)
        #expect(probe.total("window") == 1_048_577)
        #expect(probe.total("header") <= 262145 && probe.total("header-check") <= 262145)
        let previous = try await coordinator.beginCodexRead(
            task, selection: setting, cursor: first.cursor)
        let earlier = try await reader.read(previous)
        #expect(
            earlier.events.isEmpty && earlier.nextBefore < first.nextBefore && earlier.cursor > 0)
        try previous.release()
        let writer = try FileHandle(forWritingTo: fixture.file)
        try writer.seekToEnd()
        let added = try CodexFixture.item(20002, text: "new saved text")
        try writer.write(contentsOf: added.dropLast())
        try writer.close()
        let partial = try await coordinator.beginCodexRead(task, selection: setting, cursor: 0)
        let partialWindow = try await reader.read(partial)
        #expect(!partialWindow.events.contains { $0.ordinal == 20002 })
        #expect(partialWindow.reasons & 1 != 0)
        try partial.release()
        let append = try FileHandle(forWritingTo: fixture.file)
        try append.seekToEnd()
        try append.write(contentsOf: Data([10]))
        try append.close()
        let refreshed = try await coordinator.beginCodexRead(task, selection: setting, cursor: 0)
        let refreshedWindow = try await reader.read(refreshed)
        #expect(refreshedWindow.events.last?.text == "new saved text")
        try refreshed.release()
        var expected = bytes
        expected.append(added)
        #expect(try Data(contentsOf: fixture.file) == expected)
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func codexCancellationAndFileRacesKeepSlotUntilActualFDReturn() async throws {
        for action in [
            "cancel", "pause", "delete-setting", "session", "replace", "truncate", "header",
        ] {
            let fixture = try CodexFixture()
            defer { fixture.remove() }
            var data = Data(CodexFixture.header.utf8)
            data.append(try CodexFixture.item(1))
            try data.write(to: fixture.file)
            let server = try AssistanceHTTPServer()
            defer { server.stop() }
            let memory = MemorySession(
                databaseURL: fixture.directory.appendingPathComponent("memory.sqlite3"))
            let probe = CodexReadProbe(hold: true)
            let reader = CodexReader(
                testAfterRead: { probe.read($0, $1, $2) }, testAfterClose: { probe.close($0) })
            let setting = try await memory.saveCodexSelection(
                id: 0, expected: 0, path: reader.select(fixture.file))
            let coordinator = try await localReady(memory: memory, server: server)
            let task = try await memory.createTask("intent")
            let op = try await coordinator.beginCodexRead(task, selection: setting, cursor: 0)
            probe.attach(op)
            let read = Task { try await reader.read(op) }
            try await probe.waitUntilBlocked()
            #expect(fcntl(probe.fd, F_GETFD) >= 0)
            #expect(ambient_codex_v1_release(op.memory, op.id) == 4)
            switch action {
            case "cancel": op.cancel()
            case "pause": try await memory.setManualPause(true)
            case "delete-setting": try await memory.deleteCodexSelection(setting)
            case "session":
                try ModelCheckBridge.check(ambient_model_v6_session(op.memory, 999, 0, 1))
            case "replace":
                try FileManager.default.moveItem(
                    at: fixture.file, to: fixture.directory.appendingPathComponent("old.jsonl"))
                try data.write(to: fixture.file)
            case "truncate": try Data(CodexFixture.header.utf8).write(to: fixture.file)
            default:
                var changed = data
                changed[changed.firstIndex(of: 116) ?? 0] = 120
                try changed.write(to: fixture.file)
            }
            #expect(ambient_codex_v1_release(op.memory, op.id) == 4)
            probe.release()
            let result = await read.result
            if case .success = result { Issue.record("Late read accepted after \(action)") }
            #expect(probe.closeResults == [0])
            #expect(probe.releaseResults == [4])
            try op.release()
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            try await coordinator.shutdown()
            try await memory.close()
        }
    }
#endif
