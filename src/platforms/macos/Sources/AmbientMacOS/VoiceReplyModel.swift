import Foundation
import Observation
import VoiceSupport

@MainActor protocol VoiceMicrophone: AnyObject {
    var elapsed: TimeInterval { get }
    func requestPermission(_ completion: @escaping @MainActor @Sendable (Bool) -> Void)
    func start(at url: URL, finished: @escaping @MainActor @Sendable (Bool) -> Void) throws
    func stop()
}

enum VoicePhase: Equatable {
    case idle, permission, recording, limit, transcribing, stopping
}
enum VoiceReplyFailure: Error, Equatable {
    case permission, recorder, models, recognition, timeout, channel, cleanup, stale
    var key: String { "voice.error.\(self)" }
}

@Observable @MainActor final class VoiceReplyModel {
    private(set) var phase = VoicePhase.idle
    private(set) var failure: VoiceReplyFailure?
    private(set) var silence = false
    private(set) var elapsed: TimeInterval = 0
    private(set) var modelDirectory: URL?
    var busy: Bool { phase != .idle || work != nil }
    var canRecord: Bool { !busy && failure != .cleanup && modelDirectory != nil }
    private let microphone: any VoiceMicrophone
    private let workspace: VoiceWorkspace
    private let supervisor: VoiceSupervisor
    private let preferences: UserDefaults?
    private static let modelKey = "ambient.voice.local-model-directory"
    private var generation: UInt64 = 0
    private var lease: VoiceLease?
    private var work: Task<Void, Never>?
    private var timer: Task<Void, Never>?
    private var validate: (@MainActor () async -> Bool)?
    private var apply: (@MainActor (String) -> Bool)?
    private var models: URL?
    init(
        microphone: any VoiceMicrophone, workspace: VoiceWorkspace, supervisor: VoiceSupervisor,
        preferences: UserDefaults? = nil
    ) {
        self.microphone = microphone
        self.workspace = workspace
        self.supervisor = supervisor
        self.preferences = preferences
        if let path = preferences?.string(forKey: Self.modelKey), path.hasPrefix("/"),
            path.utf8.count <= 4096, !path.utf8.contains(0)
        {
            modelDirectory = URL(fileURLWithPath: path, isDirectory: true)
        }
    }
    static func production() -> VoiceReplyModel {
        VoiceReplyModel(
            microphone: NativeVoiceMicrophone(),
            workspace: VoiceWorkspace(
                root: FileManager.default.temporaryDirectory
                    .appendingPathComponent("dev.ambient.voice", isDirectory: true)),
            supervisor: VoiceSupervisor(
                executable: Bundle.main.bundleURL
                    .appendingPathComponent(
                        "Contents/Helpers/AmbientVoiceHelper.app/Contents/MacOS/AmbientVoiceHelper")
            ),
            preferences: .standard)
    }
    func selectModels(_ directory: URL?) {
        guard !busy else { return }
        modelDirectory = directory
        if let directory {
            preferences?.set(directory.path, forKey: Self.modelKey)
        } else {
            preferences?.removeObject(forKey: Self.modelKey)
        }
        if failure == .models { failure = nil }
    }
    func startupCleanup() async {
        guard !busy else { return }
        phase = .stopping
        work = Task {
            defer {
                self.work = nil
                self.phase = .idle
            }
            do {
                try await self.workspace.cleanupStale()
                self.failure = nil
            } catch { self.failure = .cleanup }
        }
        if let work { await work.value }
    }
    func start(
        models: URL, validate: @escaping @MainActor () async -> Bool,
        apply: @escaping @MainActor (String) -> Bool
    ) {
        guard !busy, failure != .cleanup else { return }
        generation += 1
        let token = generation
        self.models = models
        self.validate = validate
        self.apply = apply
        failure = nil
        silence = false
        elapsed = 0
        phase = .permission
        work = Task {
            let allowed = await validate()
            guard self.generation == token else {
                self.work = nil
                self.phase = .idle
                return
            }
            self.work = nil
            guard allowed else {
                self.phase = .idle
                self.failure = .stale
                return
            }
            // The system callback has no cancellable lifetime and owns no recorder or operation.
            self.microphone.requestPermission { [weak self = self] granted in
                self?.permissionReturned(granted, token: token)
            }
        }
    }
    private func permissionReturned(_ granted: Bool, token: UInt64) {
        guard generation == token, phase == .permission, work == nil else { return }
        guard granted else {
            phase = .idle
            failure = .permission
            return
        }
        work = Task {
            defer { self.work = nil }
            guard let validate = self.validate, await validate(), self.generation == token else {
                self.phase = .idle
                if self.generation == token { self.failure = .stale }
                return
            }
            do {
                let lease = try await self.workspace.prepare()
                self.lease = lease
                guard self.generation == token, await validate(), self.generation == token else {
                    await self.cleanup(
                        lease, failure: self.generation == token ? .stale : nil, token: token)
                    return
                }
                try self.microphone.start(at: lease.audioURL) { [weak self = self] success in
                    self?.recordingEnded(success, token: token)
                }
                self.phase = .recording
                self.observeRecording(lease, token: token)
            } catch {
                if let lease = self.lease {
                    await self.cleanup(lease, failure: .recorder, token: token)
                } else {
                    self.phase = .idle
                    if self.generation == token { self.failure = .cleanup }
                }
            }
        }
    }
    private func observeRecording(_ lease: VoiceLease, token: UInt64) {
        timer = Task {
            while self.generation == token, self.phase == .recording {
                do { try await Task.sleep(for: .milliseconds(200)) } catch { return }
                guard self.generation == token, self.phase == .recording else { return }
                self.elapsed = self.microphone.elapsed
                do {
                    let bytes = try await self.workspace.audioSize(lease)
                    guard self.generation == token, self.phase == .recording else { return }
                    if bytes >= VoiceAudio.maximumBytes || self.elapsed >= 60 {
                        self.recordingEnded(true, token: token)
                    }
                } catch { self.abort(.recorder) }
            }
        }
    }
    private func recordingEnded(_ success: Bool, token: UInt64) {
        guard generation == token, phase == .recording else { return }
        elapsed = max(elapsed, microphone.elapsed)
        microphone.stop()
        timer?.cancel()
        timer = nil
        if success { phase = .limit } else { abort(.recorder) }
    }
    func finish() {
        guard phase == .recording || phase == .limit, work == nil, let lease,
            let validate, let apply, let models
        else { return }
        let token = generation
        microphone.stop()
        timer?.cancel()
        timer = nil
        phase = .transcribing
        work = Task {
            defer { self.work = nil }
            var result: VoiceResult?
            var failure: VoiceReplyFailure?
            if await validate(), self.generation == token {
                do { result = try await self.supervisor.run(lease, models: models) } catch {
                    switch error as? VoiceFailure {
                    case .timeout: failure = .timeout
                    case .cancelled: break
                    case .invalidChannel, .storage: failure = .channel
                    default: failure = .recognition
                    }
                }
            } else {
                failure = .stale
            }
            guard !(await self.supervisor.running) else {
                self.phase = .idle
                self.failure = .cleanup
                return
            }
            await self.cleanup(lease, failure: failure, token: token)
            guard self.failure != .cleanup, self.generation == token else { return }
            switch result {
            case .text(let text):
                guard await validate(), self.generation == token, apply(text) else {
                    if self.generation == token { self.failure = .stale }
                    return
                }
            case .silence: self.silence = true
            case .failure(let code):
                switch code {
                case .models: self.failure = .models
                case .limit: self.failure = .channel
                case .input: self.failure = .recorder
                case .recognition: self.failure = .recognition
                }
            case nil: break
            }
            self.validate = nil
            self.apply = nil
            self.models = nil
        }
    }
    func cancel() { abort(nil) }
    private func abort(_ failure: VoiceReplyFailure?) {
        generation += 1
        validate = nil
        apply = nil
        models = nil
        timer?.cancel()
        timer = nil
        microphone.stop()
        if let work {
            phase = .stopping
            work.cancel()
        } else if let lease, self.failure != .cleanup {
            phase = .stopping
            let token = generation
            work = Task {
                await self.cleanup(lease, failure: failure, token: token)
                self.work = nil
            }
        } else {
            phase = .idle
        }
    }
    private func cleanup(_ lease: VoiceLease, failure: VoiceReplyFailure?, token: UInt64) async {
        do {
            try await workspace.cleanup(lease)
            self.lease = nil
            if generation == token { self.failure = failure }
        } catch { self.failure = .cleanup }
        phase = .idle
    }
    func retryCleanup() {
        guard !busy, failure == .cleanup else { return }
        phase = .stopping
        work = Task {
            defer {
                self.work = nil
                self.phase = .idle
            }
            guard !(await self.supervisor.running) else { return }
            do {
                if let lease = self.lease {
                    try await self.workspace.cleanup(lease)
                    self.lease = nil
                } else {
                    try await self.workspace.cleanupStale()
                }
                self.failure = nil
            } catch { self.failure = .cleanup }
        }
    }
    func shutdown() async -> Bool {
        cancel()
        if let work { await work.value }
        let running = await supervisor.running
        return !busy && failure != .cleanup && !running
    }
}
