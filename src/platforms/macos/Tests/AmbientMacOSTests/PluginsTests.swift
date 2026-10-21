import CryptoKit
import Foundation
import Security
import Testing

@testable import AmbientMacOS

private func folder(_ mcp: String) throws -> URL {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
        "ambient-plugin-" + UUID().uuidString)
    let skills = root.appending(path: "skills/search", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: skills, withIntermediateDirectories: true)
    try Data(#"{"name":"Plans","version":"1.0.0"}"#.utf8).write(
        to: root.appending(path: "plugin.json"))
    try Data(mcp.utf8).write(to: root.appending(path: "mcp.json"))
    try Data("---\nname: search-plans\ndescription: Find plans\n---\nSearch first.\n".utf8).write(
        to: skills.appending(path: "SKILL.md"))
    return root
}
private func hex(_ text: String) -> String {
    SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
}
private let pinned = #"{"name":"search","readOnlyHint":true}"#
private func tool() -> DiscoveredTool {
    DiscoveredTool(
        name: "search", hash: hex(pinned), readOnly: true, approvable: true, description: nil,
        definition: pinned)
}

@Test func pluginEnvelopeUsesExplicitKeysAndRejectsForeignForms() throws {
    let e = PluginEnvelope(
        version: 1, instance: String(repeating: "a", count: 32), pluginID: 7, server: "docs",
        generation: 9, secret: .bearer(token: "plugin-token"))
    let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(e)) as? [String: Any]
    #expect(
        Set(json?.keys.map { $0 } ?? []) == [
            "version", "instance", "plugin_id", "server", "generation", "secret",
        ])
    #expect((json?["secret"] as? [String: Any])?.keys.first == "bearer")
    #expect(e.account == "7/docs")
    try e.validate()
    for bad in [
        PluginEnvelope(
            version: 1, instance: "A", pluginID: 7, server: "docs", generation: 9,
            secret: .bearer(token: "t")),
        PluginEnvelope(
            version: 1, instance: e.instance, pluginID: 7, server: "Docs", generation: 9,
            secret: .bearer(token: "t")),
        PluginEnvelope(
            version: 1, instance: e.instance, pluginID: 7, server: "docs", generation: 0,
            secret: .bearer(token: "t")),
        PluginEnvelope(
            version: 1, instance: e.instance, pluginID: 7, server: "docs", generation: 9,
            secret: .bearer(token: "with space")),
    ] {
        #expect(throws: IntegrationFailure.invalid) { try bad.validate() }
    }
}

@Test func packageReaderReadsBothLayoutsBoundedAndNeverFollowsLinks() throws {
    let root = try folder(#"{"mcpServers":{"docs":{"url":"https://mcp.example.com/mcp"}}}"#)
    defer { try? FileManager.default.removeItem(at: root) }
    let files = try PluginPackageReader.read(folder: root)
    #expect(files.map(\.path) == ["plugin.json", "mcp.json", "skills/search/SKILL.md"])
    let review = try PluginBridge.review(files)
    #expect(review.servers.map(\.key) == ["docs"])
    #expect(review.skills.map(\.name) == ["search-plans"])
    // A linked skill is refused instead of read.
    let outside = root.appending(path: "outside.md")
    try Data("x".utf8).write(to: outside)
    let linked = root.appending(path: "skills/linked", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: linked, withIntermediateDirectories: true)
    try FileManager.default.createSymbolicLink(
        at: linked.appending(path: "SKILL.md"), withDestinationURL: outside)
    #expect(throws: PluginPackageReader.Failure.unreadable) {
        _ = try PluginPackageReader.read(folder: root)
    }
    // The Codex layout and a missing manifest.
    let codex = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: codex) }
    try FileManager.default.createDirectory(
        at: codex.appending(path: ".codex-plugin", directoryHint: .isDirectory),
        withIntermediateDirectories: true)
    #expect(throws: PluginPackageReader.Failure.missing) {
        _ = try PluginPackageReader.read(folder: codex)
    }
    try Data(#"{"name":"Codex"}"#.utf8).write(
        to: codex.appending(path: ".codex-plugin/plugin.json"))
    try Data(#"{"mcpServers":{"docs":{"type":"http","url":"https://x.example/mcp"}}}"#.utf8).write(
        to: codex.appending(path: ".mcp.json"))
    #expect(
        try PluginPackageReader.read(folder: codex).map(\.path) == [
            ".codex-plugin/plugin.json", ".mcp.json",
        ])
}

@Test @MainActor func pluginSecretsGoCoreFirstAndBindToTheDatabaseInstance() async throws {
    let fixture = try IntegrationKeychainFixture()
    defer { fixture.remove() }
    let root = try folder(
        #"{"mcpServers":{"docs":{"url":"https://mcp.example.com/mcp","headers":{"Authorization":"x"}}}}"#
    )
    defer { try? FileManager.default.removeItem(at: root) }
    let store = EnvelopeKeychain.plugins(keychain: fixture.keychain)
    func installed(_ memory: MemorySession) async throws -> PluginRecord {
        let (files, review) = try await memory.reviewPluginFolder(root)
        let id = try await memory.installPlugin(files, hash: review.hash, at: 1)
        return try #require(try await memory.plugins().plugins.first { $0.id == id })
    }
    let memory = MemorySession(databaseURL: fixture.databaseURL())
    let owner = PluginSecretOwner(memory: memory, store: store)
    var p = try await installed(memory)
    let before = try #require(p.servers.first).generation
    p = try await owner.replace(p, server: "docs", token: "plugin-token")
    let server = try #require(p.servers.first)
    #expect(server.generation > before, "Core moves the generation before the Keychain write")
    let instance = try await memory.plugins().instance
    #expect(
        try store.load(
            instance: instance, account: PluginEnvelope.account(plugin: p.id, server: "docs"))?
            .generation
            == server.generation)
    #expect(try await owner.signedIn(p) == ["docs"])
    // Disabled or without approved tools: no key goes to dispatch.
    #expect(try await owner.dispatchKeys().isEmpty)
    _ = try await memory.approvePluginTool(p, server: "docs", tool: tool())
    try await memory.setPluginEnabled(p.id, enabled: true)
    #expect(
        try await owner.dispatchKeys() == [
            PluginDispatchKey(
                plugin: p.id, server: "docs", generation: server.generation, key: "plugin-token")
        ])
    // The same plugin ID and generation in another database never get this key.
    let other = MemorySession(databaseURL: fixture.databaseURL("other"))
    var q = try await installed(other)
    q = try await PluginSecretOwner(memory: other, store: store).replace(
        q, server: "docs", token: "other-token")
    _ = try await other.approvePluginTool(q, server: "docs", tool: tool())
    try await other.setPluginEnabled(q.id, enabled: true)
    #expect(try await owner.dispatchKeys().map(\.key) == ["plugin-token"])
    // Sign-out and removal delete the Keychain entry; saved facts are unaffected here.
    p = try #require(try await memory.plugins().plugins.first { $0.id == p.id })
    p = try await owner.signOut(p, server: "docs").plugin
    #expect(
        try store.load(
            instance: instance, account: PluginEnvelope.account(plugin: p.id, server: "docs"))
            == nil)
    #expect(try await owner.dispatchKeys().isEmpty)
    p = try await owner.replace(p, server: "docs", token: "again-token")
    #expect(try await owner.delete(p), "nothing to revoke at a provider")
    #expect(try await memory.plugins().plugins.isEmpty)
    #expect(
        try store.load(
            instance: instance, account: PluginEnvelope.account(plugin: p.id, server: "docs"))
            == nil)
    try await memory.close()
    try await other.close()
}

#if AMBIENT_MODEL_TEST_SUPPORT
    /// One loopback Python process serving the model (SSE) and an MCP plugin server.
    @MainActor final class PluginHTTPServer {
        let process = Process()
        let base: String
        let capture = FileManager.default.temporaryDirectory.appendingPathComponent(
            UUID().uuidString)
        init() throws {
            let output = Pipe()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
            process.arguments = [
                "-u", "-c",
                #"""
                import json,sys
                from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
                capture=sys.argv[1]
                def sse(items,effort):
                 frames=[{'type':'response.output_item.done','output_index':i,'item':v} for i,v in enumerate(items)]
                 frames.append({'type':'response.completed','response':{'status':'completed','model':'gpt-6-luna','reasoning':{'effort':effort},'service_tier':'priority','output':items,'usage':{'input_tokens':8,'output_tokens':3,'total_tokens':11}}})
                 return ''.join('data: '+json.dumps(v)+'\n\n' for v in frames).encode()
                class Handler(BaseHTTPRequestHandler):
                 def log_message(self,*a): pass
                 def send(self,status,kind,body):
                  self.send_response(status)
                  if kind:self.send_header('Content-Type',kind)
                  self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
                 def do_POST(self):
                  m=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                  auth=self.headers.get('Authorization')
                  with open(capture,'a') as f:f.write(json.dumps({'path':self.path,'auth':auth,'body':m})+'\n')
                  if self.path=='/mcp':
                   assert auth=='Bearer plugin-token'
                   method=m.get('method')
                   if method=='notifications/initialized':return self.send(202,None,b'')
                   result={'initialize':{'protocolVersion':'2025-06-18','capabilities':{'tools':{}},'serverInfo':{'name':'f','version':'1'}},
                    'tools/list':{'tools':[{'name':'search','annotations':{'readOnlyHint':True}}]},
                    'tools/call':{'content':[{'type':'text','text':'Plan v3 ships on Friday'}]}}[method]
                   return self.send(200,'application/json',json.dumps({'jsonrpc':'2.0','id':m['id'],'result':result}).encode())
                  assert self.path=='/responses' and auth=='Bearer test-only-access'
                  raw=m['input'][0]['content']
                  if raw=='Reply exactly: AMBIENT_OK':
                   item={'id':'msg','type':'message','status':'completed','role':'assistant','content':[{'type':'output_text','text':'AMBIENT_OK','annotations':[]}]}
                   return self.send(200,'text/event-stream',sse([item],m['reasoning']['effort']))
                  assert 'plugin-token' not in json.dumps(m)
                  if m['tools'] and len(m['input'])==1:
                   assert m['tools'][0]['tools'][0]['name']=='search'
                   items=[{'id':'rs','type':'reasoning','summary':[]},{'id':'fc','type':'function_call','status':'completed','call_id':'c1','name':'search','arguments':'{"q":"plan"}'}]
                   return self.send(200,'text/event-stream',sse(items,'high'))
                  assert m['input'][-1]['type']=='function_call_output'
                  task=json.loads(raw)['task']['evidence_id']
                  text=json.dumps({'proposals':[{'proposal_id':None,'kind':'clarification','message':'Ship Friday?','rationale':'Plugin result','evidence_ids':[task,'tool:1'],'step_id':None}],'rationale':'Complete set'})
                  item={'id':'msg','type':'message','status':'completed','role':'assistant','content':[{'type':'output_text','text':text,'annotations':[]}]}
                  self.send(200,'text/event-stream',sse([item],'high'))
                server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
                print('http://127.0.0.1:'+str(server.server_port),flush=True)
                server.serve_forever()
                """#, capture.path,
            ]
            process.standardOutput = output
            process.standardError = FileHandle.nullDevice
            try process.run()
            var data = Data()
            while data.last != 10 {
                guard let byte = try output.fileHandleForReading.read(upToCount: 1), !byte.isEmpty,
                    data.count < 4096
                else { throw ModelCheckFailure.native }
                data.append(byte)
            }
            base = String(decoding: data, as: UTF8.self).trimmingCharacters(
                in: .whitespacesAndNewlines)
        }
        func requests() throws -> [[String: Any]] {
            guard FileManager.default.fileExists(atPath: capture.path) else { return [] }
            return try String(contentsOf: capture, encoding: .utf8).split(separator: "\n")
                .compactMap {
                    try JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any]
                }
        }
        func stop() {
            defer { try? FileManager.default.removeItem(at: capture) }
            if process.isRunning {
                process.terminate()
                process.waitUntilExit()
            }
        }
    }

    @Test @MainActor func pluginToolResultIsCalledCitedSavedAndClearedOnDisable() async throws {
        let server = try PluginHTTPServer()
        defer { server.stop() }
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let root = try folder(
            #"{"mcpServers":{"docs":{"url":"\#(server.base)/mcp","headers":{"Authorization":"x"}}}}"#
        )
        defer { try? FileManager.default.removeItem(at: root) }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let secrets = PluginSecretOwner(
            memory: memory, store: EnvelopeKeychain.plugins(keychain: fixture.keychain))
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.base + "/responses"),
            plugins: secrets)
        let chatGPT = ChatGPTModel(coordinator: coordinator)
        let plugins = chatGPT.plugins
        // Review, token, install, discovery, approval of the declared read-only tool, enable.
        await plugins.open(root)
        #expect(plugins.draft?.review.servers.first?.needsKey == true)
        plugins.draft?.tokens["docs"] = "plugin-token"
        await plugins.installDraft()
        #expect(plugins.draft?.discoveries["docs"]?.tools?.map(\.name) == ["search"])
        #expect(plugins.draft?.selected == ["docs/search"])
        await plugins.enableDraft()
        #expect(plugins.failureKey == nil)
        let p = try #require(plugins.plugins.first)
        #expect(p.enabled && p.tools.map(\.name) == ["search"])
        // One assistance request: two POSTs, one call, the cited result saved with provenance.
        let t = try await memory.createTask("Find the launch plan")
        let task = try #require(try await memory.load(selectedTaskID: t.id).selectedTask)
        try await coordinator.setCreditReport(true)
        #expect(try await coordinator.runCheck(profile: .high).outcome == .confirmed)
        let preview = try await coordinator.prepareAssistance(task, notes: [])
        #expect(preview.context.contains("\"plugins\""))
        let published = try await coordinator.runAssistance(preview.requestID)
        let proposal = try #require(
            try await coordinator.proposalFeed().first {
                $0.task.taskID == published.publishedTaskID
            })
        let citations = try await coordinator.proposalDetails(proposal.target).citations
        let cited = try #require(citations.first { $0.plugin != nil })
        #expect(cited.text == "Plan v3 ships on Friday")
        #expect(cited.plugin?.plugin == "Plans" && cited.plugin?.tool == "search")
        let model = try server.requests().filter { $0["path"] as? String == "/responses" }
        #expect(model.count == 3, "connectivity check and two assistance POSTs")
        await plugins.load()
        #expect(plugins.calls.first?.outcome == 1)
        // Disabling clears the dependent discussion; the saved fact stays in memory.
        await plugins.setEnabled(p, enabled: false, chatGPT: chatGPT)
        #expect(try await coordinator.proposalFeed().isEmpty)
        #expect(
            try await memory.evidence(taskID: t.id).items.contains {
                $0.text == "Plan v3 ships on Friday"
            })
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif
