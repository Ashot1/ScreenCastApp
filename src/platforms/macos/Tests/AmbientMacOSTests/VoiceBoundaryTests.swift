import Darwin
import Foundation
import Testing

@testable import VoiceSupport

func voiceWAV(samples: [Int16] = [0, 16384, -16384], rate: UInt32 = 16000) -> Data {
    var data = Data()
    func text(_ value: String) { data.append(contentsOf: value.utf8) }
    func u16(_ value: UInt16) {
        data.append(UInt8(truncatingIfNeeded: value))
        data.append(UInt8(truncatingIfNeeded: value >> 8))
    }
    func u32(_ value: UInt32) {
        u16(UInt16(truncatingIfNeeded: value))
        u16(UInt16(truncatingIfNeeded: value >> 16))
    }
    text("RIFF")
    u32(UInt32(36 + samples.count * 2))
    text("WAVEfmt ")
    u32(16)
    u16(1)
    u16(1)
    u32(rate)
    u32(rate * 2)
    u16(2)
    u16(16)
    text("data")
    u32(UInt32(samples.count * 2))
    for sample in samples { u16(UInt16(bitPattern: sample)) }
    return data
}

@Test func voiceAcceptsOnlyClosedBoundedPCM16Mono16000WAV() throws {
    #expect(try VoiceAudio.samples(voiceWAV()) == [0, 0.5, -0.5])
    #expect(throws: VoiceFailure.invalidAudio) {
        try VoiceAudio.samples(voiceWAV(rate: 44100))
    }
    #expect(throws: VoiceFailure.invalidAudio) {
        try VoiceAudio.samples(voiceWAV().dropLast())
    }
    #expect(throws: VoiceFailure.invalidAudio) {
        try VoiceAudio.samples(voiceWAV(samples: []))
    }
    #expect(throws: VoiceFailure.invalidAudio) {
        try VoiceAudio.samples(voiceWAV(samples: Array(repeating: 0, count: 960001)))
    }
}

@Test func voiceResultChannelIsClosedBoundedAndNeverTruncates() throws {
    #expect(try VoiceChannel.decode(VoiceChannel.encode(.text("Ответ 🌿"))) == .text("Ответ 🌿"))
    #expect(try VoiceChannel.decode(VoiceChannel.encode(.silence)) == .silence)
    #expect(try VoiceChannel.decode(VoiceChannel.encode(.failure(.models))) == .failure(.models))
    for raw in [
        #"{"status":"text","text":""}"#, #"{"status":"future"}"#,
        #"{"status":"silence","text":"hidden"}"#, #"{"status":"silence","extra":1}"#,
    ] {
        #expect(throws: VoiceFailure.invalidChannel) { try VoiceChannel.decode(Data(raw.utf8)) }
    }
    #expect(throws: VoiceFailure.invalidChannel) { try VoiceChannel.decode(Data([0xff])) }
    #expect(throws: VoiceFailure.invalidChannel) {
        try VoiceChannel.encode(.text(String(repeating: "я", count: 8193)))
    }
    #expect(throws: VoiceFailure.invalidChannel) {
        try VoiceChannel.decode(Data(repeating: 32, count: 32769))
    }
}

@Test func voiceWorkspaceCreatesPrivateUniqueOperationAndClosedLease() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
        "voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let lease = try await workspace.prepare()
    let canonicalRoot = await workspace.root
    #expect(
        lease.directory.deletingLastPathComponent().standardizedFileURL
            == canonicalRoot.standardizedFileURL)
    let attributes = try FileManager.default.attributesOfItem(atPath: lease.directory.path)
    #expect((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700)
    #expect(FileManager.default.fileExists(atPath: lease.audioURL.path))
    let request = lease.request(models: URL(fileURLWithPath: "/test-owned-models"))
    #expect(try VoiceChannel.decodeRequest(VoiceChannel.encodeRequest(request)) == request)
}

@Test func voiceModelsRejectMissingInventoryBeforeInference() {
    #expect(throws: VoiceFailure.models) {
        try VoiceModels.validate(
            URL(fileURLWithPath: "/private/tmp/ambient-voice-missing-" + UUID().uuidString))
    }
}

@Test func voiceCleanupFailureRetainsOperationUntilSuccessfulRetry() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let lease = try await workspace.prepare()
    try FileManager.default.setAttributes(
        [.posixPermissions: 0o500], ofItemAtPath: lease.directory.path)
    await #expect(throws: VoiceFailure.cleanup) { try await workspace.cleanup(lease) }
    #expect(FileManager.default.fileExists(atPath: lease.audioURL.path))
    try FileManager.default.setAttributes(
        [.posixPermissions: 0o700], ofItemAtPath: lease.directory.path)
    try await workspace.cleanup(lease)
    #expect(!FileManager.default.fileExists(atPath: lease.directory.path))
}

@Test func voiceIncompleteRootScanAndByteOverflowNeverAdmitNewRecording() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    try FileManager.default.createDirectory(
        at: root, withIntermediateDirectories: false,
        attributes: [.posixPermissions: 0o700])
    for _ in 0..<64 {
        let directory = root.appendingPathComponent("op-" + UUID().uuidString.lowercased())
        try FileManager.default.createDirectory(
            at: directory, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        #expect(
            FileManager.default.createFile(
                atPath: directory.appendingPathComponent("lease").path,
                contents: Data(), attributes: [.posixPermissions: 0o600]))
    }
    let workspace = VoiceWorkspace(root: root)
    await #expect(throws: VoiceFailure.limit) { try await workspace.prepare() }
    let remaining = try FileManager.default.contentsOfDirectory(atPath: root.path)
    #expect(!remaining.isEmpty && remaining.count < 64)
    let lease = try await workspace.prepare()
    let file = open(lease.audioURL.path, O_WRONLY | O_NOFOLLOW)
    try #require(file >= 0)
    #expect(ftruncate(file, 256 * 1024 * 1024) == 0)
    #expect(close(file) == 0)
    await #expect(throws: (any Error).self) { try await workspace.prepare() }
}

@Test func voiceNestedDirectoryLimitAlsoBoundsStartupCleanup() async throws {
    let root = URL(fileURLWithPath: "/private/tmp").appendingPathComponent(
        "ambient-voice-test-" + UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let workspace = VoiceWorkspace(root: root)
    let lease = try await workspace.prepare()
    #expect(flock(lease.leaseFD.raw, LOCK_UN) == 0)
    for index in 0..<64 {
        try FileManager.default.createDirectory(
            at: lease.directory.appendingPathComponent("nested-\(index)"),
            withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    }
    await #expect(throws: (any Error).self) { try await workspace.cleanupStale() }
    #expect(FileManager.default.fileExists(atPath: lease.audioURL.path))
}
