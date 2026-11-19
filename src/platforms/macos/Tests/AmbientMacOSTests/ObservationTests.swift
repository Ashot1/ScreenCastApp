import CAmbient
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import Synchronization
import Testing

@testable import AmbientMacOS

@Test func observationFrameRoundTripAndIdleCadence() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 20, creditAcknowledged: true))
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    let frame = ObservationFrame(
        displayKey: "display-A", bundleID: "test.editor", appName: "Editor", title: "Draft",
        lines: [
            .init(
                text: "Привет Ambient",
                words: [
                    .init(text: "Привет", rect: CGRect(x: 0.1, y: 0.2, width: 0.2, height: 0.1)),
                    .init(text: "Ambient", rect: CGRect(x: 0.4, y: 0.2, width: 0.2, height: 0.1)),
                ])
        ], jpeg: Data([1, 2, 3]))
    let first = try await memory.submitObservationFrame(
        frame, idleSeconds: 0, at: .init(utc: 100, monotonicMS: 0))
    #expect(first.disposition == .recorded)
    #expect(first.next == .capture(after: 5))
    let duplicate = try await memory.submitObservationFrame(
        frame, idleSeconds: 0, at: .init(utc: 101, monotonicMS: 1000))
    #expect(duplicate.disposition == .duplicate)
    #expect(
        try await memory.observationTick(
            idleSeconds: 40, appSwitched: false, at: .init(utc: 102, monotonicMS: 2000))
            == .capture(after: 30))
    #expect(
        try await memory.observationTick(
            idleSeconds: 600, appSwitched: false, at: .init(utc: 103, monotonicMS: 3000)) == .hold)
    #expect(
        try await memory.observationTick(
            idleSeconds: 600, appSwitched: true, at: .init(utc: 104, monotonicMS: 4000))
            == .capture(after: 0))
    #expect(try await memory.observationJournal(at: 104).logFrames == 1)
    #expect(try await memory.observationJournal(at: 86_502).logFrames == 0)
    #expect(try await memory.observationHighlight().highlight == nil)
    try await memory.close()
}

@Test func observationCompletionRequiresHighAndHasNoManualPublication() throws {
    var raw = AmbientModelCompletion()
    raw.attempt.id = 1
    raw.attempt.stage = 2
    raw.attempt.outcome = 1
    raw.attempt.expected_model = 1
    raw.attempt.expected_effort = 2
    raw.attempt.expected_tier = 1
    raw.attempt.purpose = 3
    let decoded = try ModelCheckBridge.decodeCompletion(raw)
    #expect(decoded.attempt.purpose == .observation)
    #expect(decoded.publishedTaskID == nil)
    raw.attempt.expected_effort = 1
    #expect(throws: ModelCheckFailure.native) { try ModelCheckBridge.decodeCompletion(raw) }
}

@Test func observationClockNeverMovesBackwards() {
    var clock = ObservationClock()
    let first = clock.sample(utc: 100, uptime: 10)
    let second = clock.sample(utc: 90, uptime: 9)
    #expect(second.utc == first.utc)
    #expect(second.monotonicMS == first.monotonicMS)
    #expect(clock.sample(utc: 101, uptime: 11).monotonicMS == 11_000)
}

@Test func observationChoosesLargestDisplayIntersection() {
    #expect(
        ScreenFrameSource.displayIndex(
            window: CGRect(x: 900, y: 50, width: 500, height: 500),
            displays: [
                CGRect(x: 0, y: 0, width: 1000, height: 800),
                CGRect(x: 1000, y: 0, width: 1000, height: 800),
            ]) == 1)
    #expect(
        ScreenFrameSource.displayIndex(
            window: CGRect(x: 4000, y: 0, width: 10, height: 10),
            displays: [CGRect(x: 0, y: 0, width: 100, height: 100)]) == nil)
}

private func observationImageFixture() throws -> CGImage {
    let context = try #require(
        CGContext(
            data: nil, width: 1600, height: 400, bitsPerComponent: 8,
            bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
    context.setFillColor(CGColor(gray: 1, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: 1600, height: 400))
    let font = CTFontCreateWithName("Helvetica" as CFString, 64, nil)
    for (text, y) in [("Ambient observation", 260.0), ("Привет мир", 120.0)] {
        let line = CTLineCreateWithAttributedString(
            NSAttributedString(
                string: text,
                attributes: [
                    NSAttributedString.Key(kCTFontAttributeName as String): font,
                    NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(
                        gray: 0, alpha: 1),
                ]))
        context.textPosition = CGPoint(x: 80, y: y)
        CTLineDraw(line, context)
    }
    return try #require(context.makeImage())
}

@Test func observationVisionFixtureKeepsLinesAndWordBoxesAndBoundsJPEG() async throws {
    let image = try observationImageFixture()
    let lines = try await ObservationRecognition.recognize(image)
    let words = lines.flatMap(\.words)
    #expect(words.contains { $0.text == "Ambient" })
    #expect(words.contains { $0.text == "Привет" })
    #expect(lines.contains { $0.text.contains("Ambient observation") && $0.words.count == 2 })
    #expect(
        words.allSatisfy {
            $0.rect.width > 0 && $0.rect.height > 0
                && CGRect(x: 0, y: 0, width: 1, height: 1).contains($0.rect)
        })
    let jpeg = try ObservationRecognition.jpeg(image)
    #expect(jpeg.count <= 786432)
    let decoded = try #require(CGImageSourceCreateWithData(jpeg as CFData, nil))
    let resized = try #require(CGImageSourceCreateImageAtIndex(decoded, 0, nil))
    #expect(max(resized.width, resized.height) == 1280)
}

// Test-first package held outside the shared Swift target during the 27F verification freeze.
private actor ObservationFixtureSource: FrameSource {
    func requestPermission() -> Bool { permission }
    var permission = false
    var idle: UInt32 = 0
    var captures = 0
    var warmStarted = false
    private var warmWaiter: CheckedContinuation<Void, Never>?
    private var releaseWaiter: CheckedContinuation<Void, Never>?
    private var permissionHeld = false
    private var permissionArrived = false
    private var permissionArrival: CheckedContinuation<Void, Never>?
    private var permissionRelease: CheckedContinuation<Void, Never>?
    var permissionChecks = 0
    func holdNextPermission() {
        permissionHeld = true
        permissionArrived = false
    }
    func waitForPermission() async {
        if !permissionArrived { await withCheckedContinuation { permissionArrival = $0 } }
    }
    func releasePermission() {
        permissionRelease?.resume()
        permissionRelease = nil
    }
    func hasPermission() async -> Bool {
        permissionChecks += 1
        if permissionHeld {
            permissionHeld = false
            permissionArrived = true
            permissionArrival?.resume()
            permissionArrival = nil
            await withCheckedContinuation { permissionRelease = $0 }
        }
        return permission
    }
    func idleSeconds() -> UInt32 { idle }
    func setPermission(_ value: Bool) { permission = value }
    func setIdle(_ value: UInt32) { idle = value }
    func waitForWarmUp() async {
        if !warmStarted { await withCheckedContinuation { warmWaiter = $0 } }
    }
    func releaseWarmUp() {
        releaseWaiter?.resume()
        releaseWaiter = nil
    }
    func warmUp() async {
        warmStarted = true
        warmWaiter?.resume()
        warmWaiter = nil
        await withCheckedContinuation { releaseWaiter = $0 }
    }
    func capture() -> ObservationFrame? {
        captures += 1
        return .init(
            displayKey: "display-A", bundleID: "test.editor", appName: "Editor",
            title: "Draft \(captures)", lines: [.init(text: "Fixture \(captures)", words: [])],
            jpeg: Data([1]))
    }
}
private actor ObservationIdleDispatcher: ObservationDispatching {
    var cancellations = 0
    func prepareObservation(at: ObservationTime) -> Bool { true }
    func runObservation(at: ObservationTime) -> ModelOperationResult? { nil }
    func cancelObservation() { cancellations += 1 }
}
@Test func observationSessionPermissionWarmUpIdleAndPauseGates() async throws {
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 20, creditAcknowledged: true))
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    let source = ObservationFixtureSource()
    let session = ObservationSession(
        memory: memory, dispatcher: ObservationIdleDispatcher(), source: source, draw: { _ in },
        changed: {})
    #expect(try await session.cycle() == .hold)
    #expect(await session.state == .needsPermission)
    #expect(await source.captures == 0)
    await source.setPermission(true)
    let preparing = Task { try await session.cycle() }
    await source.waitForWarmUp()
    #expect(await session.state == .preparing)
    await source.releaseWarmUp()
    #expect(try await preparing.value == .capture(after: 5))
    #expect(await source.captures == 1)
    await source.setIdle(600)
    #expect(try await session.cycle() == .hold)
    #expect(await source.captures == 1)
    #expect(try await session.cycle(appSwitched: true) == .hold)
    #expect(await source.captures == 2)
    try await memory.setManualPause(true)
    #expect(try await session.cycle(appSwitched: true) == .hold)
    #expect(await session.state == .paused)
    #expect(await source.captures == 2)
    await source.setPermission(false)
    #expect(try await session.cycle() == .hold)
    #expect(await session.state == .needsPermission)
    await session.shutdown()
    try await memory.close()
}

#if AMBIENT_MODEL_TEST_SUPPORT
    @MainActor private final class ObservationHTTPServer {
        let process = Process()
        let url: String
        init(tier: String = "default", knownUsage: Bool = true, failed: Bool = false) throws {
            let output = Pipe()
            process.executableURL = URL(filePath: "/usr/bin/python3")
            process.arguments = [
                "-u", "-c",
                #"""
                from http.server import BaseHTTPRequestHandler,HTTPServer
                import json,sys
                class Handler(BaseHTTPRequestHandler):
                 def log_message(self,*args): pass
                 def do_POST(self):
                  request=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                  assert request['tools']==[]
                  assert request['text']['format']['name']=='screen_observation'
                  content=request['input'][0]['content']
                  context=json.loads(content[0]['text'])
                  assert any(part['type']=='input_image' for part in content)
                  if sys.argv[3]=='failed':
                   self.send_response(500);self.send_header('Content-Length','0');self.end_headers();return
                  frame=context['frames'][0]
                  assert 'Typo' in frame['text']
                  answer={'items':[{'task_id':None,'new_task':'Review wording','related_task_ids':[],
                    'quotes':[{'frame_id':frame['frame_id'],'text':'Typo'}],
                    'proposal':{'message':'Check this word','rationale':'Visible text', 'quote_indexes':[0],
                      'highlight':{'frame_id':frame['frame_id'],'anchor':'Typo'}}}]}
                  item={'id':'message-observation','type':'message','status':'completed','role':'assistant',
                    'content':[{'type':'output_text','text':json.dumps(answer),'annotations':[]}]}
                  response={'status':'completed','model':'gpt-6-luna','reasoning':{'effort':'high'},
                    'service_tier':sys.argv[1],'output':[item]}
                  if sys.argv[2]=='known': response['usage']={'input_tokens':3,'output_tokens':2,'total_tokens':5}
                  events=[{'type':'response.output_item.done','output_index':0,'item':item},
                    {'type':'response.completed','response':response}]
                  body=''.join('data: '+json.dumps(event)+'\r\n\r\n' for event in events).encode()
                  self.send_response(200);self.send_header('Content-Type','text/event-stream')
                  self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
                server=HTTPServer(('127.0.0.1',0),Handler)
                print('http://127.0.0.1:'+str(server.server_port)+'/responses',flush=True)
                server.serve_forever()
                """#,
                tier, knownUsage ? "known" : "unknown", failed ? "failed" : "completed",
            ]
            process.standardOutput = output
            process.standardError = FileHandle.standardError
            try process.run()
            var bytes = Data()
            while bytes.last != 10 {
                guard let byte = try output.fileHandleForReading.read(upToCount: 1), !byte.isEmpty,
                    bytes.count < 1024
                else { throw ModelCheckFailure.native }
                bytes.append(byte)
            }
            url = String(decoding: bytes, as: UTF8.self).trimmingCharacters(
                in: .whitespacesAndNewlines)
        }
        func stop() {
            if process.isRunning {
                process.terminate()
                process.waitUntilExit()
            }
        }
    }
    @MainActor private final class ObservationDrawing {
        var values: [ObservationHighlight?] = []
        var changed = 0
    }
    @MainActor private func observationEventually(_ predicate: () async throws -> Bool) async throws
    {
        let deadline = ContinuousClock.now + .seconds(5)
        while try await !predicate(), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(try await predicate())
    }
    private func observationTypoFrame() -> ObservationFrame {
        .init(
            displayKey: "display-A", bundleID: "test.editor", appName: "Editor", title: "Draft",
            lines: [
                .init(
                    text: "Typo",
                    words: [
                        .init(text: "Typo", rect: CGRect(x: 0.1, y: 0.2, width: 0.3, height: 0.1))
                    ])
            ], jpeg: Data([1, 2, 3]))
    }
    @Test @MainActor func seedObservationUIFixtures() async throws {
        guard let path = ProcessInfo.processInfo.environment["AMBIENT_OBSERVATION_UI_SEED_DIR"]
        else { return }
        let root = URL(filePath: path, directoryHint: .isDirectory).standardizedFileURL
            .resolvingSymlinksInPath()
        try #require(root.path.contains("/target/verification/27g/fixtures"))
        let stopped = root.appending(path: "stopped", directoryHint: .isDirectory)
        let started = root.appending(path: "started", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: stopped, withIntermediateDirectories: true)
        let memory = MemorySession(databaseURL: stopped.appending(path: "memory.sqlite3"))
        let server = try ObservationHTTPServer(tier: "future-tier", knownUsage: false)
        defer { server.stop() }
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let now = Int64(Date().timeIntervalSince1970)
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 17, creditAcknowledged: true))
        #expect(try await coordinator.prepareObservation(at: .init(utc: now, monotonicMS: 0)))
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: .init(utc: now, monotonicMS: 0))
        let result = try await coordinator.runObservation(at: .init(utc: now, monotonicMS: 20_000))
        try #require(result != nil)
        try await memory.setObservationPolicy(
            .init(started: false, perHourLimit: 17, creditAcknowledged: false))
        let journal = try await memory.observationJournal(at: now)
        #expect(journal.sends.count == 1)
        #expect(journal.sends.first?.tier == .other)
        #expect(journal.sends.first?.outcome == .confirmed)
        #expect(try await memory.checkSummary().unknown == 1)
        try await coordinator.shutdown()
        try await memory.close()
        try FileManager.default.copyItem(at: stopped, to: started)
        let restored = MemorySession(databaseURL: started.appending(path: "memory.sqlite3"))
        try await restored.setObservationPolicy(
            .init(started: true, perHourLimit: 9, creditAcknowledged: true))
        try await restored.close()
        let manifest = [
            "kind":
                "Real Core journal from localhost test-only HTTP; no credentials or stub shipped",
            "stoppedLimit": "17", "startedLimit": "9", "sends": "1", "tier": "other",
            "usage": "unknown",
        ]
        try JSONSerialization.data(
            withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys]
        ).write(to: root.appending(path: "manifest.json"))
    }
    @Test @MainActor func observationDispatchReadsHighlightAfterCompletionAndPreservesDirtyDraft()
        async throws
    {
        let server = try ObservationHTTPServer()
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 20, creditAcknowledged: true))
        #expect(await coordinator.creditReport() == false)
        #expect(
            try await coordinator.prepareObservation(
                at: .init(utc: Int64(Date().timeIntervalSince1970), monotonicMS: 0)))
        let model = AssistantModel(session: memory)
        let chat = ChatGPTModel(coordinator: coordinator)
        await model.reload()
        model.draft = "Unsaved independent text"
        let source = ObservationFixtureSource()
        await source.setPermission(true)
        await source.setIdle(600)
        let drawing = ObservationDrawing()
        let session = ObservationSession(
            memory: memory, dispatcher: coordinator, source: source,
            draw: { drawing.values.append($0) },
            changed: {
                drawing.changed += 1
                await chat.observationDidChange(model)
            })
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0,
            at: .init(utc: Int64(Date().timeIntervalSince1970), monotonicMS: 0))
        let preparation = Task { try await session.cycle() }
        await source.waitForWarmUp()
        await source.releaseWarmUp()
        _ = try await preparation.value
        try await observationEventually {
            drawing.values.count == 1 && drawing.changed == 1
                && model.snapshot?.tasks.items.count == 1
        }
        #expect(drawing.values[0]?.anchor == "Typo")
        #expect(model.draft == "Unsaved independent text")
        try await coordinator.setCreditReport(false)
        await #expect(throws: ModelCheckFailure.report) {
            _ = try await coordinator.runCheck(profile: .low)
        }
        let afterRefresh = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0,
            at: .init(
                utc: Int64(Date().timeIntervalSince1970),
                monotonicMS: UInt64(ProcessInfo.processInfo.systemUptime * 1000)))
        #expect(afterRefresh.disposition == .duplicate)
        #expect(try await memory.checkSummary().latest == nil)
        let journal = try await memory.observationJournal(at: Int64(Date().timeIntervalSince1970))
        #expect(journal.sends.count == 1 && journal.sends[0].addedCards == 1)
        #expect(journal.sends.first?.tier == .default)
        #expect(journal.sends.first?.outcome == .confirmed)
        await source.setIdle(0)
        _ = try await session.cycle()
        #expect(drawing.values.count == 2 && drawing.values[1] == nil)
        _ = try await session.cycle()
        #expect(drawing.values.count == 2)
        await session.shutdown()
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    private actor ObservationHeldDispatch: ModelCheckProtocol {
        let downstream: LocalAssistanceBridge
        private var entered = false
        private var held: Bool
        private var failClose: Bool
        private var waiter: CheckedContinuation<Void, Never>?
        private var releaseWaiter: CheckedContinuation<Void, Never>?
        init(url: String, held: Bool = true, failClose: Bool = false) {
            downstream = LocalAssistanceBridge(url: url)
            self.held = held
            self.failClose = failClose
        }
        func holdNextDispatch() { held = true }
        func waitForDispatch() async {
            if !entered { await withCheckedContinuation { waiter = $0 } }
        }
        func release() {
            releaseWaiter?.resume()
            releaseWaiter = nil
        }
        func dispatch(_ handle: UInt64, access: String, keys: [PluginDispatchKey], root: URL)
            async throws -> ModelOperationResult
        {
            if held {
                entered = true
                waiter?.resume()
                waiter = nil
                await withCheckedContinuation { releaseWaiter = $0 }
            }
            return try await downstream.dispatch(handle, access: access, keys: keys, root: root)
        }
        nonisolated func cancel(_ handle: UInt64) { _ = ambient_model_v6_cancel(handle) }
        func close(_ handle: UInt64) throws {
            if failClose {
                failClose = false
                throw ModelCheckFailure.storage
            }
            try ModelCheckBridge.check(ambient_model_v6_close(handle))
        }
    }
    @Test(arguments: [false, true]) @MainActor
    func observationUICompletionPublishesJournalWithoutAnotherCycle(failed: Bool) async throws {
        let server = try ObservationHTTPServer(failed: failed)
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let bridge = ObservationHeldDispatch(url: server.url, failClose: failed)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 7, creditAcknowledged: true))
        let now = Int64(Date().timeIntervalSince1970)
        #expect(try await coordinator.prepareObservation(at: .init(utc: now, monotonicMS: 0)))
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: .init(utc: now, monotonicMS: 0))
        let source = ObservationFixtureSource()
        await source.setPermission(true)
        await source.setIdle(600)
        let drawing = ObservationDrawing()
        let session = ObservationSession(
            memory: memory, dispatcher: coordinator, source: source, draw: { _ in },
            changed: { drawing.changed += 1 })
        let cycle = Task { try await session.cycle() }
        await source.waitForWarmUp()
        await source.releaseWarmUp()
        _ = try await cycle.value
        await bridge.waitForDispatch()
        #expect(session.display.journal?.sends.isEmpty == true)
        await bridge.release()
        // No worker or second cycle: only the existing dispatch completion can refresh the display.
        try await observationEventually {
            failed ? session.display.failure == .storage : drawing.changed == 1
        }
        try await observationEventually { session.display.journal?.hourAttempts == 1 }
        let actual = try await memory.observationJournal(at: now)
        #expect(actual.sends.first?.outcome == (failed ? .provider : .confirmed))
        #expect(session.display.journal?.hourAttempts == 1)
        #expect(session.display.journal?.sends.first?.outcome == actual.sends.first?.outcome)
        #expect(session.display.journal?.sends.first?.tier == (failed ? .missing : .default))
        #expect(session.display.failure == (failed ? .storage : nil))
        #expect(await source.captures == 0)
        await session.shutdown()
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func observationUIDeleteWaitsForActualDispatchAndPreservesAttemptBudget()
        async throws
    {
        let server = try ObservationHTTPServer(tier: "future-tier", knownUsage: false)
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let bridge = ObservationHeldDispatch(url: server.url, held: false)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        let policy = ObservationPolicy(started: true, perHourLimit: 7, creditAcknowledged: true)
        try await memory.setObservationPolicy(policy)
        let now = Int64(Date().timeIntervalSince1970)
        #expect(try await coordinator.prepareObservation(at: .init(utc: now, monotonicMS: 0)))
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: .init(utc: now, monotonicMS: 0))
        _ = try await coordinator.runObservation(at: .init(utc: now, monotonicMS: 20_000))
        let completed = try await memory.observationJournal(at: now)
        #expect(completed.hourAttempts == 1)
        #expect(completed.sends.first?.tier == .other)
        #expect(completed.sends.first?.outcome == .confirmed)
        let savedTasks = try await memory.tasks().items
        let budget = try await memory.checkSummary()
        #expect(budget.unknown == 1)
        await bridge.holdNextDispatch()
        var frame = observationTypoFrame()
        frame = .init(
            displayKey: frame.displayKey, bundleID: frame.bundleID, appName: frame.appName,
            title: "Another draft",
            lines: [.init(text: "Typo in another paragraph", words: frame.lines[0].words)],
            jpeg: frame.jpeg)
        _ = try await memory.submitObservationFrame(
            frame, idleSeconds: 0, at: .init(utc: now, monotonicMS: 20_000))
        let source = ObservationFixtureSource()
        await source.setPermission(true)
        await source.setIdle(600)
        let drawing = ObservationDrawing()
        let session = ObservationSession(
            memory: memory, dispatcher: coordinator, source: source, draw: { _ in },
            changed: { drawing.changed += 1 })
        let preparation = Task { try await session.cycle() }
        await source.waitForWarmUp()
        await source.releaseWarmUp()
        _ = try await preparation.value
        await bridge.waitForDispatch()
        let before = try await memory.observationJournal(at: now)
        #expect(before.hourAttempts == 1)
        #expect(before.sends.first?.outcome == .confirmed)
        var finished = false
        let deletion = Task {
            try await session.deleteJournal()
            finished = true
        }
        try await observationEventually { try await coordinator.load() == .cancelling }
        #expect(!finished)
        let cleared = try await memory.observationJournal(at: now)
        #expect(cleared.policy == policy)
        #expect(cleared.sends.isEmpty)
        #expect(cleared.hourAttempts == before.hourAttempts)
        await bridge.release()
        try await deletion.value
        let after = try await memory.observationJournal(at: now)
        #expect(after.policy == policy)
        #expect(after.hourAttempts == before.hourAttempts)
        #expect(after.logFrames == 0)
        #expect(after.sends.isEmpty)
        #expect(session.display.journal?.sends.isEmpty == true)
        #expect(drawing.changed == 0)
        #expect(try await memory.tasks().items == savedTasks)
        #expect(try await memory.checkSummary() == budget)
        await session.shutdown()
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test(arguments: [false, true]) @MainActor
    func observationShutdownWaitsForActualDispatchAndRejectsLatePublication(stop: Bool)
        async throws
    {
        let server = try ObservationHTTPServer()
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let bridge = ObservationHeldDispatch(url: server.url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 20, creditAcknowledged: true))
        #expect(
            try await coordinator.prepareObservation(
                at: .init(utc: Int64(Date().timeIntervalSince1970), monotonicMS: 0)))
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0,
            at: .init(utc: Int64(Date().timeIntervalSince1970), monotonicMS: 0))
        let source = ObservationFixtureSource()
        await source.setPermission(true)
        await source.setIdle(600)
        let drawing = ObservationDrawing()
        let session = ObservationSession(
            memory: memory, dispatcher: coordinator, source: source, draw: { _ in },
            changed: { drawing.changed += 1 })
        let preparation = Task { try await session.cycle() }
        await source.waitForWarmUp()
        await source.releaseWarmUp()
        _ = try await preparation.value
        await bridge.waitForDispatch()
        let shutdown = Task {
            if stop { try await session.stop() } else { await session.shutdown() }
        }
        try await observationEventually { try await coordinator.load() == .cancelling }
        if stop {
            #expect(
                try await memory.observationJournal(at: Int64(Date().timeIntervalSince1970)).policy
                    .started == false)
            await #expect(throws: ModelCheckFailure.busy) {
                _ = try await memory.beginCheck(
                    userReported: true, at: Int64(Date().timeIntervalSince1970))
            }
        } else {
            #expect(
                try await memory.beginObservation(
                    at: .init(
                        utc: Int64(Date().timeIntervalSince1970),
                        monotonicMS: UInt64(ProcessInfo.processInfo.systemUptime * 1000))) == .busy)
        }
        await bridge.release()
        try await shutdown.value
        #expect(drawing.changed == 0)
        #expect(try await memory.tasks().items.isEmpty)
        #expect(try await memory.checkSummary().attempts == 0)
        #expect(await session.state == .stopped)
        await session.shutdown()
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    private actor ObservationEntranceGate {
        private var entered = false
        private var arrival: CheckedContinuation<Void, Never>?
        private var releaseWaiter: CheckedContinuation<Void, Never>?
        func wait() async {
            entered = true
            arrival?.resume()
            arrival = nil
            await withCheckedContinuation { releaseWaiter = $0 }
        }
        func waitForArrival() async {
            if !entered { await withCheckedContinuation { arrival = $0 } }
        }
        func release() {
            releaseWaiter?.resume()
            releaseWaiter = nil
        }
    }
    @Test @MainActor func observationCancelledBeforeCoordinatorEntryNeverDispatches() async throws {
        let server = try ObservationHTTPServer()
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 20, creditAcknowledged: true))
        let time = ObservationTime(
            utc: Int64(Date().timeIntervalSince1970),
            monotonicMS: UInt64(ProcessInfo.processInfo.systemUptime * 1000))
        #expect(try await coordinator.prepareObservation(at: time))
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: .init(utc: time.utc, monotonicMS: 0))
        let gate = ObservationEntranceGate()
        let task = Task {
            await gate.wait()
            return try await coordinator.runObservation(at: time)
        }
        await gate.waitForArrival()
        task.cancel()
        await gate.release()
        await #expect(throws: CancellationError.self) { _ = try await task.value }
        #expect(try await memory.checkSummary().attempts == 0)
        #expect(try await memory.tasks().items.isEmpty)
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test(arguments: [
        "hide", "clear", "delete", "pause", "suppress", "stop", "logout", "lateHide", "lateLogout",
    ])
    @MainActor func observationLocalMutationHidesWhileIdle(action: String) async throws {
        let server = try ObservationHTTPServer()
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 20, creditAcknowledged: true))
        let time = ObservationTime(utc: Int64(Date().timeIntervalSince1970), monotonicMS: 0)
        _ = try await coordinator.prepareObservation(at: time)
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: time)
        let source = ObservationFixtureSource()
        await source.setPermission(true)
        await source.setIdle(600)
        let drawing = ObservationDrawing()
        let session = ObservationSession(
            memory: memory, dispatcher: coordinator, source: source,
            draw: { drawing.values.append($0) }, changed: {})
        let preparation = Task { try await session.cycle() }
        await source.waitForWarmUp()
        await source.releaseWarmUp()
        _ = try await preparation.value
        try await observationEventually { drawing.values.last.flatMap { $0 } != nil }
        let priorPermissionChecks = await source.permissionChecks
        _ = try await coordinator.prepareObservation(at: time)
        #expect(await source.permissionChecks == priorPermissionChecks)
        let proposal = try #require(try await memory.proposalFeed().first)
        let task = try await memory.task(proposal.task.taskID)
        var earlier: Task<Void, Never>?
        if action.hasPrefix("late") {
            await source.holdNextPermission()
            earlier = Task { await memory.observationDidMutate() }
            await source.waitForPermission()
        }
        switch action {
        case "hide", "lateHide":
            try await memory.setProposalVisibility(proposal.target, visible: false)
        case "clear":
            try await memory.clearDiscussion(task, workspaceRevision: proposal.workspaceRevision)
        case "delete":
            try await memory.deleteTask(task)
            // Projection reload can fail after the mutation committed; delivery must already be complete.
            await #expect(throws: MemoryFailure.self) {
                _ = try await memory.load(selectedTaskID: task.id)
            }
        case "pause": try await memory.setManualPause(true)
        case "suppress": try await memory.setInitiativeSuppressed(task, value: true)
        case "stop":
            try await memory.setObservationPolicy(
                .init(started: false, perHourLimit: 20, creditAcknowledged: false))
        default: try await coordinator.signOut()
        }
        let afterHide = drawing.values.count
        if let earlier {
            await source.releasePermission()
            await earlier.value
            #expect(drawing.values.count == afterHide)
        }
        #expect(drawing.values.last.flatMap { $0 } == nil)
        #expect(await source.captures == 0)
        if action == "logout" || action == "lateLogout" {
            #expect(try await memory.observationHighlight().highlight != nil)
            try await memory.setModelSession(epoch: 2, valid: true, reported: true)
            _ = try await memory.editTask(task, text: "Edited after reconnect")
            #expect(drawing.values.last.flatMap { $0 } == nil)
            #expect(drawing.values.count == afterHide)
        }
        await session.shutdown()
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

private func observationStoredFrames(_ url: URL) throws -> Int {
    let process = Process()
    let output = Pipe()
    process.executableURL = URL(filePath: "/usr/bin/python3")
    process.arguments = [
        "-c",
        "import sqlite3,sys;print(sqlite3.connect(sys.argv[1]).execute('SELECT count(*) FROM observation_frames').fetchone()[0])",
        url.path,
    ]
    process.standardOutput = output
    try process.run()
    let data = output.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    guard process.terminationStatus == 0,
        let count = Int(
            String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
    else { throw ModelCheckFailure.native }
    return count
}
@MainActor private func observationWait(_ predicate: () async throws -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(5)
    while try await !predicate(), ContinuousClock.now < deadline {
        try await Task.sleep(for: .milliseconds(10))
    }
    #expect(try await predicate())
}
@Test(arguments: [false, true]) @MainActor
func observationJournalDriverCleansWithoutArmingSavedPolicy(afterStop: Bool) async throws {
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 20, creditAcknowledged: true))
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    let frame = ObservationFrame(
        displayKey: "A", bundleID: "fixture", appName: "Fixture", title: "Title",
        lines: [.init(text: "Old frame", words: [])], jpeg: Data([1]))
    _ = try await memory.submitObservationFrame(
        frame, idleSeconds: 0, at: .init(utc: 100, monotonicMS: 0))
    let time = Mutex(ObservationTime(utc: 100, monotonicMS: 0))
    let source = ObservationFixtureSource()
    let dispatcher = ObservationIdleDispatcher()
    let session = ObservationSession(
        memory: memory, dispatcher: dispatcher, source: source, draw: { _ in }, changed: {},
        clock: ObservationClock(now: { time.withLock { $0 } }), journalInterval: .milliseconds(10))
    await session.cleanup()
    if afterStop {
        try await session.start(creditAcknowledged: true)
        try await observationWait { await source.permissionChecks > 0 }
        try await session.stop()
    }
    let checks = await source.permissionChecks
    #expect(afterStop || checks == 0)
    #expect(try observationStoredFrames(url) == 1)
    time.withLock { $0 = .init(utc: 86_502, monotonicMS: 86_402_000) }
    try await observationWait { try observationStoredFrames(url) == 0 }
    #expect(await source.permissionChecks == checks)
    #expect(await source.warmStarted == false)
    #expect(await source.captures == 0)
    await session.shutdown()
    try await memory.close()
}
@Test @MainActor func observationShutdownEndsJournalAndRejectsRestart() async throws {
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    try await memory.setObservationPolicy(
        .init(started: true, perHourLimit: 20, creditAcknowledged: true))
    try await memory.setModelSession(epoch: 1, valid: true, reported: true)
    _ = try await memory.submitObservationFrame(
        .init(
            displayKey: "A", bundleID: "fixture", appName: "Fixture", title: "Title",
            lines: [.init(text: "Old frame", words: [])], jpeg: Data([1])), idleSeconds: 0,
        at: .init(utc: 100, monotonicMS: 0))
    let time = Mutex(ObservationTime(utc: 100, monotonicMS: 0))
    let source = ObservationFixtureSource()
    let session = ObservationSession(
        memory: memory, dispatcher: ObservationIdleDispatcher(), source: source, draw: { _ in },
        changed: {},
        clock: ObservationClock(now: { time.withLock { $0 } }), journalInterval: .milliseconds(10))
    await session.cleanup()
    await session.shutdown()
    time.withLock { $0 = .init(utc: 86_502, monotonicMS: 86_402_000) }
    await session.cleanup()
    await #expect(throws: ModelCheckFailure.closed) {
        try await session.start(creditAcknowledged: true)
    }
    try await Task.sleep(for: .milliseconds(50))
    #expect(try observationStoredFrames(url) == 1)
    #expect(await source.permissionChecks == 0)
    try await memory.close()
}

@Test @MainActor func observationStartupCleanupCannotReplaceExplicitStartAndShutdownWaitsForWarmUp()
    async throws
{
    let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    let source = ObservationFixtureSource()
    await source.setPermission(true)
    let dispatcher = ObservationIdleDispatcher()
    let session = ObservationSession(
        memory: memory, dispatcher: dispatcher, source: source, draw: { _ in }, changed: {},
        journalInterval: .milliseconds(10))
    await session.cleanup()
    try await session.start(creditAcknowledged: true)
    await source.waitForWarmUp()
    await session.cleanup()
    #expect(await session.state == .preparing)
    let finished = Mutex(false)
    let shutdown = Task {
        await session.shutdown()
        finished.withLock { $0 = true }
    }
    try await observationWait { await dispatcher.cancellations >= 2 }
    #expect(finished.withLock { $0 } == false)
    await source.releaseWarmUp()
    await shutdown.value
    #expect(finished.withLock { $0 })
    #expect(await source.captures == 0)
    await #expect(throws: ModelCheckFailure.closed) {
        try await session.start(creditAcknowledged: true)
    }
    try await memory.close()
}

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func observationManualCheckboxDoesNotCancelSavedCreditDispatch() async throws {
        let server = try ObservationHTTPServer()
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let bridge = ObservationHeldDispatch(url: server.url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 20, creditAcknowledged: true))
        let time = ObservationTime(
            utc: Int64(Date().timeIntervalSince1970),
            monotonicMS: UInt64(ProcessInfo.processInfo.systemUptime * 1000))
        _ = try await coordinator.prepareObservation(at: time)
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: .init(utc: time.utc, monotonicMS: 0))
        let operation = Task { try await coordinator.runObservation(at: time) }
        await bridge.waitForDispatch()
        try await coordinator.setCreditReport(false)
        #expect(try await coordinator.load() != .cancelling)
        #expect(try await memory.beginObservation(at: time) == .busy)
        await bridge.release()
        let result = try await operation.value
        #expect(result?.attempt.purpose == .observation)
        #expect(try await memory.checkSummary().attempts == 1)
        #expect(try await memory.tasks().items.count == 1)
        #expect(await coordinator.creditReport() == false)
        await #expect(throws: ModelCheckFailure.report) {
            _ = try await coordinator.runCheck(profile: .low)
        }
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    private actor ObservationHeldReturn: ObservationDispatching {
        let coordinator: ChatGPTCoordinator
        private var returned = false
        private var arrival: CheckedContinuation<Void, Never>?
        private var releaseWaiter: CheckedContinuation<Void, Never>?
        init(_ coordinator: ChatGPTCoordinator) { self.coordinator = coordinator }
        func prepareObservation(at: ObservationTime) async throws -> Bool {
            try await coordinator.prepareObservation(at: at)
        }
        func cancelObservation() async { await coordinator.cancelObservation() }
        func runObservation(at: ObservationTime) async throws -> ModelOperationResult? {
            let result = try await coordinator.runObservation(at: at)
            returned = true
            arrival?.resume()
            arrival = nil
            await withCheckedContinuation { releaseWaiter = $0 }
            return result
        }
        func waitForReturn() async {
            if !returned { await withCheckedContinuation { arrival = $0 } }
        }
        func release() {
            releaseWaiter?.resume()
            releaseWaiter = nil
        }
    }
    @Test(arguments: [false, true]) @MainActor
    func observationOldCompletionCannotRestoreHighlightAfterSameIdentityReconnect(reconnect: Bool)
        async throws
    {
        let server = try ObservationHTTPServer()
        defer { server.stop() }
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
        let store = AssistanceTestStore()
        let saved = try #require(store.load())
        let coordinator = ChatGPTCoordinator(
            store: store, memory: memory, modelBridge: LocalAssistanceBridge(url: server.url))
        let held = ObservationHeldReturn(coordinator)
        try await memory.setObservationPolicy(
            .init(started: true, perHourLimit: 20, creditAcknowledged: true))
        let time = ObservationTime(utc: Int64(Date().timeIntervalSince1970), monotonicMS: 0)
        _ = try await coordinator.prepareObservation(at: time)
        _ = try await memory.submitObservationFrame(
            observationTypoFrame(), idleSeconds: 0, at: time)
        let source = ObservationFixtureSource()
        await source.setPermission(true)
        await source.setIdle(600)
        let drawing = ObservationDrawing()
        let session = ObservationSession(
            memory: memory, dispatcher: held, source: source,
            draw: { drawing.values.append($0) }, changed: { drawing.changed += 1 })
        let preparation = Task { try await session.cycle() }
        await source.waitForWarmUp()
        await source.releaseWarmUp()
        _ = try await preparation.value
        await held.waitForReturn()
        #expect(try await memory.observationHighlight().highlight != nil)
        if reconnect {
            store.expireForTest()
            #expect(try await coordinator.load() == .expired)
            store.save(saved)
            _ = try await coordinator.prepareObservation(at: time)
        } else {
            _ = try await memory.createTask("Independent task")
        }
        await held.release()
        try await observationEventually { drawing.changed == 1 }
        if reconnect {
            #expect(drawing.values.allSatisfy { $0 == nil })
        } else {
            #expect(drawing.values.last.flatMap { $0 }?.anchor == "Typo")
        }
        #expect(try await memory.tasks().items.count == (reconnect ? 1 : 2))
        #expect(try await memory.checkSummary().attempts == 1)
        await session.shutdown()
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

@Test func observationMissingWordGeometryPreservesTextWithoutInventingAdjacency() {
    let text = "first middle last"
    let line = ObservationRecognition.line(text) { range in
        text[range] == "middle" ? nil : CGRect(x: 0.1, y: 0.2, width: 0.1, height: 0.1)
    }
    #expect(line.text == text)
    #expect(line.words.isEmpty)
}
