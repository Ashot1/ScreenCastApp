import CAmbient
import Foundation

struct LocalRootSettings: Equatable, Sendable, Identifiable {
    let path: String
    let revision: UInt64
    let exclusions: [String]
    var id: String { path }
}
struct LocalSourceOperation: Equatable, Sendable {
    let memory: UInt64
    let id: UInt64
    let expiresAt: Int64
    func cancel() { _ = ambient_source_v1_cancel(memory, id) }
    func release() throws { try LocalSourceBridge.check(ambient_source_v1_release(memory, id)) }
}
struct LocalBranch: Equatable, Sendable, Identifiable {
    let index: Int
    let fullRef: String
    let oid: String?
    let head: UInt32
    var id: Int { index }
}
struct LocalObservation: Equatable, Sendable {
    let started: Int64
    let ended: Int64
    let completeness: MemoryCompleteness
    let reasons: UInt32
    let branches: [LocalBranch]
}
struct LocalGitFile: Sendable {
    let path: String
    let bytes: [UInt8]
}
enum LocalSourceFailure: Error, Equatable {
    case root, deleted
    var key: String { self == .root ? "source.error.root" : "source.error.deleted" }
}
enum LocalSourceBridge {
    static func check(_ code: UInt32) throws {
        if code == 15 { throw LocalSourceFailure.deleted }
        if code == 14 { throw ModelCheckFailure.stale }
        try ModelCheckBridge.check(code)
    }
    static func withText<T>(_ text: String, _ body: (AmbientText) throws -> T) rethrows -> T {
        try Array(text.utf8).withUnsafeBufferPointer {
            try body(AmbientText(bytes: $0.baseAddress, length: UInt32($0.count)))
        }
    }
    static func decode(_ span: AmbientSpan, _ bytes: [UInt8], _ used: UInt32) throws -> String {
        try ModelCheckBridge.text(
            AmbientModelSpan(offset: span.offset, length: span.length), bytes, used: Int(used))
    }
    static func settings(memory: UInt64, path: String? = nil, index: UInt32 = 0) throws
        -> LocalRootSettings?
    {
        var out = AmbientSourceSettings()
        var spans = [AmbientSpan](repeating: AmbientSpan(), count: 16)
        var bytes = [UInt8](repeating: 0, count: 16384)
        let status = spans.withUnsafeMutableBufferPointer { ex in
            bytes.withUnsafeMutableBufferPointer { b in
                if let path {
                    return withText(path) {
                        ambient_source_v1_settings_get(
                            memory, $0, &out, ex.baseAddress, 16, b.baseAddress, 16384)
                    }
                }
                return ambient_source_v1_settings_at(
                    memory, index, &out, ex.baseAddress, 16, b.baseAddress, 16384)
            }
        }
        if status == 14 { return nil }
        try check(status)
        guard out.revision > 0, out.revision <= UInt64(Int64.max), out.exclusion_count <= 16,
            out.bytes_used <= 16384
        else { throw ModelCheckFailure.native }
        let actualPath = try decode(out.path, bytes, out.bytes_used)
        guard actualPath.hasPrefix("/"), !actualPath.utf8.contains(0),
            actualPath.utf8.count <= 4096,
            path == nil || path == actualPath
        else { throw ModelCheckFailure.native }
        let exclusions = try spans.prefix(Int(out.exclusion_count)).map {
            try decode($0, bytes, out.bytes_used)
        }
        return LocalRootSettings(path: actualPath, revision: out.revision, exclusions: exclusions)
    }
    static func save(memory: UInt64, path: String, expected: UInt64, exclusions: [String]) throws
        -> LocalRootSettings
    {
        guard exclusions.count <= 16 else { throw ModelCheckFailure.limit }
        let storage = exclusions.map { Array($0.utf8) }
        var inputs: [AmbientText] = []
        func append(_ index: Int) throws -> UInt64 {
            if index == storage.count {
                return try inputs.withUnsafeBufferPointer { ex in
                    try withText(path) { p in
                        var revision: UInt64 = 0
                        try check(
                            ambient_source_v1_settings_save(
                                memory, p, expected, ex.baseAddress, UInt32(ex.count), &revision))
                        return revision
                    }
                }
            }
            return try storage[index].withUnsafeBufferPointer {
                inputs.append(AmbientText(bytes: $0.baseAddress, length: UInt32($0.count)))
                defer { inputs.removeLast() }
                return try append(index + 1)
            }
        }
        let revision = try append(0)
        return LocalRootSettings(path: path, revision: revision, exclusions: exclusions)
    }
    static func observation(_ op: LocalSourceOperation) throws -> LocalObservation {
        var out = AmbientSourceObservation()
        var records = [AmbientSourceBranch](repeating: AmbientSourceBranch(), count: 256)
        var bytes = [UInt8](repeating: 0, count: 1_048_576)
        try records.withUnsafeMutableBufferPointer { r in
            try bytes.withUnsafeMutableBufferPointer { b in
                try check(
                    ambient_source_v1_observation(
                        op.memory, op.id, &out, r.baseAddress, 256, b.baseAddress, 1_048_576))
            }
        }
        guard let completeness = MemoryCompleteness(rawValue: out.completeness),
            out.started_at_utc <= out.ended_at_utc, out.branch_count <= 256,
            out.bytes_used <= 1_048_576, out.reasons <= 255
        else { throw ModelCheckFailure.native }
        let branches = try records.prefix(Int(out.branch_count)).enumerated().map { index, r in
            guard r.head <= 4, r.has_oid <= 1, r.has_oid == 1 || r.oid.length == 0 else {
                throw ModelCheckFailure.native
            }
            let ref = try decode(r.full_ref, bytes, out.bytes_used)
            let oid = r.has_oid == 1 ? try decode(r.oid, bytes, out.bytes_used) : nil
            guard ref.hasPrefix("refs/heads/"), ref.utf8.count <= 2043,
                oid.map({ [40, 64].contains($0.utf8.count) }) ?? true
            else { throw ModelCheckFailure.native }
            return LocalBranch(index: index, fullRef: ref, oid: oid, head: r.head)
        }
        return LocalObservation(
            started: out.started_at_utc, ended: out.ended_at_utc, completeness: completeness,
            reasons: out.reasons, branches: branches)
    }
    static func finish(
        _ op: LocalSourceOperation, files: [LocalGitFile], stats: AmbientSourceStats,
        start: Int64, end: Int64, reasons: UInt32
    ) throws {
        let paths = files.flatMap { Array($0.path.utf8) }
        let contents = files.flatMap(\.bytes)
        // Two flat buffers keep every borrowed span live without one native stack frame per file.
        try paths.withUnsafeBufferPointer { pathBuffer in
            try contents.withUnsafeBufferPointer { contentBuffer in
                var pathOffset = 0
                var contentOffset = 0
                let inputs = files.map { file in
                    let pathCount = file.path.utf8.count
                    defer {
                        pathOffset += pathCount
                        contentOffset += file.bytes.count
                    }
                    return AmbientSourceFile(
                        path: AmbientText(
                            bytes: pathBuffer.baseAddress?.advanced(by: pathOffset),
                            length: UInt32(pathCount)),
                        bytes: contentBuffer.baseAddress?.advanced(by: contentOffset),
                        length: UInt32(file.bytes.count))
                }
                try inputs.withUnsafeBufferPointer {
                    try check(
                        ambient_source_v1_finish(
                            op.memory, op.id, $0.baseAddress, UInt32($0.count), stats, start, end,
                            reasons))
                }
            }
        }
    }

    static func preview(_ op: LocalSourceOperation, index: Int) throws -> String {
        guard (0..<256).contains(index) else { throw ModelCheckFailure.invalid }
        var bytes = [UInt8](repeating: 0, count: 16384)
        var length: UInt32 = 0
        try bytes.withUnsafeMutableBufferPointer {
            try check(
                ambient_source_v1_preview(
                    op.memory, op.id, UInt32(index), $0.baseAddress, 16384, &length))
        }
        return try decode(AmbientSpan(offset: 0, length: length), bytes, length)
    }
}
extension MemorySession {
    func localRoot(path: String) throws -> LocalRootSettings? {
        try LocalSourceBridge.settings(memory: sourceMemoryHandle(), path: path)
    }
    func localRoots() throws -> [LocalRootSettings] {
        let handle = try sourceMemoryHandle()
        var roots: [LocalRootSettings] = []
        for index in 0..<64 {
            guard let r = try LocalSourceBridge.settings(memory: handle, index: UInt32(index))
            else {
                break
            }
            roots.append(r)
        }
        return roots
    }
    func saveLocalRoot(path: String, expected: UInt64, exclusions: [String]) throws
        -> LocalRootSettings
    {
        try LocalSourceBridge.save(
            memory: sourceMemoryHandle(), path: path, expected: expected, exclusions: exclusions)
    }
    func deleteLocalRoot(_ root: LocalRootSettings) throws {
        let handle = try sourceMemoryHandle()
        try LocalSourceBridge.withText(root.path) {
            try LocalSourceBridge.check(
                ambient_source_v1_settings_delete(handle, $0, root.revision))
        }
    }
    func beginLocalRead(_ task: MemoryTask, root: LocalRootSettings, expiresAt: Int64) throws
        -> LocalSourceOperation
    {
        let handle = try sourceMemoryHandle()
        var id: UInt64 = 0
        try LocalSourceBridge.withText(root.path) {
            try LocalSourceBridge.check(
                ambient_source_v1_begin(handle, task.id, task.revision, $0, root.revision, &id))
        }
        guard id > 0 else { throw ModelCheckFailure.native }
        return LocalSourceOperation(memory: handle, id: id, expiresAt: expiresAt)
    }
    func releaseLocalRead(_ op: LocalSourceOperation) throws {
        guard try sourceMemoryHandle() == op.memory else { throw ModelCheckFailure.closed }
        try op.release()
    }
    func localPreview(_ op: LocalSourceOperation, index: Int) throws -> String {
        guard try sourceMemoryHandle() == op.memory else { throw ModelCheckFailure.closed }
        return try LocalSourceBridge.preview(op, index: index)
    }
    func bindLocalBranch(_ op: LocalSourceOperation, index: Int) throws -> UInt64 {
        guard try sourceMemoryHandle() == op.memory, (0..<256).contains(index) else {
            throw ModelCheckFailure.invalid
        }
        var id: UInt64 = 0
        try LocalSourceBridge.check(
            ambient_source_v1_bind(
                op.memory, op.id, UInt32(index), Int64(Date().timeIntervalSince1970), &id))
        return id
    }
}
