import CAmbient
import Dispatch
import Foundation

protocol ChatGPTProtocol: Sendable {
    func begin(_ envelope: ChatGPTEnvelope, redirect: String?, refresh: Bool) async throws -> UInt64
    func prepare(_ handle: UInt64) async throws -> String?
    func callback(_ handle: UInt64, url: String) async throws -> String
    func dispatch(_ handle: UInt64, clientID: String) async throws -> ChatGPTEnvelope
    func close(_ handle: UInt64) async throws
    func cancel(_ handle: UInt64)
}
/// Synchronous Rust I/O runs on its own native serial queue, separate from memory and UI.
actor ChatGPTBridge: ChatGPTProtocol {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.auth.protocol")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }

    func begin(_ envelope: ChatGPTEnvelope, redirect: String?, refresh: Bool) throws -> UInt64 {
        try envelope.validate()
        var (record, bytes) = encode(envelope)
        let redirectBytes = Array((redirect ?? "").utf8)
        var handle: UInt64 = 0
        try bytes.withUnsafeBufferPointer { input in
            try redirectBytes.withUnsafeBufferPointer { uri in
                try Self.check(
                    ambient_auth_v1_begin(
                        refresh ? 1 : 0, &record, input.baseAddress, UInt32(input.count),
                        uri.baseAddress, UInt32(uri.count), &handle))
            }
        }
        guard handle != 0 else { throw ChatGPTFailure.response }
        bytes.removeAll()
        return handle
    }
    func prepare(_ handle: UInt64) throws -> String? {
        let (record, bytes) = try output { ambient_auth_v1_prepare(handle, $0, $1, $2) }
        return record.url.length == 0 ? nil : try string(record.url, bytes)
    }
    func callback(_ handle: UInt64, url: String) throws -> String {
        let input = Array(url.utf8)
        let (record, bytes) = try input.withUnsafeBufferPointer { input in
            try output {
                ambient_auth_v1_callback(handle, input.baseAddress, UInt32(input.count), $0, $1, $2)
            }
        }
        return try string(record.client_id, bytes)
    }
    func dispatch(_ handle: UInt64, clientID: String) throws -> ChatGPTEnvelope {
        let id = Array(clientID.utf8)
        let (record, bytes) = try id.withUnsafeBufferPointer { input in
            try output {
                ambient_auth_v1_dispatch(handle, input.baseAddress, UInt32(input.count), $0, $1, $2)
            }
        }
        guard record.registration <= 3, record.has_hint <= 1, record.has_credentials <= 1,
            record.scope_count <= 64, record.reserved == 0
        else { throw ChatGPTFailure.response }
        var e = ChatGPTEnvelope(
            host: try string(record.host, bytes), registration: record.registration)
        if record.registration >= 2 { e.clientID = try string(record.client_id, bytes) }
        if record.registration == 3 {
            e.issuer = try string(record.issuer, bytes)
            e.subject = try string(record.subject, bytes)
        }
        if record.has_hint == 1 {
            e.hint = try string(record.hint, bytes)
            e.nonce = try string(record.nonce, bytes)
        }
        if record.has_credentials == 1 {
            var spans = record.scopes
            let scopes = try withUnsafeBytes(of: &spans) { raw in
                try raw.bindMemory(to: AmbientAuthSpan.self).prefix(Int(record.scope_count)).map {
                    try string($0, bytes)
                }
            }
            e.credentials = ChatGPTCredentials(
                access: try string(record.access, bytes),
                refresh: try string(record.refresh, bytes), scopes: scopes,
                expiresAt: record.expires_at)
        }
        try e.validate()
        return e
    }
    nonisolated func cancel(_ handle: UInt64) {
        // Native generation fencing owns commit cancellation; close reports lifecycle errors
        // after the admitted call returns, including a race with an already closed handle.
        _ = ambient_auth_v1_cancel(handle)
    }
    func close(_ handle: UInt64) throws { try Self.check(ambient_auth_v1_close(handle)) }

    private func output(
        _ call: (UnsafeMutablePointer<AmbientAuthRecord>, UnsafeMutablePointer<UInt8>?, UInt32) ->
            UInt32
    ) throws -> (AmbientAuthRecord, [UInt8]) {
        var record = AmbientAuthRecord()
        var bytes = [UInt8](repeating: 0, count: 65_536)
        try bytes.withUnsafeMutableBufferPointer {
            try Self.check(call(&record, $0.baseAddress, UInt32($0.count)))
        }
        guard record.bytes_used <= 65_536 else { throw ChatGPTFailure.response }
        return (record, Array(bytes.prefix(Int(record.bytes_used))))
    }
    private func string(_ span: AmbientAuthSpan, _ bytes: [UInt8]) throws -> String {
        let start = Int(span.offset)
        let end = start + Int(span.length)
        guard start <= end, end <= bytes.count,
            let value = String(bytes: bytes[start..<end], encoding: .utf8)
        else { throw ChatGPTFailure.response }
        return value
    }
    private func encode(_ e: ChatGPTEnvelope) -> (AmbientAuthRecord, [UInt8]) {
        var r = AmbientAuthRecord()
        var bytes: [UInt8] = []
        func span(_ s: String) -> AmbientAuthSpan {
            let value = AmbientAuthSpan(offset: UInt32(bytes.count), length: UInt32(s.utf8.count))
            bytes.append(contentsOf: s.utf8)
            return value
        }
        r.registration = e.registration
        r.host = span(e.host)
        if let id = e.clientID { r.client_id = span(id) }
        if let issuer = e.issuer { r.issuer = span(issuer) }
        if let subject = e.subject { r.subject = span(subject) }
        if let hint = e.hint, let nonce = e.nonce {
            r.has_hint = 1
            r.hint = span(hint)
            r.nonce = span(nonce)
        }
        if let c = e.credentials {
            r.has_credentials = 1
            r.access = span(c.access)
            r.refresh = span(c.refresh)
            r.expires_at = c.expiresAt
            r.scope_count = UInt32(c.scopes.count)
            let scopes = c.scopes.map(span)
            withUnsafeMutableBytes(of: &r.scopes) { raw in
                let output = raw.bindMemory(to: AmbientAuthSpan.self)
                for (i, s) in scopes.enumerated() { output[i] = s }
            }
        }
        r.bytes_used = UInt32(bytes.count)
        return (r, bytes)
    }
    private nonisolated static func check(_ status: UInt32) throws {
        switch status {
        case 0: return
        case 1, 2, 9, 11: throw ChatGPTFailure.invalid
        case 3: throw ChatGPTFailure.cancelled
        case 4: throw ChatGPTFailure.expired
        case 5: throw ChatGPTFailure.network
        case 7: throw ChatGPTFailure.identity
        case 8: throw ChatGPTFailure.denied
        case 12: throw ChatGPTFailure.closed
        case 13: throw ChatGPTFailure.busy
        default: throw ChatGPTFailure.response
        }
    }
}
