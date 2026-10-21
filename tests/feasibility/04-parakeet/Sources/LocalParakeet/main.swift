import FluidAudio
import Foundation

// Test helper only: drop library diagnostics that can contain recognized tokens.
AppLogger.minimumLevel = .fault
AppLogger.mirrorsToConsole = false
let arguments = CommandLine.arguments
if arguments.count != 5 {
    FileHandle.standardError.write(
        Data("expected models, completed audio, result, ready paths\n".utf8))
    exit(64)
}
do {
    let models = try AsrModels.loadLocal(
        from: URL(fileURLWithPath: arguments[1]), version: .v3, encoderPrecision: .int8)
    let manager = AsrManager(
        config: ASRConfig(
            tdtConfig: TdtConfig(blankId: AsrModelVersion.v3.blankId),
            encoderHiddenSize: AsrModelVersion.v3.encoderHiddenSize,
            // The disk-backed URL path uses an unowned Foundation temp directory.
            streamingEnabled: false))
    try await manager.loadModels(models)
    var state = TdtDecoderState.make(decoderLayers: await manager.decoderLayerCount)
    try Data().write(to: URL(fileURLWithPath: arguments[4]))
    let start = Date()
    let result = try await manager.transcribe(
        URL(fileURLWithPath: arguments[2]), decoderState: &state)
    // Text uses an owned ephemeral result channel, never a diagnostic stream.
    let output: [String: Any] = [
        "text": result.text, "inference_s": Date().timeIntervalSince(start),
        "audio_s": result.duration,
    ]
    try JSONSerialization.data(withJSONObject: output).write(
        to: URL(fileURLWithPath: arguments[3]), options: .atomic)
    await manager.cleanup()
} catch {
    // Framework errors can include user paths; return only a stable diagnostic code.
    FileHandle.standardError.write(Data("local-parakeet-failed\n".utf8))
    exit(1)
}
