import AppKit
import Testing

@testable import AmbientMacOS

@Test func highlightGeometryUsesDisplayPointsIncludingRetinaAndNegativeOrigins() {
    let rect = CGRect(x: 0.25, y: 0.125, width: 0.5, height: 0.25)
    let displays: [(CGRect, CGRect)] = [
        (
            CGRect(x: 0, y: 0, width: 1440, height: 900),
            CGRect(x: 360, y: 112.5, width: 720, height: 225)
        ),
        (
            CGRect(x: -1920, y: 0, width: 1920, height: 1080),
            CGRect(x: -1440, y: 135, width: 960, height: 270)
        ),
        (
            CGRect(x: 0, y: 900, width: 1600, height: 1200),
            CGRect(x: 400, y: 1050, width: 800, height: 300)
        ),
        (
            CGRect(x: 200, y: -1200, width: 1600, height: 1200),
            CGRect(x: 600, y: -1050, width: 800, height: 300)
        ),
    ]
    for (display, expected) in displays {
        #expect(
            HighlightOverlay.screenRect(normalizedRect: rect, displayFrame: display) == expected)
    }
}

@Test func highlightGeometryKeepsVisionBottomLeftOrientation() {
    let display = CGRect(x: -500, y: 900, width: 1000, height: 800)
    #expect(
        HighlightOverlay.screenRect(
            normalizedRect: CGRect(x: 0, y: 0, width: 1, height: 0.125), displayFrame: display)
            == CGRect(x: -500, y: 900, width: 1000, height: 100))
    #expect(
        HighlightOverlay.screenRect(
            normalizedRect: CGRect(x: 0, y: 0.875, width: 1, height: 0.125), displayFrame: display)
            == CGRect(x: -500, y: 1600, width: 1000, height: 100))
}

@Test func highlightFindsDisplayByUUIDAndRejectsUnknownDisplay() {
    let first = "8E15644A-F853-4471-9F0E-2EB87A518000"
    let second = "8E15644A-F853-4471-9F0E-2EB87A518001"
    let displays: [(key: String, frame: CGRect)] = [
        (first, CGRect(x: 0, y: 0, width: 1440, height: 900)),
        (second, CGRect(x: -1920, y: 0, width: 1920, height: 1080)),
    ]
    #expect(
        HighlightOverlay.displayFrame(displayKey: second, displays: displays)
            == CGRect(x: -1920, y: 0, width: 1920, height: 1080))
    #expect(
        HighlightOverlay.displayFrame(displayKey: first, displays: displays)
            == CGRect(x: 0, y: 0, width: 1440, height: 900))
    #expect(HighlightOverlay.displayFrame(displayKey: "unknown-display", displays: displays) == nil)
    #expect(HighlightOverlay.displayFrame(displayKey: first, displays: []) == nil)
}

@Test @MainActor func highlightPanelIsPassiveAndConfiguredForAllSpaces() {
    _ = NSApplication.shared
    let panel = HighlightOverlay.makePanel()
    defer { panel.close() }
    #expect(!panel.isVisible)
    #expect(panel.styleMask.contains(.nonactivatingPanel))
    #expect(!panel.styleMask.contains(.titled))
    #expect(panel.ignoresMouseEvents)
    #expect(!panel.canBecomeKey && !panel.canBecomeMain)
    #expect(!panel.isKeyWindow && !panel.isMainWindow)
    #expect(
        panel.collectionBehavior.contains([
            .canJoinAllSpaces, .fullScreenAuxiliary, .canJoinAllApplications,
        ]))
    #expect(panel.level > .normal)
    #expect(!panel.isOpaque && !panel.hasShadow)
    #expect(panel.backgroundColor == .clear)
    #expect(panel.contentView != nil)
    #expect(!panel.hidesOnDeactivate)
}
