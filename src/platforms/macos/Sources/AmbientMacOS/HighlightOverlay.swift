import AppKit
import ColorSync

/// Draws only the core's latest binding; tracking and staleness remain with observation.
@MainActor public final class HighlightOverlay {
    private var panel: NSPanel?

    public init() {}

    isolated deinit {
        panel?.close()
    }

    public func show(normalizedRect: CGRect, displayKey: String) {
        let displays = NSScreen.screens.compactMap { screen -> (key: String, frame: CGRect)? in
            guard
                let number = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")]
                    as? NSNumber,
                let uuid = CGDisplayCreateUUIDFromDisplayID(number.uint32Value)?.takeRetainedValue()
            else { return nil }
            return (CFUUIDCreateString(nil, uuid) as String, screen.frame)
        }
        guard let display = Self.displayFrame(displayKey: displayKey, displays: displays) else {
            hide()
            return
        }
        let window = panel ?? Self.makePanel()
        panel = window
        window.setFrame(
            Self.screenRect(normalizedRect: normalizedRect, displayFrame: display), display: true)
        window.orderFrontRegardless()
    }

    public func hide() {
        panel?.orderOut(nil)
    }

    nonisolated static func screenRect(normalizedRect: CGRect, displayFrame: CGRect) -> CGRect {
        // Vision and AppKit both start at bottom-left. NSScreen is in points, including Retina.
        CGRect(
            x: displayFrame.minX + normalizedRect.minX * displayFrame.width,
            y: displayFrame.minY + normalizedRect.minY * displayFrame.height,
            width: normalizedRect.width * displayFrame.width,
            height: normalizedRect.height * displayFrame.height)
    }

    nonisolated static func displayFrame(
        displayKey: String, displays: [(key: String, frame: CGRect)]
    ) -> CGRect? {
        displays.first { $0.key == displayKey }?.frame
    }

    static func makePanel() -> NSPanel {
        let window = HighlightPanel(
            contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = false
        window.hidesOnDeactivate = false
        window.ignoresMouseEvents = true
        window.level = .floating
        window.collectionBehavior = [
            .canJoinAllSpaces, .fullScreenAuxiliary, .canJoinAllApplications,
        ]
        window.animationBehavior = .none
        window.contentView = HighlightFrameView()
        return window
    }
}

private final class HighlightPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

private final class HighlightFrameView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        NSColor.labelColor.setStroke()
        let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 4, yRadius: 4)
        path.lineWidth = 2
        path.stroke()
    }
}
