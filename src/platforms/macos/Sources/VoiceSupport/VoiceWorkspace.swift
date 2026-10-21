import Darwin
import Foundation

final class VoiceFD: Sendable {
    let raw: Int32
    init(_ raw: Int32) throws {
        guard raw >= 0 else { throw VoiceFailure.storage }
        self.raw = raw
    }
    deinit { Darwin.close(raw) }
}

enum VoiceFS {
    static let directoryFlags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
    static let fileFlags = O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC
    static func info(_ fd: Int32) throws -> stat {
        var value = stat()
        guard fstat(fd, &value) == 0 else { throw VoiceFailure.storage }
        return value
    }
    static func identity(_ value: stat) -> VoiceIdentity {
        VoiceIdentity(device: value.st_dev, inode: value.st_ino, owner: value.st_uid)
    }
    static func directory(_ path: String) throws -> VoiceFD {
        guard path.hasPrefix("/"), !path.utf8.contains(0), path.utf8.count <= 4096 else {
            throw VoiceFailure.storage
        }
        var fd = try VoiceFD(Darwin.open("/", directoryFlags))
        for part in path.split(separator: "/") {
            guard part != "..", part != "." else { throw VoiceFailure.storage }
            fd = try VoiceFD(openat(fd.raw, String(part), directoryFlags))
        }
        return fd
    }
    static func privateDirectory(_ fd: Int32) throws {
        let value = try info(fd)
        guard value.st_mode & S_IFMT == S_IFDIR, value.st_uid == getuid(),
            value.st_mode & 0o777 == 0o700
        else { throw VoiceFailure.storage }
    }
    static func regular(_ fd: Int32, maximum: Int) throws -> stat {
        let value = try info(fd)
        guard value.st_mode & S_IFMT == S_IFREG, value.st_uid == getuid(), value.st_nlink == 1,
            value.st_mode & 0o077 == 0, value.st_size >= 0, value.st_size <= maximum
        else {
            throw VoiceFailure.storage
        }
        return value
    }
    static func readFile(_ parent: Int32, name: String, maximum: Int) throws -> Data {
        let file = try VoiceFD(openat(parent, name, fileFlags))
        let before = try regular(file.raw, maximum: maximum)
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: min(maximum + 1, 65536))
        while true {
            let count = Darwin.read(file.raw, &buffer, min(buffer.count, maximum + 1 - data.count))
            if count < 0 && errno == EINTR { continue }
            guard count >= 0 else { throw VoiceFailure.storage }
            if count == 0 { break }
            data.append(contentsOf: buffer.prefix(count))
            guard data.count <= maximum else { throw VoiceFailure.storage }
        }
        let after = try regular(file.raw, maximum: maximum)
        guard before.st_size == data.count, before.st_size == after.st_size,
            before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
            before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
            before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
            before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec
        else { throw VoiceFailure.storage }
        return data
    }
    static func entries(_ fd: Int32, maximum: Int) throws -> [String] {
        let raw = openat(fd, ".", directoryFlags)
        guard raw >= 0 else { throw VoiceFailure.storage }
        guard let stream = fdopendir(raw) else {
            Darwin.close(raw)
            throw VoiceFailure.storage
        }
        defer { closedir(stream) }
        var names: [String] = []
        errno = 0
        while let entry = readdir(stream) {
            let name = withUnsafePointer(to: entry.pointee.d_name) {
                $0.withMemoryRebound(to: CChar.self, capacity: 1024) {
                    String(validatingCString: $0)
                }
            }
            guard let name else { throw VoiceFailure.storage }
            if name == "." || name == ".." { continue }
            guard names.count < maximum else { throw VoiceFailure.limit }
            names.append(name)
            errno = 0
        }
        guard errno == 0 else { throw VoiceFailure.storage }
        return names
    }
    static func inspectTree(
        _ fd: Int32, depth: Int = 0, remaining: inout Int,
        directories: inout Int
    ) throws {
        guard depth <= 8, directories > 0 else { throw VoiceFailure.limit }
        directories -= 1
        try privateDirectory(fd)
        for name in try entries(fd, maximum: 128) {
            var value = stat()
            guard fstatat(fd, name, &value, AT_SYMLINK_NOFOLLOW) == 0, value.st_uid == getuid()
            else {
                throw VoiceFailure.storage
            }
            if value.st_mode & S_IFMT == S_IFDIR {
                let child = try VoiceFD(openat(fd, name, directoryFlags))
                try inspectTree(
                    child.raw, depth: depth + 1, remaining: &remaining, directories: &directories)
            } else {
                guard value.st_mode & S_IFMT == S_IFREG, value.st_nlink == 1,
                    value.st_mode & 0o077 == 0, value.st_size >= 0, value.st_size <= remaining
                else {
                    throw VoiceFailure.storage
                }
                remaining -= Int(value.st_size)
            }
        }
    }
    static func removeTree(_ fd: Int32, depth: Int = 0, directories: inout Int) throws {
        guard depth <= 8, directories > 0 else { throw VoiceFailure.cleanup }
        directories -= 1
        for name in try entries(fd, maximum: 128) {
            // Keep the exclusively locked inode linked until all other files are gone.
            if depth == 0 && name == "lease" { continue }
            var value = stat()
            guard fstatat(fd, name, &value, AT_SYMLINK_NOFOLLOW) == 0, value.st_uid == getuid()
            else {
                throw VoiceFailure.cleanup
            }
            if value.st_mode & S_IFMT == S_IFDIR {
                let child = try VoiceFD(openat(fd, name, directoryFlags))
                try privateDirectory(child.raw)
                try removeTree(child.raw, depth: depth + 1, directories: &directories)
                guard unlinkat(fd, name, AT_REMOVEDIR) == 0 else { throw VoiceFailure.cleanup }
            } else {
                guard value.st_mode & S_IFMT == S_IFREG, value.st_nlink == 1,
                    unlinkat(fd, name, 0) == 0
                else { throw VoiceFailure.cleanup }
            }
        }
    }
}

public actor VoiceWorkspace {
    public let root: URL
    private static let maximumBytes = 256 * 1024 * 1024
    public init(root: URL) {
        // Canonicalize only the existing OS-owned parent; never follow a voice root symlink.
        let parent = root.deletingLastPathComponent().path
        let canonical = parent.withCString { realpath($0, nil) }
        if let canonical {
            self.root = URL(fileURLWithPath: String(cString: canonical), isDirectory: true)
                .appendingPathComponent(root.lastPathComponent, isDirectory: true)
            free(canonical)
        } else {
            self.root = root
        }
    }
    private func cleanedRoot() throws -> VoiceFD {
        let parent = try VoiceFS.directory(root.deletingLastPathComponent().path)
        if mkdirat(parent.raw, root.lastPathComponent, 0o700) != 0 && errno != EEXIST {
            throw VoiceFailure.storage
        }
        let rootFD = try VoiceFS.directory(root.path)
        try VoiceFS.privateDirectory(rootFD.raw)
        // Reserve capacity for one new bounded WAV and result before admitting a recording.
        var remaining =
            Self.maximumBytes - VoiceAudio.maximumBytes - VoiceChannel.maximumResultBytes
        let names = try VoiceFS.entries(rootFD.raw, maximum: 64)
        // Share the allowance across subtrees: empty directories cannot bypass the byte limit.
        var directories = 63
        var retained = 0
        for name in names {
            guard name.hasPrefix("op-"), UUID(uuidString: String(name.dropFirst(3))) != nil else {
                throw VoiceFailure.cleanup
            }
            let lease = try VoiceLease(existing: root.appendingPathComponent(name))
            try VoiceFS.inspectTree(
                lease.directoryFD.raw, remaining: &remaining, directories: &directories)
            if flock(lease.leaseFD.raw, LOCK_EX | LOCK_NB) == 0 {
                try lease.removeLocked()
            } else if errno == EWOULDBLOCK {
                retained += 1
            } else {
                throw VoiceFailure.cleanup
            }
        }
        guard names.count < 64 else { throw VoiceFailure.limit }
        guard retained == 0 else { throw VoiceFailure.cleanup }
        return rootFD
    }
    public func cleanupStale() throws {
        _ = try cleanedRoot()
    }
    public func prepare() throws -> VoiceLease {
        let rootFD = try cleanedRoot()
        let name = "op-" + UUID().uuidString.lowercased()
        guard mkdirat(rootFD.raw, name, 0o700) == 0 else { throw VoiceFailure.storage }
        let directory = root.appendingPathComponent(name, isDirectory: true)
        let fd = try VoiceFS.directory(directory.path)
        let leaseFD = try VoiceFD(
            openat(fd.raw, "lease", O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600))
        guard flock(leaseFD.raw, LOCK_SH | LOCK_NB) == 0 else { throw VoiceFailure.storage }
        let audio = try VoiceFD(
            openat(fd.raw, "audio.wav", O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        )
        _ = try VoiceFS.regular(audio.raw, maximum: VoiceAudio.maximumBytes)
        return try VoiceLease(directory: directory, directoryFD: fd, leaseFD: leaseFD)
    }
    public func cleanup(_ lease: VoiceLease) throws {
        guard
            lease.directory.deletingLastPathComponent().standardizedFileURL
                == root.standardizedFileURL
        else {
            throw VoiceFailure.cleanup
        }
        do {
            guard flock(lease.leaseFD.raw, LOCK_EX | LOCK_NB) == 0 else {
                throw VoiceFailure.cleanup
            }
            try lease.removeLocked()
        } catch { throw VoiceFailure.cleanup }
    }
    public func audioSize(_ lease: VoiceLease) throws -> Int {
        let file = try VoiceFD(openat(lease.directoryFD.raw, "audio.wav", VoiceFS.fileFlags))
        return Int(try VoiceFS.regular(file.raw, maximum: VoiceAudio.maximumBytes + 1).st_size)
    }
}

/// Immutable descriptors remain owned through actual child exit and cleanup.
public final class VoiceLease: Sendable {
    public let directory: URL
    public let directoryIdentity: VoiceIdentity
    public let leaseIdentity: VoiceIdentity
    let directoryFD: VoiceFD
    let leaseFD: VoiceFD
    public var audioURL: URL { directory.appendingPathComponent("audio.wav") }
    init(directory: URL, directoryFD: VoiceFD, leaseFD: VoiceFD) throws {
        self.directory = directory
        self.directoryFD = directoryFD
        self.leaseFD = leaseFD
        directoryIdentity = VoiceFS.identity(try VoiceFS.info(directoryFD.raw))
        leaseIdentity = VoiceFS.identity(try VoiceFS.regular(leaseFD.raw, maximum: 0))
        try verifyPaths()
    }
    convenience init(existing directory: URL) throws {
        let fd = try VoiceFS.directory(directory.path)
        try VoiceFS.privateDirectory(fd.raw)
        let lease = try VoiceFD(
            openat(fd.raw, "lease", O_RDWR | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC))
        _ = try VoiceFS.regular(lease.raw, maximum: 0)
        try self.init(directory: directory, directoryFD: fd, leaseFD: lease)
    }
    public static func open(_ request: VoiceRequest) throws -> VoiceLease {
        try VoiceLeaseCandidate(request).acquire()
    }
    func verifyPaths() throws {
        try VoiceFS.privateDirectory(directoryFD.raw)
        let original = try VoiceFS.directory(directory.path)
        let linked = try VoiceFD(openat(original.raw, "lease", VoiceFS.fileFlags))
        guard VoiceFS.identity(try VoiceFS.info(original.raw)) == directoryIdentity,
            VoiceFS.identity(try VoiceFS.regular(linked.raw, maximum: 0)) == leaseIdentity,
            VoiceFS.identity(try VoiceFS.regular(leaseFD.raw, maximum: 0)) == leaseIdentity
        else {
            throw VoiceFailure.storage
        }
    }
    public func request(models: URL) -> VoiceRequest {
        VoiceRequest(
            directory: directory.path, directoryIdentity: directoryIdentity,
            leaseIdentity: leaseIdentity, modelDirectory: models.path)
    }
    public func audioSamples() throws -> [Float] {
        try verifyPaths()
        return try VoiceAudio.samples(
            VoiceFS.readFile(directoryFD.raw, name: "audio.wav", maximum: VoiceAudio.maximumBytes))
    }
    public func readResult() throws -> VoiceResult {
        try verifyPaths()
        return try VoiceChannel.decode(
            VoiceFS.readFile(
                directoryFD.raw, name: "result.json", maximum: VoiceChannel.maximumResultBytes))
    }
    public func writeResult(_ result: VoiceResult) throws {
        try verifyPaths()
        let data = try VoiceChannel.encode(result)
        let fd = try VoiceFD(
            openat(
                directoryFD.raw, "result.json",
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600))
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(
                    fd.raw, bytes.baseAddress?.advanced(by: offset), bytes.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw VoiceFailure.storage }
                offset += count
            }
        }
    }
    func removeLocked() throws {
        try verifyPaths()
        var remaining = 256 * 1024 * 1024
        var directories = 64
        try VoiceFS.inspectTree(directoryFD.raw, remaining: &remaining, directories: &directories)
        directories = 64
        try VoiceFS.removeTree(directoryFD.raw, directories: &directories)
        try verifyPaths()
        let parent = try VoiceFS.directory(directory.deletingLastPathComponent().path)
        guard unlinkat(directoryFD.raw, "lease", 0) == 0,
            unlinkat(parent.raw, directory.lastPathComponent, AT_REMOVEDIR) == 0
        else {
            throw VoiceFailure.cleanup
        }
    }
}

/// Before the independent lock is acquired, no audio/result operations are available.
public struct VoiceLeaseCandidate: ~Copyable, Sendable {
    private let request: VoiceRequest
    private let directory: URL
    private let directoryFD: VoiceFD
    private let leaseFD: VoiceFD
    public init(_ request: VoiceRequest) throws {
        let directory = URL(fileURLWithPath: request.directory, isDirectory: true)
        guard directory.lastPathComponent.hasPrefix("op-"),
            UUID(uuidString: String(directory.lastPathComponent.dropFirst(3))) != nil
        else {
            throw VoiceFailure.storage
        }
        let directoryFD = try VoiceFS.directory(directory.path)
        try VoiceFS.privateDirectory(directoryFD.raw)
        let leaseFD = try VoiceFD(
            openat(directoryFD.raw, "lease", O_RDWR | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC))
        guard VoiceFS.identity(try VoiceFS.info(directoryFD.raw)) == request.directoryIdentity,
            VoiceFS.identity(try VoiceFS.regular(leaseFD.raw, maximum: 0)) == request.leaseIdentity
        else {
            throw VoiceFailure.storage
        }
        self.request = request
        self.directory = directory
        self.directoryFD = directoryFD
        self.leaseFD = leaseFD
    }
    public consuming func acquire() throws -> VoiceLease {
        // Independent opens, rather than inherited descriptors, have independent flock ownership.
        guard flock(leaseFD.raw, LOCK_SH) == 0 else { throw VoiceFailure.storage }
        let lease = try VoiceLease(directory: directory, directoryFD: directoryFD, leaseFD: leaseFD)
        guard lease.directoryIdentity == request.directoryIdentity,
            lease.leaseIdentity == request.leaseIdentity
        else {
            throw VoiceFailure.storage
        }
        return lease
    }
}
