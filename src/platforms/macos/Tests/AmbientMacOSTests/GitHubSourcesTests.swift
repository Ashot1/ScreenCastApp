import CAmbient
import Foundation
import Testing

@testable import AmbientMacOS

typealias GitHubWindow = IntegrationWindow<GitHubObject, GitHubCoverage>
typealias GitHubPreview = IntegrationPreview<GitHubObject>

#if AMBIENT_MODEL_TEST_SUPPORT
    private let githubHTTPScript = #"""
        import json,sys,time,os
        from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
        from urllib.parse import urlsplit,parse_qs
        mode,capture=sys.argv[1:]
        detail=0
        class Handler(BaseHTTPRequestHandler):
         def log_message(self,*args): pass
         def do_GET(self):
          global detail
          assert self.headers['Authorization']=='Bearer local-test-PAT'
          assert self.headers['X-GitHub-Api-Version']=='2026-03-10'
          assert self.headers['Accept']=='application/vnd.github+json'
          assert self.headers['User-Agent']=='Ambient/0.1.0'
          with open(capture,'a') as f:f.write(json.dumps(self.path)+'\n')
          path=urlsplit(self.path).path
          page=int(parse_qs(urlsplit(self.path).query).get('page',['1'])[0])
          if mode=='hold':
           while not os.path.exists(capture+'.release'):time.sleep(.01)
          if mode in ['401','403','429']:
           self.send_response(int(mode));self.send_header('Retry-After','120');self.send_header('Content-Length','0');self.end_headers();return
          link=None
          if path=='/repos/Example/Project/pulls/3':
           detail+=1
           body={'id':20,'number':3,'title':'Initial title' if detail==1 else 'Final title','body':None,'user':{'login':'author'},'created_at':'2026-10-20T01:00:00Z','updated_at':'2026-10-20T02:00:00Z','state':'open','merged':False,'head':{'sha':'changed' if mode=='head-change' and detail>1 else 'opaque-head'},'base':{'sha':'opaque-base','repo':{'id':10,'full_name':'Example/Project'}}}
          elif path=='/repos/Example/Project/pulls/3/reviews':
           if mode=='category404':self.send_response(404);self.send_header('Content-Length','0');self.end_headers();return
           body=[{'id':30,'state':'COMMENTED','body':'review exact 🌿','user':{'login':'reviewer'},'commit_id':None,'submitted_at':'2026-10-20T02:00:00Z'}] if page==1 else [{'id':31,'state':'APPROVED','body':'','user':None,'commit_id':None}]
           if page==1:link='<https://api.github.com/repos/Example/Project/pulls/3/reviews?per_page=100&page=2>; rel="next"'
           if mode=='bad-link':link='<http://127.0.0.1:1/forbidden>; rel="next"'
          elif path=='/repos/Example/Project/pulls/3/comments':
           body=[{'id':40,'body':'inline exact','user':None,'created_at':'2026-10-20T01:00:00Z','updated_at':'2026-10-20T02:00:00Z','pull_request_review_id':None,'path':'file.swift','commit_id':'commit','original_commit_id':'original','original_start_line':None,'side':'FUTURE'}]
          elif path=='/repos/Example/Project/issues/3/comments':
           body=[{'id':50,'body':'general exact','user':None,'created_at':'2026-10-20T01:00:00Z','updated_at':'2026-10-20T02:00:00Z'},{'id':51,'user':None,'created_at':'2026-10-20T01:00:00Z','updated_at':'2026-10-20T02:00:00Z'}]
          else:raise AssertionError('unexpected path')
          data=json.dumps(body,ensure_ascii=False).encode()
          self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)))
          if link:self.send_header('Link',link)
          self.end_headers()
          try:self.wfile.write(data)
          except BrokenPipeError:pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        print('http://127.0.0.1:'+str(server.server_port),flush=True)
        server.serve_forever()
        """#
    private let githubRepository = [GitHubRepository(owner: "Example", repository: "Project")]
    private let githubURL = "https://github.com/Example/Project/pull/3"
    @Test @MainActor func githubNativeKeychainHTTPBatchAnd08BAssistanceUseTheSameStore()
        async throws
    {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let server = try IntegrationHTTPServer(
            script: githubHTTPScript, mode: "standard", directory: fixture.directory)
        defer { server.stop() }
        let model = try AssistanceHTTPServer(mode: "github")
        defer { model.stop() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let coordinator = try await localReady(memory: memory, server: model)
        let owner = integrationOwner(memory, fixture, server)
        let original = try await memory.saveGitHubProfile(
            id: 0, expected: 0, name: "Work",
            repositories: githubRepository)
        let profile = try await owner.replace(
            original, secret: .githubToken(token: "local-test-PAT"))
        let task = try await memory.createTask("Discuss selected PR objects")
        let op = try await coordinator.beginIntegrationRead(
            task, profile: profile, target: githubURL)
        let window: GitHubWindow = try await owner.read(op)
        #expect(window.objects.count == 5)
        #expect(window.categories[0].pages == 2 && window.categories[2].empty == 1)
        #expect(
            window.objects[0].text.contains("Final title")
                && !window.objects[0].text.contains("Initial title"))
        #expect(
            window.objects[3].metadata.fields.originalStartLine == nil
                && window.objects[3].metadata.fields.side?.unknown == true)
        #expect(try server.paths().count == 6)
        await #expect(throws: IntegrationFailure.invalid) {
            _ = try await memory.bindIntegrationObjects(op, indices: [0, 1, 2])
        }
        let preview: GitHubPreview = try await memory.integrationPreview(op, indices: [0, 1, 2])
        #expect(preview.objects.count == 3 && preview.json.contains("APPROVED"))
        let ids = try await coordinator.bindIntegrationObjects(op, indices: [0, 1, 2])
        try op.release()
        #expect(ids.count == 3)
        let current = try await memory.task(task.id)
        let evidence = try await memory.evidence(taskID: task.id)
        #expect(evidence.items.allSatisfy { $0.assistanceSelectable })
        let context = try await coordinator.prepareAssistance(current, notes: ids)
        #expect(
            context.context.contains("selected_objects")
                && !context.context.contains("local-test-PAT"))
        _ = try await coordinator.runAssistance(context.requestID)
        let proposal = try #require(try await coordinator.proposalFeed().first)
        let details = try await coordinator.proposalDetails(proposal.target)
        guard case .github(let cited) = details.citations.first?.integration else {
            Issue.record("cited GitHub metadata missing")
            return
        }
        #expect(
            cited.coverage == "selected_objects" && cited.repositoryFullName == "Example/Project")
        #expect(details.citations.first?.sourceObservedAtUTC == window.objects[0].observedAtUTC)
        #expect(try await coordinator.copyProposal(proposal.target) == "Copy exact 🌿 saved text")
        let metadata: IntegrationSavedPage<GitHubMetadata> =
            try await memory
            .integrationSavedMetadata(.github, taskID: task.id)
        #expect(metadata.rows.count == 3)
        let revoked = try await owner.replace(
            profile, secret: .githubToken(token: "replacement-test-PAT"))
        #expect(revoked.credentialGeneration != profile.credentialGeneration)
        #expect(try await coordinator.proposalFeed().isEmpty)
        await #expect(throws: ModelCheckFailure.stale) {
            _ = try await coordinator.copyProposal(proposal.target)
        }
        #expect(try await memory.evidence(taskID: task.id).items.count == 3)
        for file in [fixture.databaseURL()] {
            let data = try Data(contentsOf: file)
            #expect(data.range(of: Data("local-test-PAT".utf8)) == nil)
            #expect(data.range(of: Data("replacement-test-PAT".utf8)) == nil)
        }
        #expect(
            try model.requestContexts().allSatisfy {
                !$0.contains("local-test-PAT") && !$0.contains("replacement-test-PAT")
            })
        try await coordinator.shutdown()
        try await memory.close()
        let reopened = MemorySession(databaseURL: fixture.databaseURL())
        let again: IntegrationSavedPage<GitHubMetadata> =
            try await reopened
            .integrationSavedMetadata(.github, taskID: task.id)
        #expect(again == metadata)
        #expect(try await reopened.proposalFeed().isEmpty)
        try await reopened.close()
    }
    @Test @MainActor func githubHTTPAccessRateIdentityAndCategoryCoverageStayExplicit() async throws
    {
        for mode in ["401", "403", "429", "head-change", "category404", "bad-link"] {
            let fixture = try IntegrationKeychainFixture()
            defer { fixture.remove() }
            let server = try IntegrationHTTPServer(
                script: githubHTTPScript, mode: mode, directory: fixture.directory)
            defer { server.stop() }
            let model = try AssistanceHTTPServer()
            defer { model.stop() }
            let memory = MemorySession(databaseURL: fixture.databaseURL())
            let coordinator = try await localReady(memory: memory, server: model)
            let owner = integrationOwner(memory, fixture, server)
            let original = try await memory.saveGitHubProfile(
                id: 0, expected: 0, name: "Work",
                repositories: githubRepository)
            let profile = try await owner.replace(
                original, secret: .githubToken(token: "local-test-PAT"))
            let task = try await memory.createTask("Discuss")
            let op = try await coordinator.beginIntegrationRead(
                task, profile: profile, target: githubURL)
            if let code = UInt32(mode) {
                await #expect(throws: IntegrationFailure.response(code)) {
                    let _: GitHubWindow = try await owner.read(op)
                }
                #expect(try server.paths().count == 1)
                let after = try await memory.integrationProfile(profile.id, kind: .github)
                if code == 401 {
                    #expect(after.credentialGeneration != profile.credentialGeneration)
                    #expect(try await owner.hasCredential(after) == false)
                } else {
                    #expect(after.notBeforeUTC ?? 0 >= Int64(Date().timeIntervalSince1970) + 115)
                    #expect(after.credentialGeneration == profile.credentialGeneration)
                }
            } else if mode == "head-change" {
                await #expect(throws: IntegrationFailure.response(409)) {
                    let _: GitHubWindow = try await owner.read(op)
                }
            } else {
                let window: GitHubWindow = try await owner.read(op)
                #expect(
                    window.categories[0].status
                        == (mode == "category404" ? "unavailable" : "pagination"))
                #expect(try server.paths().count == 5)
            }
            try op.release()
            #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
            try await coordinator.shutdown()
            try await memory.close()
        }
    }
    @Test @MainActor func githubCancellationRetainsTheJobUntilRealTransportCleanup() async throws {
        let fixture = try IntegrationKeychainFixture()
        defer { fixture.remove() }
        let server = try IntegrationHTTPServer(
            script: githubHTTPScript, mode: "hold", directory: fixture.directory)
        defer { server.stop() }
        let model = try AssistanceHTTPServer()
        defer { model.stop() }
        let memory = MemorySession(databaseURL: fixture.databaseURL())
        let coordinator = try await localReady(memory: memory, server: model)
        let owner = integrationOwner(memory, fixture, server)
        let p = try await memory.saveGitHubProfile(
            id: 0, expected: 0, name: "Work",
            repositories: githubRepository)
        let profile = try await owner.replace(p, secret: .githubToken(token: "local-test-PAT"))
        let task = try await memory.createTask("Discuss")
        let op = try await coordinator.beginIntegrationRead(
            task, profile: profile, target: githubURL)
        let read = Task { () -> GitHubWindow in try await owner.read(op) }
        try await server.waitForRequest()
        #expect(ambient_integration_v1_release(op.memory, op.id) == 4)
        op.cancel()
        await #expect(throws: IntegrationFailure.response(499)) { _ = try await read.value }
        try op.release()
        #expect(try await memory.evidence(taskID: task.id).items.isEmpty)
        try await coordinator.shutdown()
        try await memory.close()
    }
#endif
