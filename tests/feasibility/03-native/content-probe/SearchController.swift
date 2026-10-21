import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

func hardwareInputCounts() -> [UInt32] {
    let types: [CGEventType] = [
        .keyDown, .keyUp, .flagsChanged, .mouseMoved, .leftMouseDown, .leftMouseUp,
        .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp,
        .leftMouseDragged, .rightMouseDragged, .otherMouseDragged, .scrollWheel,
        .tabletPointer, .tabletProximity,
    ]
    return types.map { CGEventSource.counterForEventType(.hidSystemState, eventType: $0) }
}

func safeFailure(_ error: Error) -> String {
    guard let error = error as? ContentProbeError else { return "request-unreadable" }
    switch error {
    case .invalidRequest: return "invalid-request"
    case .permissionDenied: return "permission-denied"
    case .targetChanged: return "target-changed"
    case .sourceUnknown: return "source-unknown"
    case .sourceExcluded: return "source-excluded"
    case .sourceChanged: return "source-changed"
    case .unsupported: return "unsupported-AX-capability"
    case .ax(let code): return "AX-error-\(code)"
    }
}

@MainActor
final class SearchController: NSObject {
    private let request: ContentProbeRequest
    private let window: AXUIElement
    private let launchDate: Date
    private var lease: NavigationLease
    private var panel: NSPanel?

    init(request: ContentProbeRequest, window: AXUIElement, launchDate: Date) {
        self.request = request
        self.window = window
        self.launchDate = launchDate
        lease = NavigationLease(initialInputCounts: hardwareInputCounts())
        super.init()
    }

    func start() {
        let panel = NSPanel(
            contentRect: NSRect(x: 40, y: 80, width: 180, height: 60),
            styleMask: [.titled, .nonactivatingPanel], backing: .buffered, defer: false
        )
        panel.title = "Native content test"
        panel.level = .floating
        let button = NSButton(title: "Stop search", target: self, action: #selector(stopSearch))
        button.frame = NSRect(x: 20, y: 15, width: 140, height: 30)
        button.keyEquivalent = "\u{1b}"
        panel.contentView?.addSubview(button)
        panel.orderFrontRegardless()
        self.panel = panel
        perform(#selector(advance), with: nil, afterDelay: 0.3)
    }

    @objc private func stopSearch() {
        lease.stop()
        finish("stop-button", code: 2)
    }

    @objc private func advance() {
        guard !lease.stopped, AXIsProcessTrusted(), CGPreflightListenEventAccess(),
            hardwareInputCounts() == lease.initialInputCounts
        else { finish("stopped-input-or-permission", code: 2) }
        do {
            let read = try readAXSnapshot(request, window: window, launchDate: launchDate)
            guard hardwareInputCounts() == lease.initialInputCounts else {
                finish("stopped-input", code: 2)
            }
            for line in snapshotSummary(read.snapshot, query: request.query) { print(line) }
            if let query = request.query,
                read.snapshot.fragments.contains(where: { $0.text.contains(query) })
            {
                finish("found-in-read-fragments", code: 0)
            }
            guard let candidate = read.navigationTarget else {
                finish("unsupported-navigation", code: 3)
            }
            let performed = try navigateAX(
                request, node: candidate, window: window, launchDate: launchDate,
                expectedWindowFrame: read.snapshot.windowFrame,
                authorize: { self.lease.consumeStep(inputCounts: hardwareInputCounts()) }
            )
            guard performed, hardwareInputCounts() == lease.initialInputCounts else {
                finish("stopped-input-or-step-limit", code: 2)
            }
            // A fresh run-loop turn lets Stop execute before scheduling another bounded search step.
            perform(#selector(advance), with: nil, afterDelay: 0.3)
        } catch { finish(safeFailure(error), code: failureExitCode(error)) }
    }

    private func finish(_ reason: String, code: Int32) -> Never {
        lease.stop()
        NSObject.cancelPreviousPerformRequests(withTarget: self)
        panel?.close()
        print("searchEnd=\(reason)")
        print("exitCode=\(code)")
        exit(code)
    }
}
