import CAmbient
import Darwin
import Dispatch
import Foundation

/// This queue owns operation descriptors; cancellation never closes an FD under an accepted syscall.
actor CodexReader {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.codex-reader")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    private var selectedURL: URL?
    #if AMBIENT_MODEL_TEST_SUPPORT
        private var testAfterRead: (@Sendable (Int32, String, Int) -> Void)?
        private var testAfterClose: (@Sendable (Int32) -> Void)?
        init(
            testAfterRead: @escaping @Sendable (Int32, String, Int) -> Void,
            testAfterClose: (@Sendable (Int32) -> Void)? = nil
        ) {
            self.testAfterRead = testAfterRead
            self.testAfterClose = testAfterClose
        }
    #endif
    init() {}
    func clear() { selectedURL = nil }
    func select(_ url: URL) throws -> String {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let fd = try Self.openFile(url.path)
        defer { Darwin.close(fd) }
        let path = try Self.path(fd)
        try Self.verify(fd, path: path, minimumSize: 0)
        selectedURL = url
        return path
    }
    private static func openFile(_ path: String) throws -> Int32 {
        guard path.hasPrefix("/"), path.utf8.count <= 4096, !path.utf8.contains(0) else {
            throw CodexSourceFailure.changed
        }
        let parts = path.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count > 1,
            parts.dropFirst().allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." })
        else { throw CodexSourceFailure.changed }
        var fd = Darwin.open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw CodexSourceFailure.changed }
        for (index, part) in parts.dropFirst().enumerated() {
            let last = index == parts.count - 2
            let next = String(part).withCString {
                Darwin.openat(
                    fd, $0,
                    O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC | (last ? 0 : O_DIRECTORY))
            }
            Darwin.close(fd)
            guard next >= 0 else { throw CodexSourceFailure.changed }
            fd = next
        }
        do {
            _ = try metadata(fd)
            return fd
        } catch {
            Darwin.close(fd)
            throw error
        }
    }
    private static func metadata(_ fd: Int32) throws -> stat {
        var s = stat()
        guard fstat(fd, &s) == 0, s.st_mode & S_IFMT == S_IFREG, s.st_size >= 0 else {
            throw CodexSourceFailure.changed
        }
        return s
    }
    private static func path(_ fd: Int32) throws -> String {
        var bytes = [CChar](repeating: 0, count: Int(MAXPATHLEN))
        let status = bytes.withUnsafeMutableBufferPointer { b -> Int32 in
            guard let p = b.baseAddress else { return -1 }
            return fcntl(fd, F_GETPATH, p)
        }
        guard status == 0, let end = bytes.firstIndex(of: 0), end > 0,
            let path = String(bytes: bytes[..<end].map { UInt8(bitPattern: $0) }, encoding: .utf8),
            path.hasPrefix("/")
        else { throw CodexSourceFailure.changed }
        return path
    }
    private static func verify(_ fd: Int32, path: String, minimumSize: UInt64) throws {
        guard try Self.path(fd) == path else { throw CodexSourceFailure.changed }
        let current = try openFile(path)
        defer { Darwin.close(current) }
        let held = try metadata(fd)
        let actual = try metadata(current)
        guard held.st_dev == actual.st_dev, held.st_ino == actual.st_ino,
            UInt64(held.st_size) >= minimumSize, UInt64(actual.st_size) >= minimumSize
        else { throw CodexSourceFailure.changed }
    }
    private func permit(_ op: CodexOperation, fd: Int32, minimumSize: UInt64) throws {
        guard Int64(Date().timeIntervalSince1970) < op.expiresAt else {
            op.cancel()
            throw ModelCheckFailure.session
        }
        try CodexBridge.check(ambient_codex_v1_permit(op.memory, op.id))
        try Self.verify(fd, path: op.path, minimumSize: minimumSize)
    }
    private func portion(
        _ op: CodexOperation, fd: Int32, offset: UInt64, count: Int, minimumSize: UInt64,
        stage: String
    ) throws -> [UInt8] {
        try permit(op, fd: fd, minimumSize: minimumSize)
        guard count > 0, count <= 8192, offset <= UInt64(Int64.max) else {
            throw ModelCheckFailure.limit
        }
        var bytes = [UInt8](repeating: 0, count: count)
        while true {
            let n = bytes.withUnsafeMutableBytes {
                Darwin.pread(fd, $0.baseAddress, count, off_t(offset))
            }
            if n < 0 {
                if errno == EINTR {
                    try permit(op, fd: fd, minimumSize: minimumSize)
                    continue
                }
                throw CodexSourceFailure.changed
            }
            #if AMBIENT_MODEL_TEST_SUPPORT
                testAfterRead?(fd, stage, n)
            #endif
            return Array(bytes.prefix(n))
        }
    }
    private func header(_ op: CodexOperation, fd: Int32, minimumSize: UInt64, stage: String) throws
        -> [UInt8]
    {
        var bytes: [UInt8] = []
        while bytes.count < 262145 {
            let part = try portion(
                op, fd: fd, offset: UInt64(bytes.count), count: min(8192, 262145 - bytes.count),
                minimumSize: minimumSize, stage: stage)
            guard !part.isEmpty else { throw CodexSourceFailure.format }
            if let end = part.firstIndex(of: 10) {
                bytes.append(contentsOf: part[...end])
                return bytes
            }
            bytes.append(contentsOf: part)
        }
        throw ModelCheckFailure.limit
    }
    private func readBytes(_ op: CodexOperation) throws -> [UInt8] {
        try CodexBridge.check(ambient_codex_v1_permit(op.memory, op.id))
        let url = selectedURL ?? URL(fileURLWithPath: op.path)
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let fd = try Self.openFile(op.path)
        defer {
            #if AMBIENT_MODEL_TEST_SUPPORT
                let status = Darwin.close(fd)
                testAfterClose?(status)
            #else
                Darwin.close(fd)
            #endif
        }
        let initial = try Self.metadata(fd)
        let size = UInt64(initial.st_size)
        let first = try header(op, fd: fd, minimumSize: size, stage: "header")
        var plan = AmbientCodexPlan()
        try first.withUnsafeBufferPointer {
            try CodexBridge.check(
                ambient_codex_v1_plan(
                    op.memory, op.id,
                    AmbientCodexFile(
                        device: UInt64(UInt32(bitPattern: initial.st_dev)), inode: initial.st_ino,
                        size: size), $0.baseAddress, UInt32($0.count), &plan))
        }
        guard plan.end <= size, plan.start <= plan.end, plan.end - plan.start <= 1_048_576 else {
            throw ModelCheckFailure.native
        }
        let start = plan.start > 0 ? plan.start - 1 : 0
        var bytes: [UInt8] = []
        let wanted = Int(plan.end - start)
        while bytes.count < wanted {
            let part = try portion(
                op, fd: fd, offset: start + UInt64(bytes.count),
                count: min(8192, wanted - bytes.count), minimumSize: size, stage: "window")
            guard !part.isEmpty else { throw CodexSourceFailure.changed }
            bytes.append(contentsOf: part)
        }
        guard try header(op, fd: fd, minimumSize: size, stage: "header-check") == first else {
            throw CodexSourceFailure.changed
        }
        try permit(op, fd: fd, minimumSize: size)
        return bytes
    }
    func read(_ op: CodexOperation) throws -> CodexObservation {
        do {
            let bytes = try readBytes(op)
            // readBytes returned through its FD/security-scope defers before Core may release this job.
            try bytes.withUnsafeBufferPointer {
                try CodexBridge.check(
                    ambient_codex_v1_finish(
                        op.memory, op.id, $0.baseAddress, UInt32($0.count),
                        Int64(Date().timeIntervalSince1970)))
            }
            return try CodexBridge.observation(op)
        } catch {
            try CodexBridge.check(ambient_codex_v1_abort(op.memory, op.id))
            throw error
        }
    }
}
