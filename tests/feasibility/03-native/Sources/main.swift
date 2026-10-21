import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

@MainActor
func inspectWindowCount(pid: Int32, expectedBundleID: String) -> Int32 {
    guard let target = NSRunningApplication(processIdentifier: pid),
        !target.isTerminated,
        target.bundleIdentifier == expectedBundleID
    else {
        print("result=target-unavailable-or-mismatch")
        return 3
    }

    // Permission can be revoked after preflight; no AX request is sent while untrusted.
    guard AXIsProcessTrusted() else {
        print("result=blocked-accessibility")
        return 2
    }

    let application = AXUIElementCreateApplication(pid)
    let timeoutStatus = AXUIElementSetMessagingTimeout(application, 1)
    guard timeoutStatus == .success else {
        print("axError=\(timeoutStatus.rawValue)")
        return 4
    }

    // Count windows without fetching their titles, text, URLs, images or child elements.
    // This timeout bounds waiting; it does not claim to cancel work in the target app.
    var count: CFIndex = 0
    let status = AXUIElementGetAttributeValueCount(
        application, kAXWindowsAttribute as CFString, &count
    )
    guard status == .success, count >= 0 else {
        print("axError=\(status.rawValue)")
        return 4
    }

    print("windowCount=\(count)")
    print("result=metadata-only")
    return 0
}

guard Bundle.main.bundleIdentifier == "dev.ambient.feasibility.native-content" else {
    print("result=unrecognized-client-bundle")
    print("exitCode=64")
    exit(64)
}

let result = runProbe(
    arguments: Array(CommandLine.arguments.dropFirst()),
    accessibility: AXIsProcessTrusted(),
    screenCapture: CGPreflightScreenCaptureAccess(),
    listenEvents: CGPreflightListenEventAccess(),
    windowProbe: inspectWindowCount,
    emit: { print($0) }
)
print("exitCode=\(result)")
exit(result)
