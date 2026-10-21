import AVFAudio
import Foundation

@MainActor final class NativeVoiceMicrophone: NSObject, VoiceMicrophone, AVAudioRecorderDelegate {
    private var recorder: AVAudioRecorder?
    private var finished: (@MainActor @Sendable (Bool) -> Void)?
    var elapsed: TimeInterval { recorder?.currentTime ?? 0 }
    func requestPermission(_ completion: @escaping @MainActor @Sendable (Bool) -> Void) {
        AVAudioApplication.requestRecordPermission { granted in
            Task { @MainActor in completion(granted) }
        }
    }
    func start(at url: URL, finished: @escaping @MainActor @Sendable (Bool) -> Void) throws {
        let recorder = try AVAudioRecorder(
            url: url,
            settings: [
                AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: 16000,
                AVNumberOfChannelsKey: 1, AVLinearPCMBitDepthKey: 16,
                AVLinearPCMIsFloatKey: false, AVLinearPCMIsBigEndianKey: false,
            ])
        recorder.delegate = self
        self.recorder = recorder
        self.finished = finished
        guard recorder.record(forDuration: 60) else {
            stop()
            throw VoiceReplyFailure.recorder
        }
    }
    func stop() {
        finished = nil
        recorder?.stop()
        recorder = nil
    }
    nonisolated func audioRecorderDidFinishRecording(
        _ recorder: AVAudioRecorder, successfully flag: Bool
    ) {
        let identity = ObjectIdentifier(recorder)
        Task { @MainActor [weak self] in
            guard let self, self.recorder.map(ObjectIdentifier.init) == identity else { return }
            self.finished?(flag)
        }
    }
    nonisolated func audioRecorderEncodeErrorDidOccur(
        _ recorder: AVAudioRecorder, error: (any Error)?
    ) {
        let identity = ObjectIdentifier(recorder)
        Task { @MainActor [weak self] in
            guard let self, self.recorder.map(ObjectIdentifier.init) == identity else { return }
            self.finished?(false)
        }
    }
}
