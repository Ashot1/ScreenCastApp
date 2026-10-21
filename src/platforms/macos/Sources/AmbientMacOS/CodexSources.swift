import CAmbient
import Foundation

struct CodexSelection: Equatable, Sendable, Identifiable {
    let id: UInt64
    let revision: UInt64
    let path: String
}
struct CodexOperation: Equatable, Sendable {
    let memory: UInt64
    let id: UInt64
    let path: String
    let expiresAt: Int64
    func cancel() { _ = ambient_codex_v1_cancel(memory, id) }
    func release() throws { try CodexBridge.check(ambient_codex_v1_release(memory, id)) }
}
struct CodexEvent: Equatable, Sendable, Identifiable {
    let id: Int
    let ordinal: UInt64
    let kind: UInt32
    let text: String
    let turnID: String
    let itemID: String?
    let observedAt: Int64?
    let partial: Bool
    var kindKey: String { Self.kindKey(kind) }
    static func kindKey(_ kind: UInt32) -> String {
        let keys = [
            "codex.user", "codex.agent", "codex.plan", "codex.delegatedCreate",
            "codex.delegatedSend", "codex.started", "codex.completed", "codex.failed",
            "codex.aborted",
        ]
        return (1...9).contains(kind) ? keys[Int(kind - 1)] : "codex.reason.unsupported"
    }
}
struct CodexObservation: Equatable, Sendable {
    let threadID: String
    let observedAt: Int64
    let cursor: UInt64
    let nextBefore: UInt64
    let reasons: UInt32
    let physicalLines: UInt32
    let events: [CodexEvent]
}
struct CodexSavedMetadata: Equatable, Sendable, Identifiable {
    let id: UInt64
    let evidenceID: UInt64
    let selectionID: UInt64
    let revision: UInt64
    let ordinal: UInt64
    let kind: UInt32
    let turnID: String
    let itemID: String?
}
enum CodexSourceFailure: Error, Equatable {
    case format, changed, conflict
    var key: String {
        switch self {
        case .format: "codex.error.format"
        case .changed: "codex.error.changed"
        case .conflict: "codex.error.conflict"
        }
    }
}
enum CodexBridge {
    static func check(_ status: UInt32) throws {
        if status == 16 { throw CodexSourceFailure.format }
        if status == 17 { throw CodexSourceFailure.conflict }
        try LocalSourceBridge.check(status)
    }
    static func settings(_ memory: UInt64, index: UInt32) throws -> CodexSelection? {
        var out = AmbientCodexSettings()
        var bytes = [UInt8](repeating: 0, count: 4096)
        let status = bytes.withUnsafeMutableBufferPointer {
            ambient_codex_v1_settings_at(memory, index, &out, $0.baseAddress, 4096)
        }
        if status == 14 { return nil }
        try check(status)
        guard out.id > 0, out.id <= UInt64(Int64.max), out.revision > 0,
            out.revision <= UInt64(Int64.max), out.reserved == 0, out.bytes_used <= 4096
        else { throw ModelCheckFailure.native }
        let path = try LocalSourceBridge.decode(out.path, bytes, out.bytes_used)
        guard path.hasPrefix("/"), !path.utf8.contains(0) else { throw ModelCheckFailure.native }
        return CodexSelection(id: out.id, revision: out.revision, path: path)
    }
    static func observation(_ op: CodexOperation) throws -> CodexObservation {
        var out = AmbientCodexWindow()
        var records = [AmbientCodexEvent](repeating: AmbientCodexEvent(), count: 128)
        var bytes = [UInt8](repeating: 0, count: 262144)
        try records.withUnsafeMutableBufferPointer { r in
            try bytes.withUnsafeMutableBufferPointer { b in
                try check(
                    ambient_codex_v1_window(
                        op.memory, op.id, &out, r.baseAddress, 128, b.baseAddress, 262144))
            }
        }
        guard out.count <= 128, out.bytes_used <= 262144, out.reasons <= 255,
            out.physical_lines <= 4096
        else { throw ModelCheckFailure.native }
        func decode(_ s: AmbientSpan) throws -> String {
            try LocalSourceBridge.decode(s, bytes, out.bytes_used)
        }
        let events = try records.prefix(Int(out.count)).enumerated().map { index, r in
            guard (1...9).contains(r.kind), r.reserved == 0, r.has_time <= 1,
                (1...2).contains(r.completeness)
            else { throw ModelCheckFailure.native }
            let turn = try decode(r.turn_id)
            let item = r.kind <= 5 ? try decode(r.item_id) : nil
            guard !turn.isEmpty, turn.utf8.count <= 2048,
                item.map({ !$0.isEmpty && $0.utf8.count <= 2048 }) ?? (r.item_id.length == 0)
            else { throw ModelCheckFailure.native }
            return CodexEvent(
                id: index, ordinal: r.ordinal, kind: r.kind, text: try decode(r.text), turnID: turn,
                itemID: item, observedAt: r.has_time == 1 ? r.observed_at_utc : nil,
                partial: r.completeness == 1)
        }
        let thread = try decode(out.thread_id)
        guard !thread.isEmpty, thread.utf8.count <= 256 else { throw ModelCheckFailure.native }
        return CodexObservation(
            threadID: thread, observedAt: out.observed_at_utc, cursor: out.cursor,
            nextBefore: out.next_before, reasons: out.reasons, physicalLines: out.physical_lines,
            events: events)
    }
    static func preview(_ op: CodexOperation, index: Int) throws -> String {
        guard (0..<128).contains(index) else { throw ModelCheckFailure.invalid }
        var bytes = [UInt8](repeating: 0, count: 16384)
        var count: UInt32 = 0
        try bytes.withUnsafeMutableBufferPointer {
            try check(
                ambient_codex_v1_preview(
                    op.memory, op.id, UInt32(index), $0.baseAddress, 16384, &count))
        }
        return try LocalSourceBridge.decode(AmbientSpan(offset: 0, length: count), bytes, count)
    }
}
extension MemorySession {
    func codexSelections() throws -> [CodexSelection] {
        let handle = try sourceMemoryHandle()
        var selections: [CodexSelection] = []
        for i in 0..<64 {
            guard let selection = try CodexBridge.settings(handle, index: UInt32(i)) else { break }
            selections.append(selection)
        }
        return selections
    }
    func saveCodexSelection(id: UInt64, expected: UInt64, path: String) throws -> CodexSelection {
        let handle = try sourceMemoryHandle()
        var savedID: UInt64 = 0
        var revision: UInt64 = 0
        try LocalSourceBridge.withText(path) {
            try CodexBridge.check(
                ambient_codex_v1_settings_save(handle, id, expected, $0, &savedID, &revision))
        }
        return CodexSelection(id: savedID, revision: revision, path: path)
    }
    func deleteCodexSelection(_ selection: CodexSelection) throws {
        try CodexBridge.check(
            ambient_codex_v1_settings_delete(sourceMemoryHandle(), selection.id, selection.revision)
        )
    }
    func beginCodexRead(
        _ task: MemoryTask, selection: CodexSelection, cursor: UInt64, expiresAt: Int64
    ) throws -> CodexOperation {
        let handle = try sourceMemoryHandle()
        var id: UInt64 = 0
        try CodexBridge.check(
            ambient_codex_v1_begin(
                handle, task.id, task.revision, selection.id, selection.revision, cursor, &id))
        guard id > 0 else { throw ModelCheckFailure.native }
        return CodexOperation(memory: handle, id: id, path: selection.path, expiresAt: expiresAt)
    }
    func codexPreview(_ op: CodexOperation, index: Int) throws -> String {
        guard try sourceMemoryHandle() == op.memory else { throw ModelCheckFailure.closed }
        return try CodexBridge.preview(op, index: index)
    }
    func bindCodexEvent(_ op: CodexOperation, index: Int) throws -> UInt64 {
        guard try sourceMemoryHandle() == op.memory, (0..<128).contains(index) else {
            throw ModelCheckFailure.invalid
        }
        var id: UInt64 = 0
        try CodexBridge.check(
            ambient_codex_v1_bind(
                op.memory, op.id, UInt32(index), Int64(Date().timeIntervalSince1970), &id))
        return id
    }
    func codexMetadata(taskID: UInt64, after: UInt64 = 0) throws -> [CodexSavedMetadata] {
        let handle = try sourceMemoryHandle()
        var page = AmbientPage()
        var rows = [AmbientCodexMetadata](repeating: AmbientCodexMetadata(), count: 50)
        var bytes = [UInt8](repeating: 0, count: 262144)
        try rows.withUnsafeMutableBufferPointer { r in
            try bytes.withUnsafeMutableBufferPointer { b in
                try CodexBridge.check(
                    ambient_codex_v1_saved(
                        handle, taskID, after, &page, r.baseAddress, 50, b.baseAddress, 262144))
            }
        }
        guard page.count <= 50, page.bytes_used <= 262144 else { throw ModelCheckFailure.native }
        return try rows.prefix(Int(page.count)).map { r in
            guard r.binding_id > 0, r.evidence_id > 0, r.selection_id > 0, r.settings_revision > 0,
                (1...9).contains(r.kind), r.reserved == 0
            else { throw ModelCheckFailure.native }
            return CodexSavedMetadata(
                id: r.binding_id, evidenceID: r.evidence_id, selectionID: r.selection_id,
                revision: r.settings_revision, ordinal: r.accepted_ordinal, kind: r.kind,
                turnID: try LocalSourceBridge.decode(r.turn_id, bytes, page.bytes_used),
                itemID: r.kind <= 5
                    ? try LocalSourceBridge.decode(r.item_id, bytes, page.bytes_used) : nil)
        }
    }
}
