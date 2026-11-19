import CoreGraphics
import Foundation

struct ObservationPolicy: Codable, Equatable, Sendable {
    let started: Bool
    let perHourLimit: UInt32
    let creditAcknowledged: Bool
}
struct ObservationTime: Equatable, Sendable {
    let utc: Int64
    let monotonicMS: UInt64
}
/// One process clock also clamps wall-clock corrections, preserving the Core call order.
struct ObservationClock: Sendable {
    private let now: @Sendable () -> ObservationTime
    init(
        now: @escaping @Sendable () -> ObservationTime = {
            .init(
                utc: Int64(Date().timeIntervalSince1970),
                monotonicMS: UInt64(ProcessInfo.processInfo.systemUptime * 1000))
        }
    ) { self.now = now }
    private var lastUTC = Int64.min
    private var lastMonotonic: UInt64 = 0
    mutating func sample() -> ObservationTime {
        clamp(now())
    }
    mutating func sample(utc: Int64, uptime: TimeInterval) -> ObservationTime {
        clamp(.init(utc: utc, monotonicMS: UInt64(max(0, uptime) * 1000)))
    }
    private mutating func clamp(_ time: ObservationTime) -> ObservationTime {
        lastUTC = max(lastUTC, time.utc)
        lastMonotonic = max(lastMonotonic, time.monotonicMS)
        return .init(utc: lastUTC, monotonicMS: lastMonotonic)
    }
}
struct ObservationWord: Equatable, Sendable {
    let text: String
    let rect: CGRect
}
struct ObservationLine: Equatable, Sendable {
    let text: String
    let words: [ObservationWord]
}
struct ObservationFrame: Sendable {
    let displayKey: String
    let bundleID: String
    let appName: String
    let title: String
    let lines: [ObservationLine]
    let jpeg: Data
}
enum ObservationCadence: Equatable, Sendable {
    case capture(after: UInt32)
    case hold
}
enum ObservationDisposition: UInt32, Sendable {
    case recorded = 1
    case duplicate, notStarted, paused, cloudUnavailable
}
struct ObservationFrameResult: Sendable {
    let frameID: UInt64?
    let highlightVersion: UInt64
    let disposition: ObservationDisposition
    let next: ObservationCadence
}
enum ObservationBegin: Equatable, Sendable {
    case ready(UInt64)
    case notDue, busy, cloudUnavailable
}
struct ObservationRect: Decodable, Equatable, Sendable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double
    var cgRect: CGRect { CGRect(x: x, y: y, width: width, height: height) }
    var valid: Bool {
        [x, y, width, height].allSatisfy { $0.isFinite && (0...1).contains($0) }
            && x + width <= 1 && y + height <= 1
    }
}
struct ObservationHighlight: Decodable, Equatable, Sendable {
    let proposalId: UInt64
    let frameId: UInt64
    let displayKey: String
    let anchor: String
    let rect: ObservationRect
    let needsRecheck: Bool
}
struct ObservationHighlightState: Decodable, Equatable, Sendable {
    let version: UInt64
    let highlight: ObservationHighlight?
}
struct ObservationSend: Decodable, Sendable {
    let attempt: UInt64
    let atUtc: Int64
    let firstFrame: UInt64
    let lastFrame: UInt64
    let apps: [String]
    let images: UInt32
    let droppedFrames: UInt64
    let droppedBytes: UInt64
    let droppedImages: UInt64
    let outcome: ModelCheckOutcome
    let tier: ModelCheckTier
    let addedCards: UInt32
    let rejectedItems: UInt32
}
struct ObservationJournal: Decodable, Sendable {
    let policy: ObservationPolicy
    let logFrames: UInt64
    let unsentFrames: UInt64
    let hourAttempts: UInt64
    let sends: [ObservationSend]
}

// These codes arrive through the journal JSON ABI; unknown numeric codes reject the entire snapshot.
extension ModelCheckOutcome: Decodable {
    init(from decoder: any Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(UInt32.self)
        guard let value = Self(rawValue: raw) else { throw ModelCheckFailure.native }
        self = value
    }
}
extension ModelCheckTier: Decodable {
    init(from decoder: any Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(UInt32.self)
        guard let value = Self(rawValue: raw) else { throw ModelCheckFailure.native }
        self = value
    }
}
