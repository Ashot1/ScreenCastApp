import CryptoKit
import Darwin
import Foundation

public enum VoiceModels {
    private struct Inventory: Decodable {
        let revision: String
        let files: [Entry]
    }
    private struct Entry: Decodable {
        let file: String
        let bytes: Int
        let sha256: String
    }
    public static func validate(_ directory: URL) throws {
        do {
            guard
                let url = Bundle.module.url(
                    forResource: "parakeet-v3-inventory", withExtension: "json")
            else {
                throw VoiceFailure.models
            }
            let inventory = try JSONDecoder().decode(Inventory.self, from: Data(contentsOf: url))
            guard inventory.revision == "7dd20fe6b1797d35f5e3307e8b1732d9a178edfe",
                inventory.files.count == 21,
                inventory.files.reduce(0, { $0 + $1.bytes }) == 483_105_645
            else {
                throw VoiceFailure.models
            }
            let root = try VoiceFS.directory(directory.path)
            for entry in inventory.files {
                let parts = entry.file.split(separator: "/")
                guard let name = parts.last, !parts.contains(".."), entry.bytes > 0 else {
                    throw VoiceFailure.models
                }
                var parent = root
                for part in parts.dropLast() {
                    parent = try VoiceFD(openat(parent.raw, String(part), VoiceFS.directoryFlags))
                }
                let file = try VoiceFD(openat(parent.raw, String(name), VoiceFS.fileFlags))
                let before = try VoiceFS.info(file.raw)
                guard before.st_mode & S_IFMT == S_IFREG, before.st_nlink == 1,
                    before.st_size == entry.bytes
                else { throw VoiceFailure.models }
                var hash = SHA256()
                var buffer = [UInt8](repeating: 0, count: 1024 * 1024)
                var readBytes = 0
                while true {
                    let count = Darwin.read(
                        file.raw, &buffer, min(buffer.count, entry.bytes + 1 - readBytes))
                    if count < 0 && errno == EINTR { continue }
                    guard count >= 0 else { throw VoiceFailure.models }
                    if count == 0 { break }
                    readBytes += count
                    guard readBytes <= entry.bytes else { throw VoiceFailure.models }
                    hash.update(data: buffer.prefix(count))
                }
                let after = try VoiceFS.info(file.raw)
                let digest = hash.finalize().map { String(format: "%02x", $0) }.joined()
                guard readBytes == entry.bytes, digest == entry.sha256,
                    before.st_size == after.st_size, before.st_nlink == after.st_nlink,
                    before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
                    before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
                    before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
                    before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec
                else {
                    throw VoiceFailure.models
                }
            }
        } catch { throw VoiceFailure.models }
    }
}
