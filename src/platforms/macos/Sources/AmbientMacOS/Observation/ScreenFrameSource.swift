import AppKit
import ColorSync
import CoreGraphics
import ScreenCaptureKit

protocol FrameSource: Sendable {
    func requestPermission() async -> Bool
    func hasPermission() async -> Bool
    func idleSeconds() async throws -> UInt32
    func warmUp() async throws
    func capture() async throws -> ObservationFrame?
}

actor ScreenFrameSource: FrameSource {
    func requestPermission() async -> Bool {
        await MainActor.run { CGRequestScreenCaptureAccess() }
    }
    func hasPermission() -> Bool { CGPreflightScreenCaptureAccess() }
    func idleSeconds() throws -> UInt32 {
        guard let anyInput = CGEventType(rawValue: UInt32.max) else {
            throw ModelCheckFailure.native
        }
        let seconds = CGEventSource.secondsSinceLastEventType(
            .combinedSessionState, eventType: anyInput)
        return UInt32(min(Double(UInt32.max), max(0, seconds)))
    }
    func warmUp() async throws { try await ObservationRecognition.warmUp() }
    func capture() async throws -> ObservationFrame? {
        try Task.checkCancellation()
        guard CGPreflightScreenCaptureAccess() else { return nil }
        let ownPID = ProcessInfo.processInfo.processIdentifier
        let frontPID = await MainActor.run {
            NSWorkspace.shared.frontmostApplication?.processIdentifier
        }
        guard let frontPID, frontPID != ownPID else { return nil }
        let content = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: true)
        try Task.checkCancellation()
        // Never capture without the exclusion filter, even if the own application is absent temporarily.
        guard let own = content.applications.first(where: { $0.processID == ownPID }),
            let ordered = CGWindowListCopyWindowInfo(
                [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
                as? [[String: Any]],
            let window = ordered.compactMap({ row -> SCWindow? in
                guard let pid = row[kCGWindowOwnerPID as String] as? Int32, pid == frontPID,
                    let id = row[kCGWindowNumber as String] as? UInt32
                else { return nil }
                return content.windows.first {
                    $0.windowID == id && $0.isOnScreen && $0.frame.width > 0 && $0.frame.height > 0
                }
            }).first, let app = window.owningApplication,
            let index = Self.displayIndex(
                window: window.frame, displays: content.displays.map(\.frame))
        else { return nil }
        let display = content.displays[index]
        guard let uuid = CGDisplayCreateUUIDFromDisplayID(display.displayID)?.takeRetainedValue()
        else { return nil }
        let key = CFUUIDCreateString(nil, uuid) as String
        let filter = SCContentFilter(
            display: display, excludingApplications: [own], exceptingWindows: [])
        let configuration = SCStreamConfiguration()
        configuration.width = CGDisplayPixelsWide(display.displayID)
        configuration.height = CGDisplayPixelsHigh(display.displayID)
        configuration.showsCursor = false
        guard CGPreflightScreenCaptureAccess(), !Task.isCancelled else { return nil }
        let image = try await SCScreenshotManager.captureImage(
            contentFilter: filter, configuration: configuration)
        try Task.checkCancellation()
        guard
            await MainActor.run(body: { NSWorkspace.shared.frontmostApplication?.processIdentifier }
            ) == frontPID
        else { return nil }
        let lines = try await ObservationRecognition.recognize(image)
        try Task.checkCancellation()
        return ObservationFrame(
            displayKey: key, bundleID: app.bundleIdentifier, appName: app.applicationName,
            title: window.title ?? "", lines: lines, jpeg: try ObservationRecognition.jpeg(image))
    }
    nonisolated static func displayIndex(window: CGRect, displays: [CGRect]) -> Int? {
        let areas = displays.map { rect -> CGFloat in
            let intersection = rect.intersection(window)
            return intersection.isNull ? 0 : intersection.width * intersection.height
        }
        guard let index = areas.indices.max(by: { areas[$0] < areas[$1] }), areas[index] > 0 else {
            return nil
        }
        return index
    }
}
