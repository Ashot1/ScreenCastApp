import CVoiceProcess
import Darwin
import Foundation
import VoiceSupport

@main enum VoiceTestChild {
    static func main() {
        var bytes = [UInt8](repeating: 0, count: VoiceChannel.maximumRequestBytes)
        let count = ambient_voice_read_request(&bytes, bytes.count)
        guard count > 0, ambient_voice_watch_parent() == 0 else { _exit(65) }
        do {
            let request = try VoiceChannel.decodeRequest(Data(bytes.prefix(Int(count))))
            if request.modelDirectory.hasSuffix("/race-success")
                || request.modelDirectory.hasSuffix("/next-success")
            {
                let lease = try VoiceLease.open(request)
                signal(SIGTERM, SIG_IGN)
                try marker(request.directory + "/ready")
                usleep(request.modelDirectory.hasSuffix("/next-success") ? 50000 : 1000)
                try lease.writeResult(.text("public test"))
            } else if request.modelDirectory.hasSuffix("/lease-race") {
                let candidate = try VoiceLeaseCandidate(request)
                try marker(request.directory + "/ready")
                let lease = try candidate.acquire()
                _ = try lease.audioSamples()
                try marker(request.directory + "/audio-opened")
            } else {
                let lease = try VoiceLease.open(request)
                signal(SIGTERM, SIG_IGN)
                try marker(request.directory + "/ready")
                withExtendedLifetime(lease) { while true { sleep(1000) } }
            }
        } catch { _exit(65) }
    }
    private static func marker(_ path: String) throws {
        guard
            FileManager.default.createFile(
                atPath: path, contents: Data(), attributes: [.posixPermissions: 0o600])
        else {
            throw VoiceFailure.storage
        }
    }
}
