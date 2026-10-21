import ApplicationServices
import CoreGraphics
import Foundation

var failures = 0

@MainActor
func check(_ condition: Bool, _ name: String) {
    if !condition {
        print("FAIL: \(name)")
        failures += 1
    }
}

func request(
    document: String = "https://allowed.example/test",
    excludedHosts: [String] = [], excludedFolders: [String] = [],
    mode: ProbeMode = .read, query: String? = nil, navigation: ProbeNavigation? = nil
) -> ContentProbeRequest {
    return ContentProbeRequest(
        pid: 123, bundleID: "dev.ambient.fixture", documentURL: document,
        excludedBundleIDs: [], excludedHosts: excludedHosts,
        excludedFolderPaths: excludedFolders, mode: mode, query: query, navigation: navigation
    )
}

@MainActor
func expectSourceError(
    _ expected: ContentProbeError, request: ContentProbeRequest, actual: URL?, _ name: String
) {
    do {
        try validateSource(request, actualDocument: actual)
        check(false, name)
    } catch let error as ContentProbeError {
        switch (expected, error) {
        case (.sourceUnknown, .sourceUnknown), (.sourceChanged, .sourceChanged),
            (.sourceExcluded, .sourceExcluded):
            break
        default:
            check(false, name)
        }
    } catch {
        check(false, name)
    }
}

expectSourceError(.sourceUnknown, request: request(), actual: nil, "unknown source blocks reading")
expectSourceError(
    .sourceChanged, request: request(), actual: URL(string: "https://other.example/test"),
    "a changed tab cannot inherit permission"
)
expectSourceError(
    .sourceExcluded,
    request: request(
        document: "https://sub.blocked.example/test", excludedHosts: ["blocked.example"]),
    actual: URL(string: "https://sub.blocked.example/test"), "excluded host includes subdomains"
)
expectSourceError(
    .sourceExcluded,
    request: request(document: "file:///tmp/private/file.txt", excludedFolders: ["/tmp/private"]),
    actual: URL(string: "file:///tmp/private/file.txt"), "excluded folder applies to descendants"
)
expectSourceError(
    .sourceExcluded,
    request: request(
        document: "file:///tmp/open/../private/file.txt", excludedFolders: ["/tmp/private"]),
    actual: URL(string: "file:///tmp/open/../private/file.txt"),
    "dot segments cannot bypass exclusion"
)
do {
    try validateSource(
        request(document: "file:///tmp/private-other/file.txt", excludedFolders: ["/tmp/private"]),
        actualDocument: URL(string: "file:///tmp/private-other/file.txt")
    )
    try validateSource(
        request(document: "https://notblocked.example/test", excludedHosts: ["blocked.example"]),
        actualDocument: URL(string: "https://notblocked.example/test")
    )
} catch {
    check(false, "folder and host exclusions use component boundaries")
}
do {
    try validateRequest(request(mode: .search))
    check(false, "search requires query and explicit navigation")
} catch {}
for invalid in [request(excludedHosts: ["."]), request(document: "file://remote-server/test.txt")] {
    do {
        try validateRequest(invalid)
        check(false, "ambiguous exclusion or remote file identity is rejected")
    } catch {}
}
check(
    failureExitCode(ContentProbeError.permissionDenied) == 2,
    "permission denial has a blocked exit code")
check(
    failureExitCode(ContentProbeError.invalidRequest) == 64,
    "invalid input is not a permission denial")
check(
    failureExitCode(ContentProbeError.ax(-25_204)) == 4,
    "native AX errors keep a distinct failure status")

var evidence = AXSnapshot()
appendEvidence(String(repeating: "🐈", count: 3_000), frame: nil, to: &evidence)
check(
    evidence.fragments.map { $0.text.utf8.count }.reduce(0, +) <= 8_192,
    "retained evidence is bounded by bytes"
)
check(evidence.limitations.contains(.byteLimit), "truncated evidence records byte loss")
check(!evidence.fragments.isEmpty, "available text is retained within the limit")
check(
    !evidence.fragments.contains { $0.text.contains("�") },
    "UTF-8 truncation preserves scalar boundaries"
)
var many = AXSnapshot()
for _ in 0..<100 { appendEvidence("x", frame: nil, to: &many) }
check(many.fragments.count == 64, "retained fragments are bounded by count")
check(many.limitations.contains(.nodeLimit), "fragment overflow is explicit")
check(many.limitations.contains(.subtreeOnly), "AX subtree never means entire document")

let clipped = windowIntersection(
    CGRect(x: -10, y: 20, width: 30, height: 20),
    window: CGRect(x: 0, y: 0, width: 100, height: 100)
)
check(clipped == CGRect(x: 0, y: 20, width: 20, height: 20), "bounds clip to the window")
check(
    windowIntersection(
        CGRect(x: 0, y: 200, width: 10, height: 10),
        window: CGRect(x: 0, y: 0, width: 100, height: 100)
    ) == nil, "offscreen geometry is not reported as visible"
)

var lease = NavigationLease(initialInputCounts: [10, 20])
check(!lease.consumeStep(inputCounts: [11, 20]), "input revokes the next step")
check(!lease.consumeStep(inputCounts: [10, 20]), "stopped search cannot resume on old baseline")
var button = NavigationLease(initialInputCounts: [10])
button.stop()
check(!button.consumeStep(inputCounts: [10]), "Stop button revokes the next step")
var invalidBaseline = NavigationLease(initialInputCounts: [])
check(
    !invalidBaseline.consumeStep(inputCounts: []),
    "missing input evidence cannot authorize navigation")
var bounded = NavigationLease(initialInputCounts: [10])
for _ in 0..<4 {
    check(bounded.consumeStep(inputCounts: [10]), "explicit lease permits bounded steps")
}
check(!bounded.consumeStep(inputCounts: [10]), "fifth navigation step is forbidden")

do {
    _ = try checkedAXElement("not-an-element" as CFString)
    check(false, "wrong CF type is rejected before conversion")
} catch {}
do {
    let native = AXUIElementCreateApplication(123)
    check(CFEqual(try checkedAXElement(native), native), "typed native AX handle is preserved")
} catch { check(false, "native AX handle conversion") }
var point = CGPoint(x: -20, y: 30)
var size = CGSize(width: 200, height: 100)
if let positionValue = AXValueCreate(.cgPoint, &point),
    let sizeValue = AXValueCreate(.cgSize, &size)
{
    check(
        frameFromAX(position: positionValue, size: sizeValue)
            == CGRect(x: -20, y: 30, width: 200, height: 100),
        "real AX geometry retains desktop point coordinates"
    )
    check(
        frameFromAX(position: sizeValue, size: positionValue) == nil,
        "AX geometry type mismatch is rejected")
} else {
    check(false, "native AX geometry fixture creation")
}

var privateSnapshot = AXSnapshot()
privateSnapshot.windowFrame = CGRect(x: 0, y: 0, width: 100, height: 100)
appendEvidence(
    "PRIVATE-SYNTHETIC-CONTENT", frame: CGRect(x: 0, y: 10, width: 20, height: 10),
    to: &privateSnapshot)
let summary = snapshotSummary(privateSnapshot, query: "SYNTHETIC")
check(
    summary.contains("visibility=unknown"), "window intersection must not claim actual visibility")
check(
    !summary.contains(where: { $0.hasPrefix("visibleAXRect=") }),
    "geometry label preserves visibility uncertainty")
check(
    summary.contains("result=found-in-read-fragments"), "match is reported within checked fragments"
)
check(!summary.joined().contains("PRIVATE-SYNTHETIC-CONTENT"), "summary does not persist text")
check(!summary.joined().contains("SYNTHETIC"), "summary does not echo the query")
check(
    snapshotSummary(privateSnapshot, query: "absent").contains(
        "result=not-found-in-read-fragments"),
    "negative result names the checked scope"
)
check(
    snapshotSummary(AXSnapshot(), query: "absent").contains("result=unreadable"),
    "no text is not absence")

if failures == 0 { print("PASS: scope, bounded evidence, geometry and latched navigation policy") }
exit(failures == 0 ? 0 : 1)
