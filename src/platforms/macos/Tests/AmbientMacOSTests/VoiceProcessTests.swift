import CVoiceProcess
import Darwin
import Foundation
import Testing

@testable import VoiceSupport

@Test func voiceChildActuallySpawnsWithClosedBoundedInput() throws {
    let bytes: [UInt8] = [123, 125]
    let child = bytes.withUnsafeBufferPointer {
        ambient_voice_child_spawn("/usr/bin/true", "/private/tmp", $0.baseAddress, $0.count)
    }
    #expect(child != nil)
    if let child {
        #expect(ambient_voice_child_wait(child) == 0)
        #expect(ambient_voice_child_signal(child, SIGKILL) == 0)
        ambient_voice_child_destroy(child)
    }
}

func voiceScript(_ body: String) throws -> URL {
    let directory = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-script-" + UUID().uuidString)
    try FileManager.default.createDirectory(
        at: directory, withIntermediateDirectories: false,
        attributes: [.posixPermissions: 0o700])
    let url = directory.appendingPathComponent("child")
    let script = """
        #!/usr/bin/python3
        import os,sys,json,struct,time,signal
        n=struct.unpack('>I',sys.stdin.buffer.read(4))[0]
        request=json.loads(sys.stdin.buffer.read(n))
        directory=request['directory']
        \(body)
        """
    try Data(script.utf8).write(to: url)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
    return url
}

@Test func voiceSupervisorWaitsForSuccessAndRejectsErrorAndOversize() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    for (body, expected) in [
        (
            "fd=os.open(directory+'/result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.write(fd,b'{\"status\":\"text\",\"text\":\"public test\"}')\nos.close(fd)",
            true
        ),
        ("sys.exit(3)", false),
        (
            "fd=os.open(directory+'/result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.write(fd,b'x'*32769)\nos.close(fd)",
            false
        ),
    ] {
        let executable = try voiceScript(body)
        defer { try? FileManager.default.removeItem(at: executable.deletingLastPathComponent()) }
        let lease = try await workspace.prepare()
        let supervisor = VoiceSupervisor(executable: executable, deadline: .seconds(5))
        do {
            let result = try await supervisor.run(
                lease, models: URL(fileURLWithPath: "/test-models"))
            #expect(expected)
            #expect(result == .text("public test"))
        } catch { #expect(!expected) }
        #expect(await supervisor.running == false)
        try await workspace.cleanup(lease)
        #expect(!FileManager.default.fileExists(atPath: lease.directory.path))
    }
}

@Test @MainActor func voiceCancellationAndDeadlineKillIgnoredTERMAndWaitForActualExit() async throws
{
    for timeout in [false, true] {
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let executable = try voiceScript(
            "signal.signal(signal.SIGTERM,signal.SIG_IGN)\nfd=os.open(directory+'/ready',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)\nos.write(fd,str(os.getpid()).encode())\nos.close(fd)\ntime.sleep(20)"
        )
        defer { try? FileManager.default.removeItem(at: executable.deletingLastPathComponent()) }
        let workspace = VoiceWorkspace(root: root)
        let lease = try await workspace.prepare()
        let supervisor = VoiceSupervisor(
            executable: executable, deadline: timeout ? .seconds(1) : .seconds(5))
        let task = Task {
            try await supervisor.run(lease, models: URL(fileURLWithPath: "/test-models"))
        }
        let ready = lease.directory.appendingPathComponent("ready")
        try await voiceEventually { FileManager.default.fileExists(atPath: ready.path) }
        let pid = try #require(Int32(String(contentsOf: ready, encoding: .utf8)))
        let started = ContinuousClock.now
        if !timeout { await supervisor.cancel() }
        #expect(await supervisor.running)
        do {
            _ = try await task.value
            Issue.record("Expected cancellation")
        } catch { #expect(error as? VoiceFailure == (timeout ? .timeout : .cancelled)) }
        #expect(started.duration(to: .now) >= .milliseconds(200))
        #expect(await supervisor.running == false)
        #expect(kill(pid, 0) == -1 && errno == ESRCH)
        try await workspace.cleanup(lease)
    }
}

func voiceTestChildURL() -> URL {
    if let path = ProcessInfo.processInfo.environment["AMBIENT_VOICE_TEST_CHILD"] {
        return URL(fileURLWithPath: path)
    }
    return URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().appendingPathComponent(
            ".build/out/Products/Debug/VoiceTestChild")
}

@Test @MainActor func voiceExitCancellationRaceNeverCancelsFollowingOperation() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let supervisor = VoiceSupervisor(executable: voiceTestChildURL(), deadline: .seconds(5))
    for _ in 0..<16 {
        let oldLease = try await workspace.prepare()
        let old = Task(priority: .low) {
            try await supervisor.run(oldLease, models: URL(fileURLWithPath: "/mode/race-success"))
        }
        try await voiceEventually {
            FileManager.default.fileExists(
                atPath: oldLease.directory.appendingPathComponent("ready").path)
        }
        let cancellation = Task(priority: .background) { old.cancel() }
        _ = await old.result
        try await workspace.cleanup(oldLease)
        let nextLease = try await workspace.prepare()
        let next = Task(priority: .high) {
            try await supervisor.run(nextLease, models: URL(fileURLWithPath: "/mode/next-success"))
        }
        await cancellation.value
        #expect(try await next.value == .text("public test"))
        try await workspace.cleanup(nextLease)
    }
}

@Test @MainActor func voiceHelperRejectsDeletedAndReplacedDirectoryAfterIndependentLock()
    async throws
{
    for replacement in [false, true] {
        let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
            "ambient-voice-test-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let workspace = VoiceWorkspace(root: root)
        let lease = try await workspace.prepare()
        try voiceWAV().write(to: lease.audioURL)
        try #require(flock(lease.leaseFD.raw, LOCK_EX | LOCK_NB) == 0)
        let supervisor = VoiceSupervisor(executable: voiceTestChildURL(), deadline: .seconds(5))
        let task = Task {
            try await supervisor.run(lease, models: URL(fileURLWithPath: "/mode/lease-race"))
        }
        try await voiceEventually {
            FileManager.default.fileExists(
                atPath: lease.directory.appendingPathComponent("ready").path)
        }
        try await workspace.cleanup(lease)
        if replacement {
            try FileManager.default.createDirectory(
                at: lease.directory, withIntermediateDirectories: false,
                attributes: [.posixPermissions: 0o700])
            #expect(
                FileManager.default.createFile(
                    atPath: lease.directory.appendingPathComponent("lease").path,
                    contents: Data(), attributes: [.posixPermissions: 0o600]))
            #expect(
                FileManager.default.createFile(
                    atPath: lease.audioURL.path, contents: voiceWAV(),
                    attributes: [.posixPermissions: 0o600]))
        }
        #expect(flock(lease.leaseFD.raw, LOCK_UN) == 0)
        await #expect(throws: VoiceFailure.recognition) { try await task.value }
        #expect(
            !FileManager.default.fileExists(
                atPath: lease.directory.appendingPathComponent("audio-opened").path))
        if replacement { #expect(try Data(contentsOf: lease.audioURL) == voiceWAV()) }
    }
}

@Test @MainActor func voiceIndependentHelperLeaseBlocksCleanupUntilActualKill() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let lease = try await workspace.prepare()
    let supervisor = VoiceSupervisor(executable: voiceTestChildURL(), deadline: .seconds(5))
    let task = Task { try await supervisor.run(lease, models: URL(fileURLWithPath: "/mode/hung")) }
    try await voiceEventually {
        FileManager.default.fileExists(atPath: lease.directory.appendingPathComponent("ready").path)
    }
    #expect(flock(lease.leaseFD.raw, LOCK_UN) == 0)
    await #expect(throws: VoiceFailure.cleanup) { try await workspace.cleanup(lease) }
    #expect(FileManager.default.fileExists(atPath: lease.audioURL.path))
    await supervisor.cancel()
    await #expect(throws: VoiceFailure.cancelled) { try await task.value }
    try await workspace.cleanup(lease)
    #expect(!FileManager.default.fileExists(atPath: lease.directory.path))
}

@Test @MainActor func voiceParentDeathEOFActuallyExitsHungComputeAndReleasesLease() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let lease = try await workspace.prepare()
    let encodedExecutable = String(
        data: try JSONSerialization.data(
            withJSONObject: voiceTestChildURL().path,
            options: [.fragmentsAllowed, .withoutEscapingSlashes]), encoding: .utf8)
    let literal = try #require(encodedExecutable)
    let body = """
        import subprocess,fcntl
        parent_lease=open(directory+'/lease','rb')
        fcntl.flock(parent_lease,fcntl.LOCK_SH)
        child=subprocess.Popen([\(literal)],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,close_fds=True)
        payload=json.dumps(request).encode()
        child.stdin.write(struct.pack('>I',len(payload))+payload);child.stdin.flush()
        end=time.monotonic()+4
        while not os.path.exists(directory+'/ready'):
         if time.monotonic()>end: child.kill();child.wait();sys.exit(3)
         time.sleep(.005)
        fd=os.open(directory+'/child-pid',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
        os.write(fd,str(child.pid).encode());os.close(fd)
        fd=os.open(directory+'/result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
        os.write(fd,b'{"status":"silence"}');os.close(fd)
        os._exit(0)
        """
    let parent = try voiceScript(body)
    defer { try? FileManager.default.removeItem(at: parent.deletingLastPathComponent()) }
    #expect(flock(lease.leaseFD.raw, LOCK_UN) == 0)
    let supervisor = VoiceSupervisor(executable: parent, deadline: .seconds(6))
    #expect(try await supervisor.run(lease, models: URL(fileURLWithPath: "/mode/hung")) == .silence)
    let pid = try #require(
        Int32(
            String(contentsOf: lease.directory.appendingPathComponent("child-pid"), encoding: .utf8)
        ))
    try await voiceEventually { kill(pid, 0) == -1 && errno == ESRCH }
    try await workspace.cleanup(lease)
    #expect(!FileManager.default.fileExists(atPath: lease.directory.path))
}
