import CAmbient
import Foundation
import Testing

@testable import AmbientMacOS

@Test func selectedAssistanceRequiresCoreHighCapabilityAndPreservesDraft() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    let task = try await memory.createTask("saved intent")
    try await memory.setModelSession(epoch: 42, valid: true, reported: true)
    #expect(try await memory.assistanceAvailable() == false)
    await #expect(throws: ModelCheckFailure.capability) {
        _ = try await memory.prepareAssistance(task, notes: [])
    }
    let model = await AssistantModel(session: memory)
    await model.reload()
    await MainActor.run { model.draft = "independent unsaved text" }
    try await memory.clearAssistance()
    #expect(await model.draft == "independent unsaved text")
    try await memory.close()
}
@Test func modelABI4RejectsChangedProfileAndInvalidSessionBeforeSending() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    #expect(MemoryLayout<AmbientAssistancePreview>.size == 48)
    #expect(MemoryLayout<AmbientAssistanceRequest>.size == 48)
    #expect(MemoryLayout<AmbientProposalTarget>.size == 16)
    #expect(MemoryLayout<AmbientModelCompletion>.size == 96)
    try await memory.setModelSession(epoch: 42, valid: true, reported: true)
    let h = try await memory.beginCheck(userReported: true, profile: .high, at: 1)
    try await memory.setModelSession(epoch: 43, valid: true, reported: true)
    let bridge = ModelCheckBridge()
    await #expect(throws: ModelCheckFailure.cancelled) {
        _ = try await bridge.dispatch(
            h, access: "test-only", keys: [], root: FileManager.default.temporaryDirectory)
    }
    try await bridge.close(h)
    #expect(try await memory.checkSummary().attempts == 0)
    try await memory.close()
}

#if AMBIENT_MODEL_TEST_SUPPORT
    import Dispatch

    final class AssistanceTestStore: ChatGPTCredentialStore, @unchecked Sendable {
        // Test-only credentials, serialized by a lock; never reads native Keychain.
        private let lock = NSLock()
        private var envelope = ChatGPTEnvelope(
            host: "urn:uuid:12345678-1234-4234-9234-123456789abc", registration: 3,
            clientID: "test-client", issuer: "https://auth.openai.com", subject: "test-subject",
            hint: "test-hint", nonce: "test-nonce",
            credentials: ChatGPTCredentials(
                access: "test-only-access", refresh: "test-only-refresh",
                scopes: ["openid", "resource.invoke", "chatgpt.tokens.use.direct"],
                expiresAt: Int64(Date().timeIntervalSince1970) + 3600))
        func load() -> ChatGPTEnvelope? { lock.withLock { envelope } }
        func save(_ value: ChatGPTEnvelope) { lock.withLock { envelope = value } }
        func expireForTest() {
            lock.withLock {
                envelope.credentials?.expiresAt = Int64(Date().timeIntervalSince1970) - 1
            }
        }
    }
    @MainActor final class AssistanceHTTPServer {
        let process = Process()
        let url: String
        let requestsURL = FileManager.default.temporaryDirectory.appendingPathComponent(
            UUID().uuidString)
        func requestContexts() throws -> [String] {
            guard FileManager.default.fileExists(atPath: requestsURL.path) else { return [] }
            let data = try String(contentsOf: requestsURL, encoding: .utf8)
            return try data.split(separator: "\n").map {
                guard
                    let value = try JSONSerialization.jsonObject(
                        with: Data($0.utf8), options: .fragmentsAllowed) as? String
                else { throw ModelCheckFailure.native }
                return value
            }
        }
        init(mode: String = "suggestion") throws {
            let output = Pipe()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
            process.arguments = [
                "-u", "-c",
                #"""
                import json,sys
                from http.server import HTTPServer,BaseHTTPRequestHandler
                mode=sys.argv[1]
                capture=sys.argv[2]
                class Handler(BaseHTTPRequestHandler):
                 def log_message(self,*args): pass
                 def do_POST(self):
                  request=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                  assert self.path=='/responses'
                  assert self.headers['Authorization']=='Bearer test-only-access'
                  assert request['store']==False and request['stream']==True and request['tools']==[]
                  assert 'max_output_tokens' not in request
                  raw=request['input'][0]['content']
                  with open(capture,'a',encoding='utf-8') as f: f.write(json.dumps(raw,ensure_ascii=False)+'\n')
                  if raw=='Reply exactly: AMBIENT_OK': text='AMBIENT_OK'
                  else:
                   assert request['reasoning']['effort']=='high'
                   assert request['text']['format']['strict']==True
                   context=json.loads(raw)
                   ground=context['notes'][0]['evidence_id'] if context['notes'] else context['task']['evidence_id']
                   if mode=='branch':
                    source=context['sources'][0]
                    assert source['source']['namespace']=='local-git-branch-v1'
                    assert source['source']['object_id']=='refs/heads/main'
                    assert source['source']['version']=='2222222222222222222222222222222222222222'
                    assert source['historical_observation']==True
                    assert source['root_path'].startswith('/private/tmp/ambient-local-')
                    assert 'Observation interval' in source['text']
                    assert source['assertion_kind']==0 and source['completeness']==2
                    ground=source['evidence_id']
                   if mode=='github':
                    assert len(context['sources'])==3
                    for source in context['sources']:
                     assert source['source']['namespace']=='github-com-rest-v1'
                     assert source['github']['coverage']=='selected_objects'
                     assert source['github']['observation']=='non_atomic_observation'
                     assert source['github']['current_state']=='current_state_unverified'
                     assert source['assertion_kind']==0
                    assert 'Final title' in context['sources'][0]['text']
                    assert 'local-test-PAT' not in raw
                    ground=context['sources'][0]['evidence_id']
                   if mode=='jira':
                    assert len(context['sources'])==3
                    for source in context['sources']:
                     assert source['source']['namespace']=='jira-cloud-rest-v1'
                     assert source['source']['scope']=='Site-ID'
                     assert source['jira']['observation']=='non_atomic_observation'
                     assert source['jira']['current_state']=='current_state_unverified'
                     assert source['assertion_kind']==0
                     assert source['jira']['coverage'][2]['omitted_prefix']==True
                    assert 'fixture@example.invalid' not in raw and 'local-test-PAT' not in raw
                    ground=context['sources'][0]['evidence_id']
                   if mode=='todoist':
                    assert len(context['sources'])==3
                    for source in context['sources']:
                     assert source['source']['namespace']=='todoist-api-v1'
                     assert source['source']['scope']=='account-1'
                     assert source['todoist']['observation']=='non_atomic_observation'
                     assert source['todoist']['current_state']=='current_state_unverified'
                     assert source['assertion_kind']==0
                    assert 'local-test-PAT' not in raw and 'CANARY' not in raw
                    ground=context['sources'][0]['evidence_id']
                   if mode=='codex':
                    source=context['sources'][0]
                    assert source['source']['namespace']=='codex-local-jsonl-v1'
                    assert source['coverage']=='local_records_only/local_window'
                    assert source['provenance']=='human_unverified'
                    assert source['text']=='exact\nCodex 🌿 prompt'
                    assert source['assertion_kind']==0
                    ground=source['evidence_id']
                   kind=mode if mode in ['clarification','silence'] else 'suggestion'
                   if mode in ['reply','reply-quota']:
                    pairs=context.get('replies',[])
                    assert len(pairs)<=2
                    if pairs:
                     assert pairs[0]['model_question']=='Copy exact 🌿 saved text'
                     assert pairs[0]['user_reply']=='No\x00 🌿'
                     ground=pairs[-1]['evidence_id']
                    kind='suggestion' if len(pairs)==2 else 'clarification'
                   ready=[s for s in context['steps'] if s['ready']]
                   old=context.get('proposals',[])
                   proposal={'proposal_id':old[0]['proposal_id'] if old else None,'kind':kind,'message':'Copy exact 🌿 saved text','rationale':'From selected saved content','evidence_ids':['note:999999'] if mode=='wrong-citation' else ([ground,context['notes'][0]['evidence_id']] if mode=='branch' else [ground]),'step_id':ready[0]['evidence_id'] if ready and kind=='suggestion' else None}
                   proposals=[] if kind=='silence' else [proposal]
                   if mode=='visual-feed':
                    title=context['task']['text']
                    if title.startswith('Починить'):
                     proposal=dict(proposal,kind='clarification',message='Шаг 2 упирается в PR #482. Проверять сначала ветку fix/session-refresh или продолжить на main?')
                    elif title.startswith('Ревью'):
                     proposal=dict(proposal,message='На карточке предложения виден номер дела. Покажите название в одну строку, а номер оставьте в подсказке.')
                    else:
                     proposal=dict(proposal,message='В ответе стоит назвать оба предела: 16 KiB на ответ и 256 KiB контекста, и объяснить, почему запрос ограничен 320 KiB.')
                    proposal['rationale']='Основание — сохранённый текст дела.'
                    proposals=[proposal]
                   if mode=='feed':
                    first=dict(proposal,kind='clarification',message='first' if not old else 'rephrased')
                    second=dict(proposal,proposal_id=old[1]['proposal_id'] if old else None,kind='suggestion',message='second')
                    proposals=[second,first,dict(proposal,proposal_id=None,message='third')] if old else [first,second]
                   text=json.dumps({'proposals':proposals,'rationale':'Complete set'},ensure_ascii=False)
                   if mode=='quota' or (mode=='reply-quota' and context.get('replies')):
                    body=b'{"error":{"code":"insufficient_quota"}}'
                    self.send_response(429);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
                   if mode=='disconnect':
                    self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length','0');self.end_headers();return
                  item={'id':'message-test','type':'message','status':'completed','role':'assistant','content':[{'type':'output_text','text':text,'annotations':[]}]}
                  if mode=='refusal' and raw!='Reply exactly: AMBIENT_OK': item['content']=[{'type':'refusal','refusal':'Test refusal'}]
                  reasoning={'effort':'low'} if mode=='mismatch' and raw!='Reply exactly: AMBIENT_OK' else request['reasoning']
                  response={'status':'completed','model':'gpt-6-luna','reasoning':reasoning,'service_tier':'fast','output':[item],'usage':{'input_tokens':8,'output_tokens':3,'total_tokens':11}}
                  terminal={'type':'response.completed','response':response}
                  if mode=='incomplete' and raw!='Reply exactly: AMBIENT_OK':
                   terminal['type']='response.incomplete';response['status']='incomplete'
                  frames=[{'type':'response.output_item.done','output_index':0,'item':item},terminal]
                  body=''.join('data: '+json.dumps(v,ensure_ascii=False)+'\r\n\r\n' for v in frames).encode()
                  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(body)));self.end_headers()
                  for n in range(0,len(body),17): self.wfile.write(body[n:n+17]);self.wfile.flush()
                server=HTTPServer(('127.0.0.1',0),Handler)
                print('http://127.0.0.1:'+str(server.server_port)+'/responses',flush=True)
                server.serve_forever()
                """#, mode, requestsURL.path,
            ]
            process.standardOutput = output
            let errors = Pipe()
            process.standardError = errors
            try process.run()
            var data = Data()
            while data.last != 10 {
                guard let byte = try output.fileHandleForReading.read(upToCount: 1), !byte.isEmpty,
                    data.count < 4096
                else { throw ModelCheckFailure.native }
                data.append(byte)
            }
            guard let line = String(data: data, encoding: .utf8) else {
                throw ModelCheckFailure.native
            }
            url = line.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        func stop() {
            defer { try? FileManager.default.removeItem(at: requestsURL) }
            if process.isRunning {
                process.terminate()
                process.waitUntilExit()
            }
        }
    }
    actor LocalAssistanceBridge: ModelCheckProtocol {
        private nonisolated let queue = DispatchSerialQueue(label: "ambient.test.local-model")
        nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
        let url: String
        private var holdClose = false
        private var waitingClose = false
        private var entered: CheckedContinuation<Void, Never>?
        private var release: CheckedContinuation<Void, Never>?
        init(url: String) { self.url = url }
        func holdNextClose() { holdClose = true }
        func waitForClose() async {
            if !waitingClose { await withCheckedContinuation { entered = $0 } }
        }
        func releaseClose() {
            release?.resume()
            release = nil
        }
        func dispatch(_ handle: UInt64, access: String, keys: [PluginDispatchKey], root: URL)
            throws -> ModelOperationResult
        {
            let token = Array(access.utf8)
            let endpoint = Array(url.utf8)
            var out = AmbientModelCompletion()
            let texts = keys.flatMap { [Array($0.server.utf8), Array($0.key.utf8)] }
            try PluginBridge.withTexts(texts + [Array(root.path.utf8)]) { k in
                let raw = keys.indices.map {
                    AmbientPluginKey(
                        plugin: keys[$0].plugin, server: k[2 * $0], generation: keys[$0].generation,
                        key: k[2 * $0 + 1])
                }
                try token.withUnsafeBufferPointer { t in
                    try endpoint.withUnsafeBufferPointer { u in
                        try raw.withUnsafeBufferPointer { r in
                            try ModelCheckBridge.check(
                                ambient_model_test_dispatch(
                                    handle, t.baseAddress, UInt32(t.count), r.baseAddress,
                                    UInt32(r.count), k[2 * keys.count], u.baseAddress,
                                    UInt32(u.count), &out))
                        }
                    }
                }
            }
            return try ModelCheckBridge.decodeCompletion(out)
        }
        nonisolated func cancel(_ handle: UInt64) { _ = ambient_model_v6_cancel(handle) }
        func close(_ handle: UInt64) async throws {
            if holdClose {
                holdClose = false
                waitingClose = true
                entered?.resume()
                entered = nil
                await withCheckedContinuation { release = $0 }
            }
            try ModelCheckBridge.check(ambient_model_v6_close(handle))
        }
    }
    @Test @MainActor func selectedAssistanceRunsRealLocalHTTPAndCopiesOnlyCurrentReceipt()
        async throws
    {
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let bridge = LocalAssistanceBridge(url: server.url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        let t = try await memory.createTask("saved intent")
        _ = try await memory.addNote(to: t, text: "saved manual note", at: 1)
        let task = try #require(try await memory.load(selectedTaskID: t.id).selectedTask)
        let note = try #require(try await memory.evidence(taskID: t.id).items.first)
        try await coordinator.setCreditReport(true)
        #expect(try await coordinator.runCheck(profile: .low).outcome == .confirmed)
        await #expect(throws: ModelCheckFailure.capability) {
            _ = try await coordinator.prepareAssistance(task, notes: [note.id])
        }
        #expect(try await coordinator.runCheck(profile: .high).outcome == .confirmed)
        let preview = try await coordinator.prepareAssistance(task, notes: [note.id])
        #expect(preview.context.contains("saved manual note"))
        let publicationResult = try await coordinator.runAssistance(preview.requestID)
        let result = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == publicationResult.publishedTaskID
            })
        #expect(result.kind == .suggestion)
        #expect(
            (try await coordinator.proposalDetails(result.target)).citations.map(\.text) == [
                "saved manual note"
            ])
        #expect(try await coordinator.copyProposal(result.target) == "Copy exact 🌿 saved text")
        let editor = AssistantModel(session: memory)
        await editor.reload()
        editor.draft = "unsaved independent draft"
        _ = try await memory.editTask(task, text: "changed saved intent")
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.copyProposal(result.target)
        }
        #expect(editor.draft == "unsaved independent draft")
        let summary = try await coordinator.checkSummary()
        #expect(summary.attempts == 3 && summary.total == 33 && summary.unknown == 0)
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func selectedAssistanceLocalHTTPKindsAndErrorsKeepRealAccounting() async throws
    {
        for mode in [
            "clarification", "silence", "wrong-citation", "mismatch", "refusal", "incomplete",
            "quota", "disconnect",
        ] {
            let server = try AssistanceHTTPServer(mode: mode)
            defer { server.stop() }
            let url = FileManager.default.temporaryDirectory.appendingPathComponent(
                UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: url) }
            let memory = MemorySession(databaseURL: url)
            let coordinator = ChatGPTCoordinator(
                store: AssistanceTestStore(), memory: memory,
                modelBridge: LocalAssistanceBridge(url: server.url))
            let task = try await memory.createTask("saved intent")
            try await coordinator.setCreditReport(true)
            _ = try await coordinator.runCheck(profile: .high)
            let preview = try await coordinator.prepareAssistance(task, notes: [])
            if mode == "clarification" || mode == "silence" {
                _ = try await coordinator.runAssistance(preview.requestID)
                let rows = try await coordinator.proposalFeed()
                #expect(mode == "silence" ? rows.isEmpty : rows.first?.kind == .clarification)
            } else {
                do {
                    _ = try await coordinator.runAssistance(preview.requestID)
                    Issue.record("Invalid answer published")
                } catch {}
                #expect(try await coordinator.proposalFeed().isEmpty)
                #expect(try await coordinator.assistanceAvailable() == false)
            }
            let summary = try await coordinator.checkSummary()
            #expect(summary.attempts == 2)
            #expect(summary.total == (mode == "quota" || mode == "disconnect" ? 11 : 22))
            #expect(summary.unknown == (mode == "quota" || mode == "disconnect" ? 1 : 0))
            try await coordinator.shutdown()
            try await memory.close()
            let reopened = MemorySession(databaseURL: url)
            #expect(try await reopened.checkSummary().unknown == summary.unknown)
            #expect(try await reopened.assistanceAvailable() == false)
            try await reopened.close()
        }
    }
    @Test @MainActor func selectedAssistanceSignOutDuringRealCloseFencesLateResultAndKeepsBusy()
        async throws
    {
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let bridge = LocalAssistanceBridge(url: server.url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        let task = try await memory.createTask("intent")
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        let preview = try await coordinator.prepareAssistance(task, notes: [])
        await bridge.holdNextClose()
        let operation = Task { try await coordinator.runAssistance(preview.requestID) }
        await bridge.waitForClose()
        try await coordinator.signOut()
        #expect(try await coordinator.load() == .cancelling)
        await #expect(throws: ModelCheckFailure.busy) {
            _ = try await coordinator.runCheck(profile: .high)
        }
        await bridge.releaseClose()
        await #expect(throws: ChatGPTFailure.cancelled) { _ = try await operation.value }
        #expect(try await coordinator.checkSummary().total == 22)
        #expect(try await memory.assistanceAvailable() == false)
        try await coordinator.shutdown()
        try await memory.close()
    }

    @Test @MainActor func selectedAssistanceLocalHTTPRejectsStaleAndOversizeBeforeAnotherPOST()
        async throws
    {
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let bridge = LocalAssistanceBridge(url: server.url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
        var task = try await memory.createTask("saved intent")
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        let preview = try await coordinator.prepareAssistance(task, notes: [])
        let handle = try await memory.beginAssistance(preview.requestID, at: 1)
        task = try await memory.editTask(task, text: "changed intent")
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await bridge.dispatch(
                handle, access: "test-only-access", keys: [],
                root: FileManager.default.temporaryDirectory)
        }
        try await bridge.close(handle)
        #expect(try await memory.checkSummary().attempts == 1)
        #expect(try await coordinator.assistanceAvailable())
        var ids: [UInt64] = []
        for _ in 0..<10 {
            let note = try await memory.addNote(
                to: task, text: String(repeating: "\\", count: 8192), at: 1)
            ids.append(note)
            task = try #require(try await memory.load(selectedTaskID: task.id).selectedTask)
        }
        // Context fits 256KiB, but re-escaping its backslashes exceeds the 320KiB HTTP body cap.
        await #expect(throws: ModelCheckFailure.limit) {
            _ = try await coordinator.prepareAssistance(task, notes: ids)
        }
        #expect(try await memory.checkSummary().attempts == 1)
        #expect(try await coordinator.assistanceAvailable())
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func selectedAssistanceSelectionChangeDiscardsPreviewAndPreservesEditor()
        async throws
    {
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let task = try await memory.createTask("saved intent")
        let note = try await memory.addNote(to: task, text: "saved note", at: 1)
        let saved = try #require(try await memory.load(selectedTaskID: task.id).selectedTask)
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        let chat = ChatGPTModel(coordinator: coordinator)
        let editor = AssistantModel(session: memory)
        await chat.reload()
        await editor.reload()
        editor.draft = "independent draft"
        await chat.prepareAssistance(saved)
        #expect(chat.assistancePreview != nil)
        await chat.selectAssistanceNote(note, selected: true)
        #expect(chat.assistancePreview == nil && chat.feed.isEmpty)
        await chat.sendAssistance(saved)
        #expect(chat.assistanceFailure == .stale)
        #expect(editor.draft == "independent draft")
        #expect(try await coordinator.checkSummary().attempts == 1)
        try await coordinator.shutdown()
        try await memory.close()
    }

    @Test @MainActor func replyUsesRealCorePreviewOneSendTemporaryAttributionAndIndependentDraft()
        async throws
    {
        let server = try AssistanceHTTPServer(mode: "reply")
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let task = try await memory.createTask("saved intent")
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        let chat = ChatGPTModel(coordinator: coordinator)
        let editor = AssistantModel(session: memory)
        await editor.reload()
        editor.draft = "independent memory draft"
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        let parent = try #require(chat.feed.first)
        #expect(parent.kind == .clarification)
        let before = try Data(contentsOf: url)
        chat.updateReply("Yes")
        chat.updateReply("No\0 🌿")
        await chat.waitForReplyPreview()
        let preview = try #require(chat.replyPreview)
        #expect(preview.context.contains("model_question"))
        #expect(try Data(contentsOf: url) == before)
        #expect(try await coordinator.checkSummary().attempts == 2)
        await chat.sendReply()
        let result = try #require(chat.feed.first)
        #expect(result.target != parent.target)
        let requests = try server.requestContexts()
        #expect(requests.count == 3)
        #expect(requests[2].utf8.elementsEqual(preview.context.utf8))
        #expect(
            (try await coordinator.proposalDetails(result.target)).citations.first?.reference.kind
                == "reply")
        #expect(
            (try await coordinator.proposalDetails(result.target)).citations.first?.text == "No\0 🌿"
        )
        #expect(
            (try await coordinator.proposalDetails(result.target)).citations.first?.modelQuestion
                == "Copy exact 🌿 saved text")
        #expect(chat.replyDraft.isEmpty && chat.replyPreview == nil)
        await chat.sendReply()
        #expect(try await coordinator.checkSummary().attempts == 3)
        chat.updateReply("Second reply")
        await chat.waitForReplyPreview()
        await chat.sendReply()
        #expect(chat.feed.first?.kind == .suggestion)
        let current = try #require(chat.feed.first)
        #expect(try await coordinator.copyProposal(current.target) == "Copy exact 🌿 saved text")
        #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
        #expect(
            try await memory.load(selectedTaskID: task.id).selectedTask?.revision == task.revision)
        #expect(editor.draft == "independent memory draft")
        #expect(try await coordinator.checkSummary().attempts == 4)
        try await coordinator.shutdown()
        try await memory.close()
    }

    @Test @MainActor func replyDraftSurvivesNewTargetPauseAndSourceRevocationWithoutRetargeting()
        async throws
    {
        let fixture = try LocalGitFixture()
        defer { fixture.remove() }
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let memory = MemorySession(
            databaseURL: fixture.container.appendingPathComponent("memory.sqlite3"))
        let coordinator = try await localReady(memory: memory, server: server)
        let reader = LocalGitReader()
        let root = try await memory.saveLocalRoot(
            path: reader.select(fixture.root), expected: 0, exclusions: [])
        let first = try await memory.createTask("first intent")
        let op = try await coordinator.beginLocalRead(first, root: root)
        _ = try await reader.read(op)
        let source = try await coordinator.bindLocalBranch(op, index: 0)
        try op.release()
        let task = try await memory.task(first.id)
        let preview = try await coordinator.prepareAssistance(task, notes: [source])
        let publicationParent = try await coordinator.runAssistance(preview.requestID)
        let parent = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == publicationParent.publishedTaskID
            })
        #expect(parent.citations.allSatisfy { $0.kind != "source" })
        let reply = try await coordinator.prepareContinuation(parent, text: "No")
        #expect(reply.context.contains("local-git-branch-v1"))
        try await memory.deleteLocalRoot(root)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.runAssistance(reply.requestID)
        }
        #expect(try await coordinator.checkSummary().attempts == 2)
        _ = try await memory.saveLocalRoot(path: root.path, expected: 0, exclusions: [])
        let chat = ChatGPTModel(coordinator: coordinator)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        chat.updateReply("unsent independent reply")
        await chat.waitForReplyPreview()
        #expect(chat.canSendReply && chat.edgeVisible)
        let firstCard = try #require(chat.feed.first)
        await chat.setVisibility(firstCard, visible: false)
        #expect(chat.feed.first?.visible == false && chat.canSendReply)
        await chat.setVisibility(firstCard, visible: true)
        let other = try await memory.createTask("other intent")
        let target = chat.replyParent
        await chat.prepareAssistance(other)
        await chat.sendAssistance(other)
        #expect(chat.replyDraft == "unsent independent reply" && chat.replyParent == target)
        let otherCard = try #require(chat.feed.first { $0.task.taskID == other.id })
        chat.chooseReply(otherCard)
        #expect(chat.replyConflict && chat.replyParent == target)
        await chat.clearDiscussion(firstCard)
        #expect(
            chat.replyIsStale && !chat.canSendReply && chat.replyDraft == "unsent independent reply"
        )
        #expect(!chat.feed.contains { $0.id == firstCard.id })
        await chat.discardReply()
        chat.chooseReply(otherCard)
        chat.updateReply("keep on pause")
        await chat.waitForReplyPreview()
        try await memory.setManualPause(true)
        await chat.refreshCurrentResult()
        #expect(
            !chat.feed.isEmpty && chat.replyPreview == nil && chat.replyParent == otherCard.target)
        #expect(chat.replyDraft == "keep on pause" && !chat.canSendReply)
        try await memory.setManualPause(false)
        await chat.refreshCurrentResult()
        chat.updateReply("still my reply")
        await chat.waitForReplyPreview()
        #expect(chat.replyParent == otherCard.target && chat.replyPreview != nil)
        try await coordinator.shutdown()
        try await memory.close()
        await reader.clear()
    }

    @Test @MainActor func replyFailureAndLogoutDuringCleanupKeepOnlyDisabledUserText() async throws
    {
        for logout in [false, true] {
            let server = try AssistanceHTTPServer(mode: logout ? "reply" : "reply-quota")
            defer { server.stop() }
            let url = FileManager.default.temporaryDirectory.appendingPathComponent(
                UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: url) }
            let memory = MemorySession(databaseURL: url)
            let bridge = LocalAssistanceBridge(url: server.url)
            let coordinator = ChatGPTCoordinator(
                store: AssistanceTestStore(), memory: memory, modelBridge: bridge)
            let task = try await memory.createTask("intent")
            try await coordinator.setCreditReport(true)
            _ = try await coordinator.runCheck(profile: .high)
            let chat = ChatGPTModel(coordinator: coordinator)
            await chat.reload()
            await chat.prepareAssistance(task)
            await chat.sendAssistance(task)
            chat.updateReply("No\0 🌿")
            await chat.waitForReplyPreview()
            if logout {
                await bridge.holdNextClose()
                let sending = Task { await chat.sendReply() }
                await bridge.waitForClose()
                await chat.signOut()
                #expect(chat.busy && !chat.canSendReply)
                await bridge.releaseClose()
                await sending.value
            } else {
                await chat.sendReply()
                #expect(chat.checkSummary?.latest?.outcome == .quota)
                #expect(chat.checkSummary?.unknown == 1)
            }
            #expect(chat.replyDraft == "No\0 🌿")
            #expect(
                chat.replyPreview == nil && chat.replyParent != nil && !chat.feed.isEmpty)
            await chat.sendReply()
            #expect(try server.requestContexts().count == 3)
            #expect(try await coordinator.checkSummary().attempts == 3)
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            try await coordinator.shutdown()
            try await memory.close()
        }
    }

    final class ReplyTestGate: @unchecked Sendable {
        private let condition = NSCondition()
        private var entered = false
        private var opened = false
        func hold() {
            condition.lock()
            entered = true
            condition.broadcast()
            while !opened { condition.wait() }
            condition.unlock()
        }
        func waitUntilEntered() async throws {
            let deadline = ContinuousClock.now + .seconds(5)
            while !condition.withLock({ entered }) {
                guard ContinuousClock.now < deadline else { throw ModelCheckFailure.native }
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func release() {
            condition.withLock {
                opened = true
                condition.broadcast()
            }
        }
    }
    @Test @MainActor func backgroundReceiptValidationDoesNotOccupyTransportOrRejectCopy()
        async throws
    {
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("intent")
        let p = try await coordinator.prepareAssistance(task, notes: [])
        let publicationR = try await coordinator.runAssistance(p.requestID)
        let r = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == publicationR.publishedTaskID
            })
        let gate = ReplyTestGate()
        let held = Task { await memory.holdReplyTest(gate) }
        try await gate.waitUntilEntered()
        let checking = Task { try await coordinator.proposalDetails(r.target) }
        // Coordinator readState is synchronous, so this runs after currentResult suspends at MemorySession.
        for _ in 0..<10 { await Task.yield() }
        #expect(try await coordinator.load() == .saved)
        let copying = Task { try await coordinator.copyProposal(r.target) }
        for _ in 0..<10 { await Task.yield() }
        gate.release()
        await held.value
        #expect(try await copying.value == "Copy exact 🌿 saved text")
        _ = try? await checking.value
        #expect(try await coordinator.load() == .saved)
        #expect(try await coordinator.checkSummary().attempts == 2)
        try await coordinator.shutdown()
        try await memory.close()
    }
    @Test @MainActor func discardingDuringPreparationCannotClearANewerDraftPreview() async throws {
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let coordinator = try await localReady(memory: memory, server: server)
        let task = try await memory.createTask("intent")
        let chat = ChatGPTModel(coordinator: coordinator)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        let gate = ReplyTestGate()
        let held = Task { await memory.holdReplyTest(gate) }
        try await gate.waitUntilEntered()
        chat.updateReply("old")
        let discard = Task { await chat.discardReply() }
        while !chat.replyDraft.isEmpty { await Task.yield() }
        chat.updateReply("new independent text")
        gate.release()
        await held.value
        await discard.value
        await chat.waitForReplyPreview()
        #expect(chat.canSendReply)
        await chat.sendReply()
        #expect(!chat.feed.isEmpty && chat.replyDraft.isEmpty)
        #expect(try await coordinator.checkSummary().attempts == 3)
        try await coordinator.shutdown()
        try await memory.close()
    }

    @Test @MainActor func sessionExpiryReloadReleasesGeneratedContextAndKeepsOnlyUserDraft()
        async throws
    {
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let store = AssistanceTestStore()
        let coordinator = try await localReady(memory: memory, server: server, store: store)
        let task = try await memory.createTask("intent")
        let chat = ChatGPTModel(coordinator: coordinator)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        chat.updateReply("independent user text")
        await chat.waitForReplyPreview()
        #expect(chat.canSendReply)
        store.expireForTest()
        await chat.reload()
        #expect(chat.state == .expired)
        #expect(
            !chat.feed.isEmpty && chat.assistancePreview == nil
                && chat.replyPreview == nil)
        #expect(
            chat.replyParent != nil && chat.replyDraft == "independent user text"
                && !chat.canSendReply)
        #expect(try await coordinator.checkSummary().attempts == 2)
        try await coordinator.shutdown()
        try await memory.close()
    }

    private actor SameIdentityRefresh: ChatGPTProtocol {
        let result: ChatGPTEnvelope
        init(result: ChatGPTEnvelope) { self.result = result }
        func begin(_ envelope: ChatGPTEnvelope, redirect: String?, refresh: Bool) throws -> UInt64 {
            guard refresh else { throw ChatGPTFailure.invalid }
            return 1
        }
        func prepare(_ handle: UInt64) -> String? { nil }
        func callback(_ handle: UInt64, url: String) throws -> String {
            throw ChatGPTFailure.invalid
        }
        func dispatch(_ handle: UInt64, clientID: String) -> ChatGPTEnvelope { result }
        func close(_ handle: UInt64) {}
        nonisolated func cancel(_ handle: UInt64) {}
    }
    @Test @MainActor func selectedAssistanceSameIdentityRefreshRetainsCoreAndDisplayedAvailability()
        async throws
    {
        let server = try AssistanceHTTPServer()
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let store = AssistanceTestStore()
        let envelope = try #require(store.load())
        let coordinator = ChatGPTCoordinator(
            store: store, bridge: SameIdentityRefresh(result: envelope), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let chat = ChatGPTModel(coordinator: coordinator)
        await chat.reportCreditSetting(true)
        chat.checkProfile = .high
        await chat.checkConnection()
        #expect(chat.assistanceReady)
        await chat.connect(refresh: true)
        #expect(try await coordinator.assistanceAvailable())
        #expect(chat.assistanceReady)
        try await coordinator.shutdown()
        try await memory.close()
    }

    @Test @MainActor
    func selectedAssistanceProviderFailurePointsToJournalInsteadOfBlamingSavedData() async throws {
        let server = try AssistanceHTTPServer(mode: "quota")
        defer { server.stop() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let memory = MemorySession(databaseURL: url)
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let task = try await memory.createTask("saved intent")
        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        let chat = ChatGPTModel(coordinator: coordinator)
        await chat.reload()
        await chat.prepareAssistance(task)
        await chat.sendAssistance(task)
        #expect(chat.feed.isEmpty)
        #expect(chat.assistanceFailure?.key == "assistance.error.result")
        #expect(chat.checkSummary?.latest?.outcome == .quota)
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func interruptedNewIntentWaitingForReplyPreparationReleasesBusyState()
        async throws
    {
        for continuing in [false, true] {
            for cancelling in [false, true] {
                let server = try AssistanceHTTPServer(mode: "clarification")
                defer { server.stop() }
                let url = FileManager.default.temporaryDirectory.appendingPathComponent(
                    UUID().uuidString)
                defer { try? FileManager.default.removeItem(at: url) }
                let memory = MemorySession(databaseURL: url)
                let coordinator = try await localReady(memory: memory, server: server)
                let task = try await memory.createTask("intent")
                let chat = ChatGPTModel(coordinator: coordinator)
                await chat.reload()
                await chat.prepareAssistance(task)
                await chat.sendAssistance(task)
                let proposal = try #require(chat.feed.first)
                let gate = ReplyTestGate()
                let held = Task { await memory.holdReplyTest(gate) }
                try await gate.waitUntilEntered()
                chat.updateReply("independent unsent text")
                for _ in 0..<10 { await Task.yield() }
                let newIntent = Task {
                    if continuing {
                        await chat.prepareContinuation(proposal)
                    } else {
                        await chat.prepareAssistance(task)
                    }
                }
                for _ in 0..<10 { await Task.yield() }
                #expect(chat.busy)
                let interrupt = Task {
                    if cancelling { await chat.cancel() } else { await chat.resetAssistance() }
                }
                for _ in 0..<10 { await Task.yield() }
                gate.release()
                await held.value
                await interrupt.value
                await newIntent.value
                #expect(!chat.busy)
                #expect(chat.assistancePreview == nil && chat.replyPreview == nil)
                #expect(
                    chat.replyDraft == "independent unsent text"
                        && chat.replyParent == proposal.target)
                #expect(try server.requestContexts().count == 2)
                try await coordinator.shutdown()
                try await memory.close()
            }
        }
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    extension MemorySession {
        fileprivate func holdReplyTest(_ gate: ReplyTestGate) { gate.hold() }
    }
#endif

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func lateReplyPreparationCannotResurrectPreviewAfterChangingRequestIntent()
        async throws
    {
        for continuing in [false, true] {
            let server = try AssistanceHTTPServer(mode: "clarification")
            defer { server.stop() }
            let url = FileManager.default.temporaryDirectory.appendingPathComponent(
                UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: url) }
            let memory = MemorySession(databaseURL: url)
            let coordinator = try await localReady(memory: memory, server: server)
            let task = try await memory.createTask("intent")
            let chat = ChatGPTModel(coordinator: coordinator)
            await chat.reload()
            await chat.prepareAssistance(task)
            await chat.sendAssistance(task)
            let proposal = try #require(chat.feed.first)
            let gate = ReplyTestGate()
            let held = Task { await memory.holdReplyTest(gate) }
            try await gate.waitUntilEntered()
            chat.updateReply("independent unsent text")
            for _ in 0..<10 { await Task.yield() }
            let newIntent = Task {
                if continuing {
                    await chat.prepareContinuation(proposal)
                } else {
                    await chat.prepareAssistance(task)
                }
            }
            for _ in 0..<10 { await Task.yield() }
            gate.release()
            await held.value
            await newIntent.value
            await chat.waitForReplyPreview()
            #expect(chat.assistancePreview != nil)
            #expect(chat.replyPreview == nil && !chat.canSendReply)
            #expect(
                chat.replyDraft == "independent unsent text" && chat.replyParent == proposal.target)
            #expect(try server.requestContexts().count == 2)
            try await coordinator.shutdown()
            try await memory.close()
        }
    }
#endif

/// A note names the built-in source whose history saved it («из истории GitHub») only for that
/// source's own namespace; the same notes, and user notes, are the ones help can select.
@Test func noteSourceIsKnownOnlyForBuiltInNamespaces() {
    func note(_ namespace: String?) -> MemoryEvidence {
        MemoryEvidence(
            id: 1, taskID: 1, text: "n",
            origin: namespace.map {
                .external(
                    key: MemorySourceKey(namespace: $0, scope: "s", objectID: "o"), version: nil)
            } ?? .user,
            observedAtUTC: nil, completeness: .unknown, kind: .sourceFact)
    }
    let expected: [(String, BuiltInIntegration)] = [
        ("local-git-branch-v1", .localProjects), ("codex-local-jsonl-v1", .codex),
        ("github-com-rest-v1", .github), ("jira-cloud-rest-v1", .jira),
        ("todoist-api-v1", .todoist),
    ]
    for (namespace, source) in expected {
        #expect(note(namespace).source == source)
        #expect(note(namespace).assistanceSelectable)
    }
    #expect(note("visual-fixture").source == nil)
    #expect(!note("visual-fixture").assistanceSelectable)
    #expect(note(nil).source == nil)
    #expect(note(nil).assistanceSelectable)
}
