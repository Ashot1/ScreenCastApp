import Darwin
import Foundation
import Testing

@testable import AmbientMacOS
@testable import VoiceSupport

@MainActor final class TestVoiceMicrophone: VoiceMicrophone {
    var audio = voiceWAV()
    var elapsed: TimeInterval = 0
    var permissions: [@MainActor @Sendable (Bool) -> Void] = []
    var finished: (@MainActor @Sendable (Bool) -> Void)?
    var starts = 0
    var stops = 0
    func requestPermission(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        permissions.append(completion)
    }
    func start(at url: URL, finished: @escaping @MainActor @Sendable (Bool) -> Void) throws {
        starts += 1
        self.finished = finished
        try audio.write(to: url)
    }
    func stop() { stops += 1 }
}

@MainActor final class VoiceTestValues {
    var allowed = true
    var draft = "previous answer"
    var checks = 0
}

@Test @MainActor func voiceStartupCleanupRemovesOnlyStaleOperationsWithoutPermission() async throws
{
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let lease = try await workspace.prepare()
    let microphone = TestVoiceMicrophone()
    let voice = VoiceReplyModel(
        microphone: microphone, workspace: workspace,
        supervisor: VoiceSupervisor(executable: URL(fileURLWithPath: "/not-used")))
    voice.selectModels(URL(fileURLWithPath: "/test-models"))
    await voice.startupCleanup()
    #expect(voice.failure == .cleanup && !voice.canRecord)
    #expect(FileManager.default.fileExists(atPath: lease.audioURL.path))
    #expect(flock(lease.leaseFD.raw, LOCK_UN) == 0)
    voice.retryCleanup()
    try await voiceEventually { !voice.busy }
    #expect(voice.failure == nil && voice.canRecord)
    #expect(microphone.permissions.isEmpty && microphone.starts == 0)
    #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    #expect(await voice.shutdown())
}

@MainActor func voiceEventually(timeout: Duration = .seconds(5), _ condition: @MainActor () -> Bool)
    async throws
{
    let deadline = ContinuousClock.now.advanced(by: timeout)
    while ContinuousClock.now < deadline {
        if condition() { return }
        try await Task.sleep(for: .milliseconds(10))
    }
    try #require(condition())
}

@Test @MainActor func voicePermissionCanBeCancelledWithoutWaitingAndNeverStartsLate() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let microphone = TestVoiceMicrophone()
    let voice = VoiceReplyModel(
        microphone: microphone, workspace: VoiceWorkspace(root: root),
        supervisor: VoiceSupervisor(executable: URL(fileURLWithPath: "/not-used")))
    voice.start(
        models: URL(fileURLWithPath: "/test-models"), validate: { true }, apply: { _ in false })
    try await voiceEventually { microphone.permissions.count == 1 }
    try #require(microphone.permissions.count == 1)
    #expect(voice.phase == .permission)
    let started = ContinuousClock.now
    #expect(await voice.shutdown())
    #expect(started.duration(to: .now) < .seconds(1))
    microphone.permissions[0](true)
    try await Task.sleep(for: .milliseconds(30))
    #expect(microphone.starts == 0)
    #expect(!voice.busy)
    #expect(!FileManager.default.fileExists(atPath: root.path))
}

@Test @MainActor func voiceWaitsForCheckmarkKeepsPreviousDraftAndGatesAfterPermission() async throws
{
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let executable = try voiceScript(
        "fd=os.open(directory+'/result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.write(fd,b'{\"status\":\"text\",\"text\":\"new answer\"}')\nos.close(fd)"
    )
    defer { try? FileManager.default.removeItem(at: executable.deletingLastPathComponent()) }
    let microphone = TestVoiceMicrophone()
    let supervisor = VoiceSupervisor(executable: executable)
    let voice = VoiceReplyModel(
        microphone: microphone, workspace: VoiceWorkspace(root: root), supervisor: supervisor)
    let values = VoiceTestValues()
    let validate: @MainActor () async -> Bool = {
        values.checks += 1
        return values.allowed
    }
    voice.start(
        models: URL(fileURLWithPath: "/test-models"), validate: validate,
        apply: {
            values.draft = $0
            return true
        })
    try await voiceEventually { microphone.permissions.count == 1 }
    try #require(microphone.permissions.count == 1)
    values.allowed = false
    microphone.permissions[0](true)
    try await voiceEventually { !voice.busy }
    #expect(microphone.starts == 0 && values.draft == "previous answer")
    values.allowed = true
    voice.start(
        models: URL(fileURLWithPath: "/test-models"), validate: validate,
        apply: {
            values.draft = $0
            return true
        })
    try await voiceEventually { microphone.permissions.count == 2 }
    try #require(microphone.permissions.count == 2)
    microphone.permissions[1](true)
    try await voiceEventually { voice.phase == .recording }
    try #require(voice.phase == .recording)
    #expect(await supervisor.running == false)
    #expect(values.draft == "previous answer")
    microphone.elapsed = 60
    microphone.finished?(true)
    #expect(voice.phase == .limit)
    #expect(voice.elapsed == 60)
    #expect(await supervisor.running == false)
    #expect(values.draft == "previous answer")
    voice.finish()
    try await voiceEventually { !voice.busy }
    #expect(values.draft == "new answer")
    #expect(values.checks >= 7)
    #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    #expect(await voice.shutdown())
}

@Test @MainActor func voiceCleanupFailureRestoresTextInputAndBlocksRecordingUntilRetry()
    async throws
{
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let microphone = TestVoiceMicrophone()
    let voice = VoiceReplyModel(
        microphone: microphone, workspace: VoiceWorkspace(root: root),
        supervisor: VoiceSupervisor(executable: URL(fileURLWithPath: "/not-used")))
    voice.selectModels(URL(fileURLWithPath: "/test-models"))
    voice.start(
        models: URL(fileURLWithPath: "/test-models"), validate: { true }, apply: { _ in true })
    try await voiceEventually { microphone.permissions.count == 1 }
    try #require(microphone.permissions.count == 1)
    microphone.permissions[0](true)
    try await voiceEventually { voice.phase == .recording }
    let name = try #require(FileManager.default.contentsOfDirectory(atPath: root.path).first)
    let directory = root.appendingPathComponent(name)
    try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: directory.path)
    voice.cancel()
    try await voiceEventually { !voice.busy }
    #expect(voice.failure == .cleanup && !voice.canRecord)
    voice.start(
        models: URL(fileURLWithPath: "/test-models"), validate: { true }, apply: { _ in true })
    #expect(microphone.permissions.count == 1)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
    voice.retryCleanup()
    try await voiceEventually { !voice.busy }
    #expect(voice.failure == nil && voice.canRecord)
    #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    #expect(await voice.shutdown())
}

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func voiceCorePauseRejectsLatePermissionAndPreservesPreviousReply()
        async throws
    {
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let database = root.appendingPathExtension("sqlite3")
        defer { try? FileManager.default.removeItem(at: database) }
        let memory = MemorySession(databaseURL: database)
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("test-owned intent")
        let microphone = TestVoiceMicrophone()
        let voice = VoiceReplyModel(
            microphone: microphone, workspace: VoiceWorkspace(root: root),
            supervisor: VoiceSupervisor(executable: URL(fileURLWithPath: "/not-used")))
        voice.selectModels(URL(fileURLWithPath: "/test-models"))
        let chat = ChatGPTModel(coordinator: coordinator, voice: voice)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        chat.updateReply("previous answer")
        await chat.waitForReplyPreview()
        chat.startVoice(replacing: true)
        try await voiceEventually { microphone.permissions.count == 1 }
        try await memory.setManualPause(true)
        microphone.permissions[0](true)
        try await voiceEventually { !voice.busy }
        #expect(microphone.starts == 0 && chat.replyDraft == "previous answer")
        #expect(voice.failure == .stale)
        #expect(!FileManager.default.fileExists(atPath: root.path))
        #expect(try server.requestContexts().count == 2)
        #expect(await chat.shutdown())
        try await memory.close()
    }

    @Test @MainActor func voiceLateASRAfterCoreRevocationCannotReplaceReply() async throws {
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let executable = try voiceScript(
            "fd=os.open(directory+'/ready',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.close(fd)\ntime.sleep(.15)\nfd=os.open(directory+'/result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.write(fd,b'{\"status\":\"text\",\"text\":\"late answer\"}')\nos.close(fd)"
        )
        defer { try? FileManager.default.removeItem(at: executable.deletingLastPathComponent()) }
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let database = root.appendingPathExtension("sqlite3")
        defer { try? FileManager.default.removeItem(at: database) }
        let memory = MemorySession(databaseURL: database)
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("test-owned intent")
        let microphone = TestVoiceMicrophone()
        let voice = VoiceReplyModel(
            microphone: microphone, workspace: VoiceWorkspace(root: root),
            supervisor: VoiceSupervisor(executable: executable))
        voice.selectModels(URL(fileURLWithPath: "/test-models"))
        let chat = ChatGPTModel(coordinator: coordinator, voice: voice)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        chat.updateReply("previous answer")
        await chat.waitForReplyPreview()
        chat.startVoice(replacing: true)
        try await voiceEventually { microphone.permissions.count == 1 }
        microphone.permissions[0](true)
        try await voiceEventually { voice.phase == .recording }
        voice.finish()
        try await voiceEventually {
            guard let name = try? FileManager.default.contentsOfDirectory(atPath: root.path).first
            else { return false }
            return FileManager.default.fileExists(
                atPath: root.appendingPathComponent(name).appendingPathComponent("ready").path)
        }
        // Deliberately omit the UI observer: final application must independently ask Core again.
        try await coordinator.setCreditReport(false)
        try await voiceEventually { !voice.busy }
        #expect(chat.replyDraft == "previous answer" && voice.failure == .stale)
        #expect(try server.requestContexts().count == 2)
        #expect(await chat.shutdown())
        try await memory.close()
    }

    @Test @MainActor func voiceStopDoesNotOccupyCloudCoordinatorAndMemorySelectionKeepsTarget()
        async throws
    {
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let executable = try voiceScript(
            "signal.signal(signal.SIGTERM,signal.SIG_IGN)\nfd=os.open(directory+'/ready',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.close(fd)\ntime.sleep(20)"
        )
        defer { try? FileManager.default.removeItem(at: executable.deletingLastPathComponent()) }
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let database = root.appendingPathExtension("sqlite3")
        defer { try? FileManager.default.removeItem(at: database) }
        let memory = MemorySession(databaseURL: database)
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("test-owned intent")
        let microphone = TestVoiceMicrophone()
        let supervisor = VoiceSupervisor(executable: executable)
        let voice = VoiceReplyModel(
            microphone: microphone, workspace: VoiceWorkspace(root: root), supervisor: supervisor)
        voice.selectModels(URL(fileURLWithPath: "/test-models"))
        let chat = ChatGPTModel(coordinator: coordinator, voice: voice)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        let receipt = try #require(chat.feed.first?.target)
        chat.updateReply("previous answer")
        await chat.waitForReplyPreview()
        chat.startVoice(replacing: true)
        try await voiceEventually { microphone.permissions.count == 1 }
        microphone.permissions[0](true)
        try await voiceEventually { voice.phase == .recording }
        chat.selectedTaskChanged()
        #expect(chat.replyParent == receipt && voice.phase == .recording)
        voice.finish()
        try await voiceEventually {
            guard let name = try? FileManager.default.contentsOfDirectory(atPath: root.path).first
            else { return false }
            return FileManager.default.fileExists(
                atPath: root.appendingPathComponent(name).appendingPathComponent("ready").path)
        }
        voice.cancel()
        _ = try await coordinator.runCheck(profile: .low)
        #expect(await supervisor.running)
        #expect(voice.busy)
        try await voiceEventually { !voice.busy }
        #expect(chat.replyDraft == "previous answer")
        chat.updateReply("new typed answer")
        await chat.waitForReplyPreview()
        #expect(chat.replyDraft == "new typed answer")
        #expect(await chat.shutdown())
        try await memory.close()
    }

    @Test(.enabled(if: ProcessInfo.processInfo.environment["AMBIENT_VOICE_REAL_ASR"] == "1"))
    @MainActor func voicePackagedRealASRReachesEditableDraftExactPreviewAndOneSend() async throws {
        let project = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        let helper = project.appendingPathComponent(
            "target/macos/Build/Products/Debug/Ambient.app/Contents/Helpers/AmbientVoiceHelper.app/Contents/MacOS/AmbientVoiceHelper"
        )
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-real-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let database = root.appendingPathExtension("sqlite3")
        defer { try? FileManager.default.removeItem(at: database) }
        let memory = MemorySession(databaseURL: database)
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("test-owned public speech reply")
        let microphone = TestVoiceMicrophone()
        microphone.audio = try Data(
            contentsOf: project.appendingPathComponent(
                "target/verification/10a/fixtures/ru_ru-short.wav"))
        let voice = VoiceReplyModel(
            microphone: microphone, workspace: VoiceWorkspace(root: root),
            supervisor: VoiceSupervisor(executable: helper))
        voice.selectModels(
            URL(
                fileURLWithPath:
                    "/Users/gojda/Library/Application Support/FluidAudio/Models/parakeet-tdt-0.6b-v3"
            ))
        let chat = ChatGPTModel(coordinator: coordinator, voice: voice)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        chat.startVoice()
        try await voiceEventually { microphone.permissions.count == 1 }
        microphone.permissions[0](true)
        try await voiceEventually { voice.phase == .recording }
        #expect(try server.requestContexts().count == 2)
        voice.finish()
        try await voiceEventually(timeout: .seconds(305)) { !voice.busy }
        try #require(voice.failure == nil)
        #expect(chat.replyDraft.lowercased().contains("православ"))
        chat.updateReply(chat.replyDraft + " (reviewed)")
        await chat.waitForReplyPreview()
        let preview = try #require(chat.replyPreview)
        #expect(try server.requestContexts().count == 2)
        await chat.sendReply()
        #expect(try server.requestContexts().count == 3)
        #expect(try server.requestContexts().last?.utf8.elementsEqual(preview.context.utf8) == true)
        #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
        #expect(
            try await memory.load(selectedTaskID: task.id).selectedTask?.revision == task.revision)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
        #expect(await chat.shutdown())
        try await memory.close()
    }

    @Test @MainActor func voiceUsesCurrentClarificationSameDraftAndOnlyExistingSend() async throws {
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let executable = try voiceScript(
            "fd=os.open(directory+'/result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.write(fd,b'{\"status\":\"text\",\"text\":\"public dictated answer\"}')\nos.close(fd)"
        )
        defer { try? FileManager.default.removeItem(at: executable.deletingLastPathComponent()) }
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let memory = MemorySession(databaseURL: root.appendingPathExtension("sqlite3"))
        defer { try? FileManager.default.removeItem(at: root.appendingPathExtension("sqlite3")) }
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("test-owned intent")
        let microphone = TestVoiceMicrophone()
        let voice = VoiceReplyModel(
            microphone: microphone, workspace: VoiceWorkspace(root: root),
            supervisor: VoiceSupervisor(executable: executable))
        voice.selectModels(URL(fileURLWithPath: "/test-models"))
        let chat = ChatGPTModel(coordinator: coordinator, voice: voice)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        let receipt = try #require(chat.feed.first?.target)
        chat.updateReply("previous answer")
        await chat.waitForReplyPreview()
        chat.startVoice()
        #expect(microphone.permissions.isEmpty)
        chat.startVoice(replacing: true)
        try await voiceEventually { microphone.permissions.count == 1 }
        try #require(microphone.permissions.count == 1)
        microphone.permissions[0](true)
        try await voiceEventually { voice.phase == .recording }
        try #require(voice.phase == .recording)
        #expect(chat.replyDraft == "previous answer")
        #expect(!chat.canSendReply)
        chat.updateReply("must not edit while recording")
        #expect(chat.replyDraft == "previous answer")
        #expect(try server.requestContexts().count == 2)
        voice.finish()
        try await voiceEventually { !voice.busy }
        await chat.waitForReplyPreview()
        #expect(chat.replyDraft == "public dictated answer")
        #expect(chat.replyParent == receipt)
        let preview = try #require(chat.replyPreview)
        #expect(try server.requestContexts().count == 2)
        await chat.sendReply()
        #expect(try server.requestContexts().last?.utf8.elementsEqual(preview.context.utf8) == true)
        #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
        #expect(
            try await memory.load(selectedTaskID: task.id).selectedTask?.revision == task.revision)
        #expect(await chat.shutdown())
        try await memory.close()
    }
#endif
