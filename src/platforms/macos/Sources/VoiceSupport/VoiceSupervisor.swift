import CVoiceProcess
import Darwin
import Foundation

/// The C mutex serializes signals with the sole reaper; Swift never reads or stores a PID.
private final class VoiceChild: @unchecked Sendable {
    private let handle: OpaquePointer
    init(executable: URL, temporary: URL, request: Data) throws {
        let child = request.withUnsafeBytes { bytes in
            ambient_voice_child_spawn(
                executable.path, temporary.path,
                bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
        }
        guard let child else { throw VoiceFailure.recognition }
        handle = child
    }
    func signal(_ number: Int32) { _ = ambient_voice_child_signal(handle, number) }
    func wait() -> Int32 { ambient_voice_child_wait(handle) }
    deinit { ambient_voice_child_destroy(handle) }
}

public actor VoiceSupervisor {
    private let executable: URL
    private let deadline: Duration
    private let waitQueue = DispatchQueue(
        label: "dev.ambient.voice-child-exit", qos: .userInitiated)
    private var child: VoiceChild?
    private var operation: UUID?
    private var completion: CheckedContinuation<VoiceResult, any Error>?
    private var lease: VoiceLease?
    private var stopping: VoiceFailure?
    private var deadlineTask: Task<Void, Never>?
    private var escalationTask: Task<Void, Never>?
    public init(executable: URL, deadline: Duration = .seconds(300)) {
        self.executable = executable
        self.deadline = deadline
    }
    public var running: Bool { child != nil }
    public func run(_ lease: VoiceLease, models: URL) async throws -> VoiceResult {
        guard child == nil else { throw VoiceFailure.recognition }
        try Task.checkCancellation()
        let id = UUID()
        let request = try VoiceChannel.encodeRequest(lease.request(models: models))
        let started = ContinuousClock.now
        let child = try VoiceChild(
            executable: executable, temporary: lease.directory, request: request)
        self.child = child
        self.lease = lease
        operation = id
        stopping = nil
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                completion = continuation
                waitQueue.async {
                    let status = child.wait()
                    Task { await self.didExit(id, status: status) }
                }
                deadlineTask = Task {
                    do {
                        try await ContinuousClock().sleep(
                            until: started.advanced(by: self.deadline))
                    } catch { return }
                    self.stop(id, reason: .timeout)
                }
            }
        } onCancel: {
            Task { await self.stop(id, reason: .cancelled) }
        }
    }
    public func cancel() {
        guard let operation else { return }
        stop(operation, reason: .cancelled)
    }
    private func stop(_ id: UUID, reason: VoiceFailure) {
        guard operation == id, let child, stopping == nil else { return }
        stopping = reason
        child.signal(SIGTERM)
        escalationTask = Task {
            do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
            guard self.operation == id else { return }
            self.child?.signal(SIGKILL)
        }
    }
    private func didExit(_ id: UUID, status: Int32) {
        guard operation == id else { return }
        deadlineTask?.cancel()
        escalationTask?.cancel()
        deadlineTask = nil
        escalationTask = nil
        guard status >= 0 else {
            // An unknown wait outcome cannot release the operation or authorize file cleanup.
            completion?.resume(throwing: VoiceFailure.storage)
            completion = nil
            return
        }
        let result: Result<VoiceResult, any Error>
        if let stopping {
            result = .failure(stopping)
        } else if status != 0 {
            result = .failure(VoiceFailure.recognition)
        } else if let lease {
            result = Result { try lease.readResult() }
        } else {
            result = .failure(VoiceFailure.storage)
        }
        child = nil
        operation = nil
        lease = nil
        stopping = nil
        let continuation = completion
        completion = nil
        continuation?.resume(with: result)
    }
}
