import CAmbient
import Darwin
import Foundation
import Testing

@testable import AmbientMacOS

struct LocalGitFixture: Sendable {
    let container: URL
    let root: URL
    init() throws {
        container = URL(fileURLWithPath: "/private/tmp/ambient-local-" + UUID().uuidString)
        root = container.appendingPathComponent("project")
        try FileManager.default.createDirectory(
            at: root.appendingPathComponent(".git/refs/heads"), withIntermediateDirectories: true)
        try write(".git/config", "[core]\nrepositoryformatversion=0\n")
        try write(".git/HEAD", "ref: refs/heads/main\n")
        try write(".git/refs/heads/main", String(repeating: "2", count: 40) + "\n")
        try write(".git/packed-refs", String(repeating: "1", count: 40) + " refs/heads/main\n")
        try write("work.txt", "unread workfile")
        try write(".git/index", "unread index")
        try FileManager.default.createDirectory(
            at: root.appendingPathComponent(".git/objects"), withIntermediateDirectories: true)
        try write(".git/objects/unread", "unread object")
    }
    func write(_ path: String, _ text: String) throws {
        try Data(text.utf8).write(to: root.appendingPathComponent(path))
    }
    func remove() { try? FileManager.default.removeItem(at: container) }
    struct Entry: Equatable {
        let bytes: Data?
        let device: dev_t
        let inode: ino_t
        let mode: mode_t
        let mtimeSeconds: Int
        let mtimeNanos: Int
    }
    func snapshot() throws -> [String: Entry] {
        var result: [String: Entry] = [:]
        func visit(_ url: URL, _ relative: String) throws {
            var info = stat()
            guard lstat(url.path, &info) == 0 else { throw ModelCheckFailure.native }
            let isFile = info.st_mode & S_IFMT == S_IFREG
            result[relative] = Entry(
                bytes: isFile ? try Data(contentsOf: url) : nil, device: info.st_dev,
                inode: info.st_ino, mode: info.st_mode,
                mtimeSeconds: info.st_mtimespec.tv_sec, mtimeNanos: info.st_mtimespec.tv_nsec)
            if info.st_mode & S_IFMT == S_IFDIR {
                for item in try FileManager.default.contentsOfDirectory(atPath: url.path) {
                    try visit(url.appendingPathComponent(item), relative + "/" + item)
                }
            }
        }
        try visit(root, "")
        return result
    }
}
@Test func localSourceSettingsAndClosedGateUseSameStoreWithoutReadingGit() async throws {
    let fixture = try LocalGitFixture()
    defer { fixture.remove() }
    let before = try fixture.snapshot()
    let memory = MemorySession(
        databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
    let reader = LocalGitReader()
    let actual = try await reader.select(fixture.root)
    let root = try await memory.saveLocalRoot(
        path: actual, expected: 0, exclusions: [".git/refs/heads/private"])
    #expect(try await memory.localRoot(path: actual) == root)
    let task = try await memory.createTask("saved intent")
    await #expect(throws: ModelCheckFailure.session) {
        _ = try await memory.beginLocalRead(task, root: root, expiresAt: Int64.max)
    }
    #expect(try fixture.snapshot() == before)
    try await memory.close()
    let reopened = MemorySession(
        databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
    #expect(try await reopened.localRoot(path: actual) == root)
    #expect(try await reopened.assistanceAvailable() == false)
    try await reopened.close()
    await reader.clear()
}
@Test func localSourceSelectionRejectsSymlinkComponentsWithoutFollowingThem() async throws {
    let fixture = try LocalGitFixture()
    defer { fixture.remove() }
    let link = fixture.container.appendingPathComponent("alias")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: fixture.root)
    let reader = LocalGitReader()
    await #expect(throws: LocalSourceFailure.root) { _ = try await reader.select(link) }
    await #expect(throws: LocalSourceFailure.root) {
        _ = try await reader.select(link.appendingPathComponent(".git"))
    }
    #expect(try await reader.select(fixture.root) == fixture.root.path)
    await reader.clear()
}
#if AMBIENT_MODEL_TEST_SUPPORT
    import Dispatch

    /// Test-only synchronous gate sits after a real read; no production callback or fake filesystem.
    final class LocalReadGate: @unchecked Sendable {
        private let condition = NSCondition()
        private var paths: [String] = []
        private var blocked = false
        private var opened = false
        let hold: Bool
        init(hold: Bool = false) { self.hold = hold }
        func read(_ path: String) {
            condition.lock()
            paths.append(path)
            if hold && !opened {
                blocked = true
                condition.broadcast()
                while !opened { condition.wait() }
            }
            condition.unlock()
        }
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
        var reads: [String] { condition.withLock { paths } }
    }
    @MainActor func localReady(
        memory: MemorySession, server: AssistanceHTTPServer,
        store: AssistanceTestStore = AssistanceTestStore()
    ) async throws -> ChatGPTCoordinator {
        let coordinator = ChatGPTCoordinator(
            store: store, memory: memory, modelBridge: LocalAssistanceBridge(url: server.url))
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        return coordinator
    }
    @Test @MainActor func localBranchesRunNativeFDCoreSQLiteHTTPAndGroundedCopyWithoutSourceWrites()
        async throws
    {
        let fixture = try LocalGitFixture()
        defer { fixture.remove() }
        let before = try fixture.snapshot()
        let server = try AssistanceHTTPServer(mode: "branch")
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
        let gate = LocalReadGate()
        let reader = LocalGitReader(testAfterRead: { gate.read($0) })
        let root = try await memory.saveLocalRoot(
            path: reader.select(fixture.root), expected: 0, exclusions: [])
        let coordinator = try await localReady(memory: memory, server: server)
        let first = try await memory.createTask("finish selected work")
        _ = try await memory.addNote(to: first, text: "saved manual note", at: 1)
        let task = try await memory.task(first.id)
        let note = try #require(try await memory.evidence(taskID: task.id).items.first)
        let op = try await coordinator.beginLocalRead(task, root: root)
        let observed = try await reader.read(op)
        #expect(observed.completeness == .complete && observed.reasons == 0)
        #expect(observed.branches.count == 1 && observed.branches[0].head == 1)
        #expect(observed.branches[0].oid == String(repeating: "2", count: 40))
        let exact = try await memory.localPreview(op, index: 0)
        #expect(exact.contains("Dated local Git branch observation"))
        let evidence = try await coordinator.bindLocalBranch(op, index: 0)
        try op.release()
        let current = try await memory.task(task.id)
        let preview = try await coordinator.prepareAssistance(current, notes: [note.id, evidence])
        #expect(preview.context.contains(exact.replacingOccurrences(of: "\n", with: "\\n")))
        let publicationResult = try await coordinator.runAssistance(preview.requestID)
        let result = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == publicationResult.publishedTaskID
            })
        #expect(Set(result.citations.map(\.id)) == ["source:\(evidence)", "note:\(note.id)"])
        #expect(result.citations.first?.kind == "source")
        #expect(try await coordinator.copyProposal(result.target) == "Copy exact 🌿 saved text")
        #expect(
            Set(gate.reads) == [
                ".git/config", ".git/HEAD", ".git/packed-refs", ".git/refs/heads/main",
            ])
        #expect(try fixture.snapshot() == before)
        try await memory.deleteLocalRoot(root)
        _ = try await memory.saveLocalRoot(path: root.path, expected: 0, exclusions: [])
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.copyProposal(result.target)
        }
        #expect(try await memory.evidence(taskID: task.id).items.count == 2)
        try await coordinator.shutdown()
        await reader.clear()
        try await memory.close()
    }
    @Test @MainActor func sourceExclusionsSkipPackedAndLogicalBranchesBeforeAnyRead() async throws {
        let fixture = try LocalGitFixture()
        defer { fixture.remove() }
        try FileManager.default.createDirectory(
            at: fixture.root.appendingPathComponent(".git/refs/heads/private"),
            withIntermediateDirectories: true)
        try fixture.write(".git/refs/heads/private/secret", String(repeating: "3", count: 40))
        try fixture.write(".git/HEAD", "ref: refs/heads/private/secret\n")
        try fixture.write(".git/packed-refs", "not parsed because excluded shared input")
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
        let gate = LocalReadGate()
        let reader = LocalGitReader(testAfterRead: { gate.read($0) })
        let root = try await memory.saveLocalRoot(
            path: reader.select(fixture.root), expected: 0, exclusions: [".git/refs/heads/private"])
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("task")
        let op = try await coordinator.beginLocalRead(task, root: root)
        let observed = try await reader.read(op)
        #expect(observed.completeness == .partial && observed.reasons & 1 != 0)
        #expect(observed.branches.map(\.fullRef) == ["refs/heads/main"])
        #expect(observed.branches[0].head == 0)
        #expect(!gate.reads.contains(".git/packed-refs"))
        #expect(!gate.reads.contains { $0.contains("private") })
        let text = try await memory.localPreview(op, index: 0)
        #expect(!text.contains("secret"))
        try op.release()
        try await coordinator.shutdown()
        await reader.clear()
        try await memory.close()
    }
    @Test @MainActor func cancelAndPauseKeepBusyUntilRealReadReturnsAndCannotImportLate()
        async throws
    {
        for paused in [false, true] {
            let fixture = try LocalGitFixture()
            defer { fixture.remove() }
            let server = try AssistanceHTTPServer()
            defer { server.stop() }
            let memory = MemorySession(
                databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
            let gate = LocalReadGate(hold: true)
            defer { gate.release() }
            let reader = LocalGitReader(testAfterRead: { gate.read($0) })
            let root = try await memory.saveLocalRoot(
                path: reader.select(fixture.root), expected: 0, exclusions: [])
            let coordinator = try await localReady(memory: memory, server: server)
            let task = try await memory.createTask("task")
            let op = try await coordinator.beginLocalRead(task, root: root)
            let reading = Task { try await reader.read(op) }
            try await gate.waitUntilBlocked()
            if paused { try await memory.setManualPause(true) } else { op.cancel() }
            #expect(ambient_source_v1_release(op.memory, op.id) == 4)
            await #expect(throws: ModelCheckFailure.busy) {
                _ = try await memory.beginLocalRead(task, root: root, expiresAt: Int64.max)
            }
            gate.release()
            await #expect(throws: paused ? ModelCheckFailure.paused : ModelCheckFailure.cancelled) {
                _ = try await reading.value
            }
            await #expect(throws: paused ? ModelCheckFailure.paused : ModelCheckFailure.cancelled) {
                _ = try await memory.bindLocalBranch(op, index: 0)
            }
            try op.release()
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            try await coordinator.shutdown()
            await reader.clear()
            try await memory.close()
        }
    }
    @Test @MainActor func sourceBeginUsesCurrentNativeSessionInsteadOfPriorAvailability()
        async throws
    {
        let fixture = try LocalGitFixture()
        defer { fixture.remove() }
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
        let trace = LocalReadGate()
        let reader = LocalGitReader(testAfterRead: { trace.read($0) })
        let root = try await memory.saveLocalRoot(
            path: reader.select(fixture.root), expected: 0, exclusions: [])
        let credentials = AssistanceTestStore()
        let coordinator = try await localReady(memory: memory, server: server, store: credentials)
        let task = try await memory.createTask("task")
        #expect(try await coordinator.assistanceAvailable())
        credentials.expireForTest()
        await #expect(throws: ModelCheckFailure.session) {
            _ = try await coordinator.beginLocalRead(task, root: root)
        }
        #expect(try await memory.assistanceAvailable() == false)
        #expect(trace.reads.isEmpty)
        #expect(try await memory.checkSummary().attempts == 1)
        try await coordinator.shutdown()
        await #expect(throws: ModelCheckFailure.closed) {
            _ = try await coordinator.beginLocalRead(task, root: root)
        }
        await reader.clear()
        try await memory.close()
    }
    @Test @MainActor func nativeReadDetectsRootRemovalReplacementAndChangesDuringRealRead()
        async throws
    {
        for mode in ["removed", "replacement", "file-change", "root-move"] {
            let fixture = try LocalGitFixture()
            defer { fixture.remove() }
            let server = try AssistanceHTTPServer()
            defer { server.stop() }
            let memory = MemorySession(
                databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
            let gate = LocalReadGate(hold: mode == "file-change" || mode == "root-move")
            defer { gate.release() }
            let reader = LocalGitReader(testAfterRead: { gate.read($0) })
            let root = try await memory.saveLocalRoot(
                path: reader.select(fixture.root), expected: 0, exclusions: [])
            let coordinator = try await localReady(memory: memory, server: server)
            let task = try await memory.createTask("task")
            let op = try await coordinator.beginLocalRead(task, root: root)
            if mode == "removed" || mode == "replacement" {
                let fd = Darwin.open(
                    fixture.root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                #expect(fd >= 0)
                defer { if fd >= 0 { Darwin.close(fd) } }
                try FileManager.default.removeItem(at: fixture.root)
                var stale = [CChar](repeating: 0, count: Int(MAXPATHLEN))
                let status = stale.withUnsafeMutableBufferPointer { pointer -> Int32 in
                    guard let base = pointer.baseAddress else { return -1 }
                    return fcntl(fd, F_GETPATH, base)
                }
                // Current macOS returns the removed name successfully; either OS result must still reject reading.
                if status == 0 {
                    #expect(
                        String(
                            bytes: stale.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) },
                            encoding: .utf8) == root.path)
                }
                if mode == "replacement" {
                    try FileManager.default.createDirectory(
                        at: fixture.root, withIntermediateDirectories: true)
                }
                await #expect(throws: (any Error).self) { _ = try await reader.read(op) }
                #expect(gate.reads.isEmpty)
            } else {
                let reading = Task { try await reader.read(op) }
                try await gate.waitUntilBlocked()
                if mode == "file-change" {
                    try fixture.write(".git/config", "[core]\nrepositoryformatversion=1\n")
                } else {
                    try FileManager.default.moveItem(
                        at: fixture.root, to: fixture.container.appendingPathComponent("moved"))
                    try FileManager.default.createDirectory(
                        at: fixture.root, withIntermediateDirectories: true)
                }
                gate.release()
                let observation = try await reading.value
                #expect(observation.completeness == .partial && observation.reasons & 64 != 0)
                #expect(observation.branches.isEmpty)
            }
            await #expect(throws: (any Error).self) {
                _ = try await coordinator.bindLocalBranch(op, index: 0)
            }
            try op.release()
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            try await coordinator.shutdown()
            await reader.clear()
            try await memory.close()
        }
    }
    @Test @MainActor func unsupportedLinksFIFOAndOversizeStayUnreadAndNeverLookComplete()
        async throws
    {
        for mode in [
            "gitdir", "commondir", "include", "config-link", "hardlink", "fifo", "oversize",
            "invalid-name",
        ] {
            let fixture = try LocalGitFixture()
            defer { fixture.remove() }
            let outside = fixture.container.appendingPathComponent("outside")
            try Data("never read this target".utf8).write(to: outside)
            switch mode {
            case "gitdir":
                try FileManager.default.moveItem(
                    at: fixture.root.appendingPathComponent(".git"),
                    to: fixture.root.appendingPathComponent(".metadata"))
                try fixture.write(".git", "gitdir: " + outside.path)
            case "commondir": try fixture.write(".git/commondir", outside.path)
            case "include":
                try fixture.write(
                    ".git/config",
                    "[core]\nrepositoryformatversion=0\n[include]\npath=" + outside.path + "\n")
            case "config-link":
                try FileManager.default.removeItem(
                    at: fixture.root.appendingPathComponent(".git/config"))
                try FileManager.default.createSymbolicLink(
                    at: fixture.root.appendingPathComponent(".git/config"),
                    withDestinationURL: outside)
            case "hardlink":
                try FileManager.default.linkItem(
                    at: fixture.root.appendingPathComponent(".git/refs/heads/main"),
                    to: outside.appendingPathExtension("link"))
            case "fifo":
                try FileManager.default.removeItem(
                    at: fixture.root.appendingPathComponent(".git/refs/heads/main"))
                #expect(
                    mkfifo(fixture.root.appendingPathComponent(".git/refs/heads/main").path, 0o600)
                        == 0)
            case "oversize": try fixture.write(".git/config", String(repeating: "x", count: 65537))
            default:
                try fixture.write(".git/refs/heads/bad.lock", String(repeating: "3", count: 40))
            }
            let before = try fixture.snapshot()
            let server = try AssistanceHTTPServer()
            defer { server.stop() }
            let memory = MemorySession(
                databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
            let trace = LocalReadGate()
            let reader = LocalGitReader(testAfterRead: { trace.read($0) })
            let root = try await memory.saveLocalRoot(
                path: reader.select(fixture.root), expected: 0, exclusions: [])
            let coordinator = try await localReady(memory: memory, server: server)
            let task = try await memory.createTask("task")
            let op = try await coordinator.beginLocalRead(task, root: root)
            let observation = try await reader.read(op)
            #expect(observation.completeness == .partial && observation.reasons != 0)
            if mode == "gitdir" || mode == "commondir" { #expect(trace.reads.isEmpty) }
            if mode == "config-link" || mode == "oversize" {
                #expect(!trace.reads.contains(".git/config"))
            }
            if mode == "hardlink" || mode == "fifo" {
                #expect(!trace.reads.contains(".git/refs/heads/main"))
                #expect(observation.branches.isEmpty)
            }
            #expect(!trace.reads.contains(".git/refs/heads/bad.lock"))
            #expect(try Data(contentsOf: outside) == Data("never read this target".utf8))
            #expect(try fixture.snapshot() == before)
            try op.release()
            try await coordinator.shutdown()
            await reader.clear()
            try await memory.close()
        }
    }
    @Test @MainActor func nativeResourceLimitsReturnPartialWithoutOversizeInputOrSourceWrites()
        async throws
    {
        for mode in ["depth", "files", "directories", "entries", "bytes"] {
            let fixture = try LocalGitFixture()
            defer { fixture.remove() }
            let heads = fixture.root.appendingPathComponent(".git/refs/heads")
            if mode == "depth" {
                let path = (0..<17).map { "d\($0)" }.joined(separator: "/")
                try FileManager.default.createDirectory(
                    at: heads.appendingPathComponent(path), withIntermediateDirectories: true)
                try fixture.write(
                    ".git/refs/heads/" + path + "/tip", String(repeating: "3", count: 40))
            } else if mode == "directories" || mode == "entries" {
                for index in 0..<(mode == "entries" ? 4100 : 260) {
                    try FileManager.default.createDirectory(
                        at: heads.appendingPathComponent("d\(index)"),
                        withIntermediateDirectories: true)
                }
            } else {
                let count = mode == "files" ? 260 : 200
                for index in 0..<count {
                    try fixture.write(
                        ".git/refs/heads/b\(index)",
                        String(repeating: "3", count: 40) + "\n"
                            + String(repeating: " ", count: mode == "bytes" ? 4055 : 0))
                }
                if mode == "bytes" {
                    let packed = (0..<4096).map {
                        String(repeating: "1", count: 40) + " refs/tags/t"
                            + String(format: "%04d", $0) + String(repeating: "x", count: 70) + "\n"
                    }.joined()
                    try fixture.write(".git/packed-refs", packed)
                }
            }
            let before = try fixture.snapshot()
            let server = try AssistanceHTTPServer()
            defer { server.stop() }
            let memory = MemorySession(
                databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
            let reader = LocalGitReader()
            let root = try await memory.saveLocalRoot(
                path: reader.select(fixture.root), expected: 0, exclusions: [])
            let coordinator = try await localReady(memory: memory, server: server)
            let task = try await memory.createTask("task")
            let op = try await coordinator.beginLocalRead(task, root: root)
            let observation = try await reader.read(op)
            #expect(observation.completeness == .partial && observation.reasons & 2 != 0)
            #expect(observation.branches.count <= 256)
            #expect(try fixture.snapshot() == before)
            try op.release()
            try await coordinator.shutdown()
            await reader.clear()
            try await memory.close()
        }
    }
    @Test @MainActor func savedBranchScopeChangesFencePreviewPOSTAndCopyWithoutDeletingHistory()
        async throws
    {
        let fixture = try LocalGitFixture()
        defer { fixture.remove() }
        let server = try AssistanceHTTPServer(mode: "branch")
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
        let reader = LocalGitReader()
        var root = try await memory.saveLocalRoot(
            path: reader.select(fixture.root), expected: 0, exclusions: [])
        let coordinator = try await localReady(memory: memory, server: server)
        let initial = try await memory.createTask("task")
        let note = try await memory.addNote(to: initial, text: "selected note", at: 1)
        let task = try await memory.task(initial.id)
        let op = try await coordinator.beginLocalRead(task, root: root)
        _ = try await reader.read(op)
        let evidence = try await coordinator.bindLocalBranch(op, index: 0)
        try op.release()
        let savedTask = try await memory.task(task.id)
        for prefix in [".git/HEAD", ".git/packed-refs", ".git/refs/heads/main"] {
            let preview = try await coordinator.prepareAssistance(
                savedTask, notes: [note, evidence])
            root = try await memory.saveLocalRoot(
                path: root.path, expected: root.revision, exclusions: [prefix])
            await #expect(throws: (any Error).self) {
                _ = try await coordinator.runAssistance(preview.requestID)
            }
            await #expect(throws: (any Error).self) {
                _ = try await coordinator.prepareAssistance(savedTask, notes: [evidence])
            }
            #expect(try await memory.task(task.id).revision == savedTask.revision)
            #expect(try await memory.checkSummary().attempts == 1)
            root = try await memory.saveLocalRoot(
                path: root.path, expected: root.revision, exclusions: [])
        }
        let preview = try await coordinator.prepareAssistance(savedTask, notes: [note, evidence])
        let publicationResult = try await coordinator.runAssistance(preview.requestID)
        let result = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == publicationResult.publishedTaskID
            })
        try await memory.deleteEvidence(evidence, from: savedTask)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.copyProposal(result.target)
        }
        let current = try await memory.task(task.id)
        let again = try await coordinator.beginLocalRead(current, root: root)
        _ = try await reader.read(again)
        await #expect(throws: LocalSourceFailure.deleted) {
            _ = try await coordinator.bindLocalBranch(again, index: 0)
        }
        try again.release()
        #expect(try await memory.evidence(taskID: task.id).items.map(\.id) == [note])
        try await coordinator.shutdown()
        await reader.clear()
        try await memory.close()
    }
#endif
