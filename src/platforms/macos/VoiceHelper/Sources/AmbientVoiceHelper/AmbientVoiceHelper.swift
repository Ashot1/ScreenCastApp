import CVoiceProcess
import Darwin
import FluidAudio
import Foundation
import VoiceSupport

@main enum AmbientVoiceHelper {
    static func main() async {
        // Configure before constructing any FluidAudio object; raw library errors never cross the channel.
        AppLogger.minimumLevel = .fault
        AppLogger.mirrorsToConsole = false
        ModelHub.offlineMode = true
        var bytes = [UInt8](repeating: 0, count: VoiceChannel.maximumRequestBytes)
        let count = ambient_voice_read_request(&bytes, bytes.count)
        guard count > 0, ambient_voice_watch_parent() == 0 else { _exit(65) }
        do {
            let request = try VoiceChannel.decodeRequest(Data(bytes.prefix(Int(count))))
            let lease = try VoiceLease.open(request)
            let result: VoiceResult
            do {
                let samples = try lease.audioSamples()
                try VoiceModels.validate(URL(fileURLWithPath: request.modelDirectory))
                if samples.allSatisfy({ $0 == 0 }) {
                    result = .silence
                } else {
                    result = try await transcribe(
                        samples, models: URL(fileURLWithPath: request.modelDirectory))
                }
            } catch {
                switch error as? VoiceFailure {
                case .models: result = .failure(.models)
                case .invalidAudio, .storage: result = .failure(.input)
                case .limit, .invalidChannel: result = .failure(.limit)
                default: result = .failure(.recognition)
                }
            }
            try lease.writeResult(result)
        } catch { _exit(65) }
    }
    private static func transcribe(_ samples: [Float], models directory: URL) async throws
        -> VoiceResult
    {
        let version = AsrModelVersion.v3
        let models = try AsrModels.loadLocal(
            from: directory, version: version, encoderPrecision: .int8)
        let config = ASRConfig(
            sampleRate: 16000, encoderHiddenSize: version.encoderHiddenSize,
            streamingEnabled: false)
        let manager = AsrManager(config: config)
        do {
            try await manager.loadModels(models)
            var state = try TdtDecoderState(decoderLayers: version.decoderLayers)
            let response = try await manager.transcribe(samples, decoderState: &state)
            await manager.cleanup()
            let text = response.text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard text.utf8.count <= VoiceChannel.maximumTextBytes else { return .failure(.limit) }
            return text.isEmpty ? .silence : .text(text)
        } catch {
            await manager.cleanup()
            throw VoiceFailure.recognition
        }
    }
}
