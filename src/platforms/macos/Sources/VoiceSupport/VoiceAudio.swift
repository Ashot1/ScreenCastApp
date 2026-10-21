import Foundation

public enum VoiceFailure: Error, Equatable, Sendable {
    case invalidAudio, invalidChannel, storage, cleanup, models, recognition, limit, cancelled,
        timeout
}

public enum VoiceAudio {
    public static let maximumBytes = 2 * 1024 * 1024
    public static let maximumSamples = 60 * 16000

    public static func samples(_ data: Data) throws -> [Float] {
        guard data.count >= 44, data.count <= maximumBytes else { throw VoiceFailure.invalidAudio }
        let bytes = [UInt8](data)
        func u16(_ p: Int) -> Int { Int(bytes[p]) | Int(bytes[p + 1]) << 8 }
        func u32(_ p: Int) -> Int { u16(p) | u16(p + 2) << 16 }
        func tag(_ p: Int, _ s: String) -> Bool {
            Array(bytes[p..<(p + 4)]) == Array(s.utf8)
        }
        guard tag(0, "RIFF"), tag(8, "WAVE"), u32(4) == bytes.count - 8 else {
            throw VoiceFailure.invalidAudio
        }
        var position = 12
        var formatted = false
        var audio: Range<Int>?
        while position + 8 <= bytes.count {
            let size = u32(position + 4)
            let start = position + 8
            guard size <= bytes.count - start else { throw VoiceFailure.invalidAudio }
            if tag(position, "fmt ") {
                guard !formatted, size >= 16, u16(start) == 1, u16(start + 2) == 1,
                    u32(start + 4) == 16000, u32(start + 8) == 32000,
                    u16(start + 12) == 2, u16(start + 14) == 16
                else { throw VoiceFailure.invalidAudio }
                formatted = true
            } else if tag(position, "data") {
                guard audio == nil, size > 0, size.isMultiple(of: 2), size / 2 <= maximumSamples
                else {
                    throw VoiceFailure.invalidAudio
                }
                audio = start..<(start + size)
            }
            position = start + size + size % 2
        }
        guard formatted, let audio, position == bytes.count else { throw VoiceFailure.invalidAudio }
        return stride(from: audio.lowerBound, to: audio.upperBound, by: 2).map {
            Float(Int16(bitPattern: UInt16(u16($0)))) / 32768
        }
    }
}
