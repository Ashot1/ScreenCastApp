import CoreGraphics
import Foundation

func failureExitCode(_ error: Error) -> Int32 {
    guard let error = error as? ContentProbeError else { return 64 }
    switch error {
    case .permissionDenied: return 2
    case .invalidRequest: return 64
    case .ax: return 4
    default: return 3
    }
}

func validateRequest(_ request: ContentProbeRequest) throws {
    guard request.pid > 0, !request.bundleID.isEmpty, request.bundleID.utf8.count <= 256,
        request.documentURL.utf8.count <= 4_096,
        request.excludedBundleIDs.count <= 32, request.excludedHosts.count <= 32,
        request.excludedFolderPaths.count <= 32,
        request.excludedBundleIDs.allSatisfy({ !$0.isEmpty && $0.utf8.count <= 256 }),
        request.excludedHosts.allSatisfy({ normalizedHost($0) != nil }),
        request.excludedFolderPaths.allSatisfy({ $0.hasPrefix("/") && $0.utf8.count <= 4_096 }),
        let expected = URL(string: request.documentURL),
        ["https", "http", "file"].contains(expected.scheme),
        expected.user == nil, expected.password == nil,
        expected.isFileURL || expected.host != nil,
        !expected.isFileURL || expected.host == nil || expected.host == ""
            || expected.host == "localhost"
    else { throw ContentProbeError.invalidRequest }
    if request.mode == .search {
        guard let query = request.query,
            !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            query.utf8.count <= 256, request.navigation != nil
        else { throw ContentProbeError.invalidRequest }
    }
    if let query = request.query, query.utf8.count > 256 { throw ContentProbeError.invalidRequest }
}

private func normalizedHost(_ value: String) -> String? {
    guard !value.isEmpty, value.utf8.count <= 253,
        !value.contains(where: { $0.isWhitespace || "/:@?#".contains($0) }),
        let url = URL(string: "https://\(value)"), let host = url.host
    else { return nil }
    let normalized = host.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "."))
    return normalized.isEmpty ? nil : normalized
}

func validateSource(_ request: ContentProbeRequest, actualDocument: URL?) throws {
    try validateRequest(request)
    guard !request.excludedBundleIDs.contains(request.bundleID) else {
        throw ContentProbeError.sourceExcluded
    }
    guard let actual = actualDocument, let expected = URL(string: request.documentURL) else {
        throw ContentProbeError.sourceUnknown
    }
    if actual.isFileURL {
        let path = actual.standardizedFileURL.resolvingSymlinksInPath().pathComponents
        for excluded in request.excludedFolderPaths {
            let root = URL(fileURLWithPath: excluded).standardizedFileURL.resolvingSymlinksInPath()
            if path.starts(with: root.pathComponents) { throw ContentProbeError.sourceExcluded }
        }
    } else if let host = actual.host?.lowercased().trimmingCharacters(
        in: CharacterSet(charactersIn: "."))
    {
        for excluded in request.excludedHosts {
            guard let excludedHost = normalizedHost(excluded) else {
                throw ContentProbeError.invalidRequest
            }
            if host == excludedHost || host.hasSuffix(".\(excludedHost)") {
                throw ContentProbeError.sourceExcluded
            }
        }
    } else {
        throw ContentProbeError.sourceUnknown
    }
    let actualIdentity =
        actual.isFileURL ? actual.standardizedFileURL.resolvingSymlinksInPath() : actual
    let expectedIdentity =
        expected.isFileURL ? expected.standardizedFileURL.resolvingSymlinksInPath() : expected
    guard actualIdentity == expectedIdentity else { throw ContentProbeError.sourceChanged }
}

func appendEvidence(_ text: String, frame: CGRect?, to snapshot: inout AXSnapshot) {
    guard snapshot.fragments.count < 64 else {
        snapshot.limitations.insert(.nodeLimit)
        return
    }
    var remaining = 8_192 - snapshot.fragments.reduce(0) { $0 + $1.text.utf8.count }
    var kept = ""
    for scalar in text.unicodeScalars {
        let size = String(scalar).utf8.count
        guard size <= remaining else {
            snapshot.limitations.insert(.byteLimit)
            break
        }
        kept.unicodeScalars.append(scalar)
        remaining -= size
    }
    if !kept.isEmpty { snapshot.fragments.append(AXFragment(text: kept, frame: frame)) }
}

func windowIntersection(_ frame: CGRect?, window: CGRect?) -> CGRect? {
    guard let frame, let window else { return nil }
    let clipped = frame.intersection(window)
    return clipped.isNull || clipped.isEmpty ? nil : clipped
}

func snapshotSummary(_ snapshot: AXSnapshot, query: String?) -> [String] {
    let matches = query.map { query in snapshot.fragments.filter { $0.text.contains(query) } } ?? []
    let result: String
    if snapshot.fragments.isEmpty {
        result = "unreadable"
    } else if query == nil {
        result = "read-AX-subtree"
    } else {
        result = matches.isEmpty ? "not-found-in-read-fragments" : "found-in-read-fragments"
    }
    var lines = [
        "result=\(result)", "coverage=AX-subtree-partial", "visibility=unknown",
        "nodes=\(snapshot.nodesVisited)",
        "retainedBytes=\(snapshot.fragments.reduce(0) { $0 + $1.text.utf8.count })",
        "matches=\(matches.count)",
        "limitations=\(snapshot.limitations.map(\.rawValue).sorted().joined(separator: ","))",
    ]
    for match in matches.prefix(8) {
        if let frame = windowIntersection(match.frame, window: snapshot.windowFrame) {
            lines.append(
                "windowClippedAXRect=\(frame.minX),\(frame.minY),\(frame.width),\(frame.height)")
        } else {
            lines.append("windowClippedAXRect=unavailable-or-outside-window")
        }
    }
    return lines
}

extension NavigationLease {
    mutating func stop() { stopped = true }

    mutating func consumeStep(inputCounts: [UInt32]) -> Bool {
        guard !stopped, !initialInputCounts.isEmpty, inputCounts == initialInputCounts,
            remainingSteps > 0
        else {
            stopped = true
            return false
        }
        remainingSteps -= 1
        return true
    }
}
