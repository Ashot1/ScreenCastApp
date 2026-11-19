import CAmbient
import Foundation

enum ObservationBridge {
    static func cadence(_ raw: AmbientObserveNext) throws -> ObservationCadence {
        switch raw.capture {
        case 1: return .capture(after: raw.seconds)
        case 2 where raw.seconds == 0: return .hold
        default: throw ModelCheckFailure.native
        }
    }
    static func submit(
        _ frame: ObservationFrame, memory: UInt64, idleSeconds: UInt32, at: ObservationTime
    ) throws -> ObservationFrameResult {
        guard frame.lines.count <= 4096, frame.lines.reduce(0, { $0 + $1.words.count }) <= 16384,
            !frame.jpeg.isEmpty, frame.jpeg.count <= 786432
        else { throw ModelCheckFailure.limit }
        var strings: [UInt8] = []
        func span(_ value: String) throws -> AmbientSpan {
            let bytes = Array(value.utf8)
            guard bytes.count <= 8_388_608 - strings.count else { throw ModelCheckFailure.limit }
            let out = AmbientSpan(offset: UInt32(strings.count), length: UInt32(bytes.count))
            strings.append(contentsOf: bytes)
            return out
        }
        let display = try span(frame.displayKey)
        let bundle = try span(frame.bundleID)
        let app = try span(frame.appName)
        let title = try span(frame.title)
        var words: [AmbientObservedWord] = []
        let lines = try frame.lines.map { line in
            let text = try span(line.text)
            for word in line.words {
                words.append(
                    AmbientObservedWord(
                        text: try span(word.text),
                        rect: AmbientObservedRect(
                            x: word.rect.origin.x, y: word.rect.origin.y,
                            width: word.rect.width, height: word.rect.height)))
            }
            return AmbientObservedLine(text: text, word_count: UInt32(line.words.count))
        }
        var out = AmbientObserveFrameResult()
        // Every borrowed input remains live through the synchronous ABI call; Rust retains no pointer.
        try strings.withUnsafeBufferPointer { text in
            try lines.withUnsafeBufferPointer { lines in
                try words.withUnsafeBufferPointer { words in
                    try frame.jpeg.withUnsafeBytes { jpeg in
                        var raw = AmbientObservedFrame(
                            strings: AmbientText(
                                bytes: text.baseAddress, length: UInt32(text.count)),
                            display_key: display, bundle_id: bundle, app_name: app, title: title,
                            lines: lines.baseAddress, words: words.baseAddress,
                            jpeg: AmbientText(
                                bytes: jpeg.bindMemory(to: UInt8.self).baseAddress,
                                length: UInt32(jpeg.count)),
                            line_count: UInt32(lines.count), word_count: UInt32(words.count),
                            idle_seconds: idleSeconds, reserved: 0)
                        try ModelCheckBridge.check(
                            ambient_observe_v1_submit_frame(
                                memory, at.utc, at.monotonicMS, &raw, &out))
                    }
                }
            }
        }
        guard let disposition = ObservationDisposition(rawValue: out.disposition),
            out.reserved == 0,
            (out.frame_id != 0) == (disposition == .recorded), out.frame_id <= UInt64(Int64.max)
        else { throw ModelCheckFailure.native }
        return .init(
            frameID: out.frame_id == 0 ? nil : out.frame_id,
            highlightVersion: out.highlight_version,
            disposition: disposition, next: try cadence(out.next))
    }
    static func json<T: Decodable>(
        _ type: T.Type, limit: Int,
        call: (UnsafeMutablePointer<UInt8>?, UInt32, UnsafeMutablePointer<UInt32>) -> UInt32
    ) throws -> T {
        var bytes = [UInt8](repeating: 0, count: 1024)
        var length: UInt32 = 0
        var status = bytes.withUnsafeMutableBufferPointer {
            call($0.baseAddress, UInt32($0.count), &length)
        }
        if status == 6 {
            guard length > bytes.count, length <= limit else { throw ModelCheckFailure.limit }
            bytes = [UInt8](repeating: 0, count: Int(length))
            status = bytes.withUnsafeMutableBufferPointer {
                call($0.baseAddress, UInt32($0.count), &length)
            }
        }
        try ModelCheckBridge.check(status)
        guard length <= bytes.count, length <= limit else { throw ModelCheckFailure.native }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do { return try decoder.decode(type, from: Data(bytes.prefix(Int(length)))) } catch {
            throw ModelCheckFailure.native
        }
    }
}
