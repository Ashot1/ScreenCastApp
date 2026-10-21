import CoreGraphics
import Foundation

enum ProbeMode: String, Decodable {
    case read
    case search
}

enum ProbeNavigation: String, Decodable {
    case scrollDown
    case expand
}

struct ContentProbeRequest: Decodable {
    let pid: Int32
    let bundleID: String
    let documentURL: String
    let excludedBundleIDs: [String]
    let excludedHosts: [String]
    let excludedFolderPaths: [String]
    let mode: ProbeMode
    let query: String?
    let navigation: ProbeNavigation?
}

struct AXFragment {
    let text: String
    let frame: CGRect?
}

enum ProbeLimitation: String, Hashable {
    case nodeLimit
    case byteLimit
    case depthLimit
    case timeLimit
    case readError
    case unavailableText
    case geometryUnavailable
    case subtreeOnly
}

struct AXSnapshot {
    var fragments: [AXFragment] = []
    var nodesVisited: Int = 0
    var limitations: Set<ProbeLimitation> = [.subtreeOnly]
    var windowFrame: CGRect?
}

enum ContentProbeError: Error {
    case invalidRequest
    case permissionDenied
    case targetChanged
    case sourceUnknown
    case sourceExcluded
    case sourceChanged
    case unsupported
    case ax(Int32)
}

struct NavigationLease {
    let initialInputCounts: [UInt32]
    var remainingSteps: Int = 4
    var stopped: Bool = false
}
