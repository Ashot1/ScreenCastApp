import CFNetwork
import Dispatch
import Foundation
import Network

struct ChatGPTCallbackParser {
    private let expected: URL
    private let message = CFHTTPMessageCreateEmpty(nil, true).takeRetainedValue()
    private var received = 0
    private var completed = false
    init(expected: URL) { self.expected = expected }
    mutating func append(_ data: Data) throws -> String? {
        guard !completed, data.count <= 8192 - received else { throw ChatGPTFailure.invalid }
        received += data.count
        let appended = data.withUnsafeBytes { bytes in
            guard let base = bytes.bindMemory(to: UInt8.self).baseAddress else { return true }
            return CFHTTPMessageAppendBytes(message, base, bytes.count)
        }
        guard appended else { throw ChatGPTFailure.invalid }
        guard CFHTTPMessageIsHeaderComplete(message) else { return nil }
        completed = true
        guard let method = CFHTTPMessageCopyRequestMethod(message)?.takeRetainedValue(),
            method as String == "GET",
            let target = CFHTTPMessageCopyRequestURL(message)?.takeRetainedValue() as URL?,
            let host = CFHTTPMessageCopyHeaderFieldValue(message, "Host" as CFString)?
                .takeRetainedValue(),
            host as String == "127.0.0.1:\(expected.port ?? 0)",
            CFHTTPMessageCopyHeaderFieldValue(message, "Transfer-Encoding" as CFString) == nil
        else { throw ChatGPTFailure.invalid }
        if let length = CFHTTPMessageCopyHeaderFieldValue(message, "Content-Length" as CFString)?
            .takeRetainedValue(), length as String != "0"
        {
            throw ChatGPTFailure.invalid
        }
        if let body = CFHTTPMessageCopyBody(message)?.takeRetainedValue(),
            CFDataGetLength(body) != 0
        {
            throw ChatGPTFailure.invalid
        }
        guard let components = URLComponents(url: target, resolvingAgainstBaseURL: false),
            components.percentEncodedPath == expected.path(percentEncoded: true),
            components.fragment == nil,
            components.user == nil, components.password == nil
        else { throw ChatGPTFailure.invalid }
        if components.scheme != nil || components.host != nil {
            guard components.scheme == "http", components.host == "127.0.0.1",
                components.port == expected.port
            else { throw ChatGPTFailure.invalid }
        }
        var result = URLComponents(url: expected, resolvingAgainstBaseURL: false)
        result?.percentEncodedQuery = components.percentEncodedQuery
        guard let url = result?.url, url.absoluteString.utf8.count <= 8192 else {
            throw ChatGPTFailure.invalid
        }
        return url.absoluteString
    }
}
protocol ChatGPTCallbackListener: Sendable {
    func start() async throws -> String
    func next() async throws -> String
    func respond() async
    func close() async
}

/// Network callbacks and CFHTTPMessage access share this actor's serial dispatch queue. The
/// callback path is the sign-in's own: ChatGPT's `/auth/callback`, Google's `/` (the loopback
/// form verified live with the Desktop client).
actor ChatGPTLoopback: ChatGPTCallbackListener {
    private let path: String
    init(path: String = "/auth/callback") { self.path = path }
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.auth.loopback")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    private struct Connection {
        let connection: NWConnection
        var parser: ChatGPTCallbackParser?
        let timer: any DispatchSourceTimer
    }
    private var listener: NWListener?
    private var listenerFinished = true
    private var connections: [UUID: Connection] = [:]
    private var acceptedCount = 0
    private var current: UUID?
    private var queued: String?
    private var expected: URL?
    private var closed = false
    private var ready: CheckedContinuation<String, Error>?
    private var waiting: CheckedContinuation<String, Error>?
    private var closing: [CheckedContinuation<Void, Never>] = []

    func start() async throws -> String {
        guard listener == nil, !closed else { throw ChatGPTFailure.closed }
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        let listener = try NWListener(using: parameters, on: .any)
        self.listener = listener
        listenerFinished = false
        listener.newConnectionLimit = 8
        listener.stateUpdateHandler = { [weak self] state in
            self?.assumeIsolated { $0.listenerState(state) }
        }
        listener.newConnectionHandler = { [weak self] connection in
            self?.assumeIsolated { $0.accept(connection) }
        }
        return try await withCheckedThrowingContinuation { continuation in
            ready = continuation
            listener.start(queue: queue)
        }
    }
    private func listenerState(_ state: NWListener.State) {
        switch state {
        case .ready:
            guard !closed, let port = listener?.port,
                let url = URL(string: "http://127.0.0.1:\(port.rawValue)\(path)")
            else {
                requestClose()
                return
            }
            expected = url
            ready?.resume(returning: url.absoluteString)
            ready = nil
        case .failed, .cancelled:
            listenerFinished = true
            ready?.resume(throwing: ChatGPTFailure.cancelled)
            ready = nil
            if !closed { requestClose() }
            drained()
        default: break
        }
    }
    private func accept(_ connection: NWConnection) {
        let id = UUID()
        let timer = DispatchSource.makeTimerSource(queue: queue)
        let canRead =
            !closed && acceptedCount < 8 && connections.values.filter { $0.parser != nil }.count < 2
        acceptedCount += 1
        let parser = canRead ? expected.map(ChatGPTCallbackParser.init) : nil
        connections[id] = Connection(connection: connection, parser: parser, timer: timer)
        connection.stateUpdateHandler = { [weak self] state in
            self?.assumeIsolated { $0.connectionState(id, state) }
        }
        timer.setEventHandler { [weak self] in
            self?.assumeIsolated { $0.connections[id]?.connection.cancel() }
        }
        timer.schedule(deadline: .now() + 5)
        timer.resume()
        connection.start(queue: queue)
        if parser == nil { connection.cancel() }
    }
    private func connectionState(_ id: UUID, _ state: NWConnection.State) {
        switch state {
        case .ready: receive(id)
        case .failed, .cancelled:
            connections.removeValue(forKey: id)?.timer.cancel()
            if current == id { current = nil }
            if acceptedCount >= 8 && connections.isEmpty { requestClose() }
            drained()
        default: break
        }
    }
    private func receive(_ id: UUID) {
        guard let context = connections[id], context.parser != nil, !closed else { return }
        context.connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) {
            [weak self] data, _, complete, error in
            self?.assumeIsolated {
                $0.received(id, data: data, complete: complete, failed: error != nil)
            }
        }
    }
    private func received(_ id: UUID, data: Data?, complete: Bool, failed: Bool) {
        guard var context = connections[id], var parser = context.parser else { return }
        guard !closed, !failed, let data, !data.isEmpty else {
            context.connection.cancel()
            return
        }
        do {
            let url = try parser.append(data)
            context.parser = parser
            connections[id] = context
            if let url {
                guard current == nil, queued == nil else {
                    context.connection.cancel()
                    return
                }
                current = id
                if let waiting {
                    self.waiting = nil
                    waiting.resume(returning: url)
                } else {
                    queued = url
                }
            } else if complete {
                context.connection.cancel()
            } else {
                receive(id)
            }
        } catch { context.connection.cancel() }
    }
    func next() async throws -> String {
        guard !closed, waiting == nil else { throw ChatGPTFailure.cancelled }
        if let queued {
            self.queued = nil
            return queued
        }
        return try await withCheckedThrowingContinuation { waiting = $0 }
    }
    func respond() async {
        guard let id = current, let context = connections[id] else { return }
        current = nil
        let body = "You may return to Ambient."
        let response =
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n\(body)"
        let connection = context.connection
        // Cancelling before this completion may discard the static browser response.
        await withCheckedContinuation { continuation in
            connection.send(
                content: Data(response.utf8),
                completion: .contentProcessed { _ in
                    connection.cancel()
                    continuation.resume()
                })
        }
    }
    private func requestClose() {
        closed = true
        queued = nil
        ready?.resume(throwing: ChatGPTFailure.cancelled)
        ready = nil
        waiting?.resume(throwing: ChatGPTFailure.cancelled)
        waiting = nil
        listener?.cancel()
        for context in connections.values { context.connection.cancel() }
    }
    private func drained() {
        guard closed, listenerFinished, connections.isEmpty else { return }
        for continuation in closing { continuation.resume() }
        closing.removeAll()
    }
    func close() async {
        requestClose()
        if listenerFinished && connections.isEmpty { return }
        await withCheckedContinuation { closing.append($0) }
    }
}
