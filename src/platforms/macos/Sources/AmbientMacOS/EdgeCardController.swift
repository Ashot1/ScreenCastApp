import AppKit
import Observation
import SwiftUI

/// Owns window visibility and geometry only; result, draft and dispatch stay with their existing owners.
@MainActor public final class EdgeCardController: NSObject, NSWindowDelegate {
    private let model: AssistantModel
    private let chatGPT: ChatGPTModel
    private var panel: NSPanel?
    private var running = false
    private var displayed = false
    public init(model: AssistantModel, chatGPT: ChatGPTModel) {
        self.model = model
        self.chatGPT = chatGPT
        super.init()
    }
    public func start() {
        guard !running else { return }
        running = true
        NotificationCenter.default.addObserver(
            self, selector: #selector(screensChanged),
            name: NSApplication.didChangeScreenParametersNotification, object: nil)
        observe()
        observeMemory()
    }
    public func stop() {
        running = false
        NotificationCenter.default.removeObserver(self)
        panel?.orderOut(nil)
        panel = nil
    }
    private func observe() {
        guard running else { return }
        withObservationTracking {
            render()
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in self?.observe() }
        }
    }
    private func observeMemory() {
        guard running else { return }
        withObservationTracking {
            _ = model.snapshot
            _ = model.manualPause
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                guard let self, self.running else { return }
                // Re-arm before suspension so a later memory change is not lost.
                self.observeMemory()
                if self.model.manualPause != false {
                    await self.chatGPT.resetAssistance()
                } else {
                    await self.chatGPT.refreshCurrentResult()
                }
            }
        }
    }
    private func render() {
        if model.closing {
            panel?.orderOut(nil)
            return
        }
        guard chatGPT.edgeVisible else {
            panel?.orderOut(nil)
            return
        }
        if panel == nil {
            let window = NSPanel(
                contentRect: .zero,
                styleMask: [.titled, .closable, .nonactivatingPanel, .fullSizeContentView],
                backing: .buffered, defer: true)
            window.title = String(localized: "assistance.title", bundle: .module)
            // Still titled: a borderless panel cannot become key, and the reply editor must type.
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            window.isOpaque = false
            window.backgroundColor = .clear
            window.isReleasedWhenClosed = false
            window.hidesOnDeactivate = false
            window.isFloatingPanel = true
            window.becomesKeyOnlyIfNeeded = true
            window.level = .floating
            window.collectionBehavior = [
                .canJoinAllSpaces, .fullScreenAuxiliary, .canJoinAllApplications,
            ]
            window.delegate = self
            window.contentView = NSHostingView(
                rootView: AssistanceCardView(model: model, chatGPT: chatGPT, edge: true)
                    .background {
                        // The glass fills the whole panel, title bar included (25A R2): a clear
                        // area would still take clicks, so no part of the panel stays empty.
                        Color.clear.glassEffect(in: .rect(cornerRadius: 30)).ignoresSafeArea()
                    }
                    .ambientTheme())
            panel = window
        }
        guard let panel else { return }
        if !displayed {
            let point = NSEvent.mouseLocation
            if let screen = NSScreen.screens.first(where: { $0.frame.contains(point) })
                ?? NSScreen.screens.first
            {
                panel.setFrame(
                    Self.placement(in: screen.visibleFrame, existing: nil), display: false)
            }
            displayed = true
        }
        // Never activate the app, makeKey, or install global input monitors on arrival.
        if !panel.isVisible { panel.orderFrontRegardless() }
    }
    public func windowShouldClose(_ sender: NSWindow) -> Bool {
        Task { await chatGPT.hideEdge() }
        sender.orderOut(nil)
        return false
    }
    @objc private func screensChanged() {
        guard let panel, displayed else { return }
        let screen = NSScreen.screens.max { a, b in
            let x = a.visibleFrame.intersection(panel.frame)
            let y = b.visibleFrame.intersection(panel.frame)
            return (x.isNull ? 0 : x.width * x.height) < (y.isNull ? 0 : y.width * y.height)
        }
        if let screen {
            panel.setFrame(
                Self.placement(in: screen.visibleFrame, existing: panel.frame),
                display: panel.isVisible)
        }
    }
    static func placement(in usable: CGRect, existing: CGRect?) -> CGRect {
        let width = min(existing?.width ?? 420, usable.width)
        let height = min(existing?.height ?? 600, usable.height)
        let x = existing?.minX ?? usable.maxX - width - 16
        let y = existing?.minY ?? usable.maxY - height - 16
        return CGRect(
            x: min(max(x, usable.minX), usable.maxX - width),
            y: min(max(y, usable.minY), usable.maxY - height), width: width, height: height)
    }
}
