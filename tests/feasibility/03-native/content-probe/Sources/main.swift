import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

guard Bundle.main.bundleIdentifier == "dev.ambient.feasibility.native-content-v2" else {
    print("result=unrecognized-client-bundle")
    exit(64)
}
let arguments = Array(CommandLine.arguments.dropFirst())
let accessibility = AXIsProcessTrusted()
print("accessibility=\(accessibility)")
print("screenCapture=\(CGPreflightScreenCaptureAccess())")
print("listenEvents=\(CGPreflightListenEventAccess())")
if arguments.isEmpty || arguments == ["--preflight"] {
    print("mode=preflight")
    print("exitCode=\(accessibility ? 0 : 2)")
    exit(accessibility ? 0 : 2)
}
guard arguments.count == 2, arguments[0] == "--request" else {
    print("result=invalid-arguments")
    exit(64)
}
do {
    let file = try FileHandle(forReadingFrom: URL(fileURLWithPath: arguments[1]))
    let data: Data?
    do { data = try file.read(upToCount: 65_537) } catch {
        try file.close()
        throw error
    }
    try file.close()
    guard let data, data.count <= 65_536 else {
        throw ContentProbeError.invalidRequest
    }
    let request = try JSONDecoder().decode(ContentProbeRequest.self, from: data)
    try validateRequest(request)
    guard accessibility else { throw ContentProbeError.permissionDenied }
    if request.mode == .search && !CGPreflightListenEventAccess() {
        throw ContentProbeError.permissionDenied
    }
    let target = try bindTarget(request)
    if request.mode == .read {
        let read = try readAXSnapshot(request, window: target.window, launchDate: target.launchDate)
        for line in snapshotSummary(read.snapshot, query: request.query) { print(line) }
        print("exitCode=\(read.snapshot.fragments.isEmpty ? 3 : 0)")
        exit(read.snapshot.fragments.isEmpty ? 3 : 0)
    }
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    let controller = SearchController(
        request: request, window: target.window, launchDate: target.launchDate)
    controller.start()
    app.run()
} catch {
    print("result=\(safeFailure(error))")
    let code = failureExitCode(error)
    print("exitCode=\(code)")
    exit(code)
}
