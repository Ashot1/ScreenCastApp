import CAmbient
import Darwin
import Dispatch
import Foundation

/// Only this queue owns directory descriptors; cancellation never closes a descriptor under a syscall.
actor LocalGitReader {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.local-git")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    #if AMBIENT_MODEL_TEST_SUPPORT
        private var testAfterRead: (@Sendable (String) -> Void)?
        init(testAfterRead: @escaping @Sendable (String) -> Void) {
            self.testAfterRead = testAfterRead
        }
    #endif
    init() {}
    private var rootFD: Int32 = -1
    private var rootPath: String?
    private var scopedURL: URL?
    private enum ReadFailure: Error { case changed }
    private struct State {
        var files: [LocalGitFile] = []
        var stats = AmbientSourceStats()
        var reasons: UInt32 = 0
    }
    deinit {
        if rootFD >= 0 { Darwin.close(rootFD) }
        scopedURL?.stopAccessingSecurityScopedResource()
    }
    func select(_ url: URL) throws -> String {
        clear()
        let accessed = url.startAccessingSecurityScopedResource()
        do {
            let fd = try Self.openRoot(url.path)
            do {
                let path = try Self.path(fd)
                try Self.verifyRoot(fd, path)
                rootFD = fd
                rootPath = path
                if accessed { scopedURL = url }
                return path
            } catch {
                Darwin.close(fd)
                throw error
            }
        } catch {
            if accessed { url.stopAccessingSecurityScopedResource() }
            throw LocalSourceFailure.root
        }
    }
    func clear() {
        if rootFD >= 0 { Darwin.close(rootFD) }
        rootFD = -1
        rootPath = nil
        scopedURL?.stopAccessingSecurityScopedResource()
        scopedURL = nil
    }
    private static func openRoot(_ path: String) throws -> Int32 {
        guard path.hasPrefix("/"), !path.utf8.contains(0) else { throw LocalSourceFailure.root }
        var fd = Darwin.open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw LocalSourceFailure.root }
        for part in path.split(separator: "/") {
            guard part != ".", part != ".." else {
                Darwin.close(fd)
                throw LocalSourceFailure.root
            }
            let next = String(part).withCString {
                Darwin.openat(fd, $0, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            }
            Darwin.close(fd)
            guard next >= 0 else { throw LocalSourceFailure.root }
            fd = next
        }
        return fd
    }
    private static func path(_ fd: Int32) throws -> String {
        var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN))
        let status = buffer.withUnsafeMutableBufferPointer { b -> Int32 in
            guard let p = b.baseAddress else { return -1 }
            return fcntl(fd, F_GETPATH, p)
        }
        guard status == 0, let end = buffer.firstIndex(of: 0), end > 0,
            let path = String(bytes: buffer[..<end].map { UInt8(bitPattern: $0) }, encoding: .utf8),
            path.hasPrefix("/")
        else { throw LocalSourceFailure.root }
        return path
    }
    private static func metadata(_ fd: Int32) throws -> stat {
        var value = stat()
        guard fstat(fd, &value) == 0 else { throw ReadFailure.changed }
        return value
    }
    private static func same(_ a: stat, _ b: stat) -> Bool {
        a.st_dev == b.st_dev && a.st_ino == b.st_ino && a.st_size == b.st_size
            && a.st_mtimespec.tv_sec == b.st_mtimespec.tv_sec
            && a.st_mtimespec.tv_nsec == b.st_mtimespec.tv_nsec
            && a.st_ctimespec.tv_sec == b.st_ctimespec.tv_sec
            && a.st_ctimespec.tv_nsec == b.st_ctimespec.tv_nsec
            && a.st_mode == b.st_mode && a.st_nlink == b.st_nlink
    }
    private static func verifyRoot(_ fd: Int32, _ original: String) throws {
        guard try path(fd) == original else { throw ReadFailure.changed }
        // F_GETPATH can return a stale pathname after rmdir; reopening is an independent identity check.
        let reopened = try openRoot(original)
        defer { Darwin.close(reopened) }
        let held = try metadata(fd)
        let current = try metadata(reopened)
        guard held.st_dev == current.st_dev, held.st_ino == current.st_ino else {
            throw ReadFailure.changed
        }
    }
    private func permit(
        _ op: LocalSourceOperation, _ path: String, directory: Bool, state: inout State
    ) throws -> Bool {
        guard Int64(Date().timeIntervalSince1970) < op.expiresAt else {
            op.cancel()
            throw ModelCheckFailure.session
        }
        guard path.utf8.count <= 2048, path.split(separator: "/").count <= 16 else {
            state.reasons |= 2
            return false
        }
        var allowed: UInt32 = 0
        let status = LocalSourceBridge.withText(path) {
            ambient_source_v1_permit(op.memory, op.id, $0, directory ? 1 : 0, &allowed)
        }
        if status == 1, path.hasPrefix(".git/refs/heads/") {
            state.reasons |= 8
            return false
        }
        try LocalSourceBridge.check(status)
        guard allowed <= 1 else { throw ModelCheckFailure.native }
        if allowed == 0 {
            state.reasons |= 1
            return false
        }
        guard let rootPath else { throw LocalSourceFailure.root }
        do { try Self.verifyRoot(rootFD, rootPath) } catch { throw ReadFailure.changed }
        return true
    }
    private func directory(
        _ op: LocalSourceOperation, parent: Int32, name: String, path: String, state: inout State
    ) throws -> Int32? {
        guard try permit(op, path, directory: true, state: &state) else { return nil }
        guard state.stats.directories < 256 else {
            state.reasons |= 2
            return nil
        }
        state.stats.directories += 1
        let fd = name.withCString {
            Darwin.openat(parent, $0, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        }
        guard fd >= 0 else {
            if errno != ENOENT { state.reasons |= 4 }
            return nil
        }
        return fd
    }
    private func file(
        _ op: LocalSourceOperation, parent: Int32, name: String, path: String, cap: Int,
        required: Bool = false, state: inout State
    ) throws {
        guard try permit(op, path, directory: false, state: &state) else { return }
        guard state.stats.files < 256 else {
            state.reasons |= 2
            return
        }
        state.stats.files += 1
        let fd = name.withCString {
            Darwin.openat(parent, $0, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        }
        guard fd >= 0 else {
            if required || errno != ENOENT { state.reasons |= 4 }
            return
        }
        defer { Darwin.close(fd) }
        let before = try Self.metadata(fd)
        guard before.st_mode & S_IFMT == S_IFREG, before.st_nlink == 1 else {
            state.reasons |= 4
            return
        }
        guard before.st_size >= 0, before.st_size <= cap else {
            state.reasons |= 2
            return
        }
        var data: [UInt8] = []
        var buffer = [UInt8](repeating: 0, count: 8192)
        while true {
            guard try permit(op, path, directory: false, state: &state) else { return }
            let remaining = min(cap + 1 - data.count, 1_048_576 - Int(state.stats.bytes))
            guard remaining > 0 else {
                state.reasons |= 2
                return
            }
            let count = buffer.withUnsafeMutableBytes { b in
                Darwin.read(fd, b.baseAddress, min(b.count, remaining))
            }
            if count < 0 {
                if errno == EINTR { continue }
                state.reasons |= 4
                return
            }
            if count == 0 { break }
            state.stats.bytes += UInt32(count)
            #if AMBIENT_MODEL_TEST_SUPPORT
                testAfterRead?(path)
            #endif
            guard data.count + count <= cap else {
                state.reasons |= 2
                return
            }
            data.append(contentsOf: buffer.prefix(count))
        }
        guard try Self.same(before, Self.metadata(fd)) else { throw ReadFailure.changed }
        state.files.append(LocalGitFile(path: path, bytes: data))
    }
    private func heads(
        _ op: LocalSourceOperation, fd: Int32, path: String, state: inout State
    ) throws {
        let before = try Self.metadata(fd)
        let copied = fcntl(fd, F_DUPFD_CLOEXEC, 0)
        guard copied >= 0 else {
            state.reasons |= 4
            return
        }
        guard let stream = fdopendir(copied) else {
            Darwin.close(copied)
            state.reasons |= 4
            return
        }
        defer { closedir(stream) }
        while true {
            guard try permit(op, path, directory: true, state: &state) else { return }
            guard state.stats.entries < 4096 else {
                state.reasons |= 2
                return
            }
            errno = 0
            guard let entry = readdir(stream) else {
                if errno != 0 { state.reasons |= 4 }
                break
            }
            state.stats.entries += 1
            let raw = entry.pointee
            let name: String? = withUnsafeBytes(of: raw.d_name) {
                String(bytes: $0.prefix(Int(raw.d_namlen)), encoding: .utf8)
            }
            guard let name else {
                state.reasons |= 8
                continue
            }
            if name == "." || name == ".." { continue }
            let childPath = path + "/" + name
            guard try permit(op, childPath, directory: false, state: &state) else { continue }
            var metadata = stat()
            let status = name.withCString { fstatat(fd, $0, &metadata, AT_SYMLINK_NOFOLLOW) }
            guard status == 0 else {
                state.reasons |= 4
                continue
            }
            if metadata.st_mode & S_IFMT == S_IFDIR {
                if let child = try directory(
                    op, parent: fd, name: name, path: childPath, state: &state)
                {
                    do { try heads(op, fd: child, path: childPath, state: &state) } catch {
                        Darwin.close(child)
                        throw error
                    }
                    Darwin.close(child)
                }
            } else {
                try file(
                    op, parent: fd, name: name, path: childPath, cap: 4096, required: true,
                    state: &state)
            }
        }
        guard try Self.same(before, Self.metadata(fd)) else { throw ReadFailure.changed }
    }
    private func collect(_ op: LocalSourceOperation) throws -> State {
        var state = State()
        guard rootFD >= 0, let rootPath else { throw LocalSourceFailure.root }
        guard try permit(op, "", directory: true, state: &state) else { return state }
        let root = fcntl(rootFD, F_DUPFD_CLOEXEC, 0)
        guard root >= 0 else { throw LocalSourceFailure.root }
        defer { Darwin.close(root) }
        let rootBefore = try Self.metadata(root)
        state.stats.directories = 1
        do {
            guard try permit(op, ".git", directory: true, state: &state) else { return state }
            let git = Darwin.openat(root, ".git", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard git >= 0 else {
                state.reasons |= 32
                return state
            }
            defer { Darwin.close(git) }
            state.stats.directories += 1
            let gitBefore = try Self.metadata(git)
            guard try permit(op, ".git/commondir", directory: false, state: &state) else {
                return state
            }
            var indirect = stat()
            if fstatat(git, "commondir", &indirect, AT_SYMLINK_NOFOLLOW) == 0 {
                state.reasons |= 32
                return state
            }
            guard errno == ENOENT else {
                state.reasons |= 4
                return state
            }
            try file(
                op, parent: git, name: "config", path: ".git/config", cap: 65536, required: true,
                state: &state)
            try file(
                op, parent: git, name: "HEAD", path: ".git/HEAD", cap: 4096, required: true,
                state: &state)
            try file(
                op, parent: git, name: "packed-refs", path: ".git/packed-refs", cap: 524288,
                state: &state)
            if let refs = try directory(
                op, parent: git, name: "refs", path: ".git/refs", state: &state)
            {
                defer { Darwin.close(refs) }
                let before = try Self.metadata(refs)
                if let fd = try directory(
                    op, parent: refs, name: "heads", path: ".git/refs/heads", state: &state)
                {
                    defer { Darwin.close(fd) }
                    try heads(op, fd: fd, path: ".git/refs/heads", state: &state)
                }
                guard try Self.same(before, Self.metadata(refs)) else { throw ReadFailure.changed }
            }
            try Self.verifyRoot(root, rootPath)
            guard try Self.same(rootBefore, Self.metadata(root)),
                try Self.same(gitBefore, Self.metadata(git))
            else { throw ReadFailure.changed }
        } catch ReadFailure.changed {
            state.files = []
            state.reasons |= 64
        }
        return state
    }
    func read(_ op: LocalSourceOperation) throws -> LocalObservation {
        let start = Int64(Date().timeIntervalSince1970)
        do {
            let state = try collect(op)
            // collect has returned and closed all operation FDs before Core releases the read lifetime.
            try LocalSourceBridge.finish(
                op, files: state.files, stats: state.stats, start: start,
                end: Int64(Date().timeIntervalSince1970), reasons: state.reasons)
            return try LocalSourceBridge.observation(op)
        } catch {
            let status = ambient_source_v1_abort(op.memory, op.id)
            if status != 0 { throw ModelCheckFailure.closed }
            throw error
        }
    }
}
