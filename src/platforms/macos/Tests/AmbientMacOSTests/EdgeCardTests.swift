import AppKit
import SwiftUI
import Testing

@testable import AmbientMacOS

@Test @MainActor func edgeGeometryKeepsPointerScreenAndOnlyClampsExistingPosition() {
    let usable = CGRect(x: -1200, y: 40, width: 1200, height: 760)
    let first = EdgeCardController.placement(in: usable, existing: nil)
    #expect(usable.contains(first))
    #expect(first.maxX == -16)
    let moved = CGRect(x: -600, y: 100, width: 420, height: 500)
    #expect(EdgeCardController.placement(in: usable, existing: moved) == moved)
    let smaller = CGRect(x: 0, y: 40, width: 320, height: 400)
    let corrected = EdgeCardController.placement(in: smaller, existing: moved)
    #expect(smaller.contains(corrected))
    #expect(corrected.width == 320 && corrected.height == 400)
}

@Test @MainActor func edgeContentHeightGrowsAndShrinksWithoutMovingTheTopEdge() {
    let usable = CGRect(x: -1200, y: 40, width: 1200, height: 760)
    let moved = CGRect(x: -600, y: 100, width: 420, height: 500)
    let compact = EdgeCardController.placement(in: usable, existing: moved, contentHeight: 120)
    #expect(compact == CGRect(x: -600, y: 480, width: 420, height: 120))
    let expanded = EdgeCardController.placement(in: usable, existing: compact, contentHeight: 460)
    #expect(expanded == CGRect(x: -600, y: 140, width: 420, height: 460))
    #expect(
        EdgeCardController.placement(in: usable, existing: expanded, contentHeight: 120) == compact)
}

@Test @MainActor func edgeContentHeightCapsLongFeedAndShrinksAfterTheCap() {
    let usable = CGRect(x: 0, y: 40, width: 1200, height: 760)
    let compact = CGRect(x: 764, y: 664, width: 420, height: 120)
    let long = EdgeCardController.placement(in: usable, existing: compact, contentHeight: 2400)
    #expect(long == CGRect(x: 764, y: 184, width: 420, height: 600))
    #expect(
        EdgeCardController.placement(in: usable, existing: long, contentHeight: 120) == compact)
}

@Test @MainActor func edgeContentHeightClampsSmallScreenAndKeepsExistingWidth() {
    let usable = CGRect(x: -320, y: 40, width: 320, height: 220)
    let moved = CGRect(x: -600, y: 100, width: 420, height: 500)
    #expect(
        EdgeCardController.placement(in: usable, existing: moved, contentHeight: 900)
            == CGRect(x: -320, y: 40, width: 320, height: 220))
    let wideScreen = CGRect(x: -1200, y: 40, width: 1200, height: 760)
    let narrow = CGRect(x: -600, y: 100, width: 300, height: 500)
    #expect(
        EdgeCardController.placement(in: wideScreen, existing: narrow, contentHeight: 120)
            == CGRect(x: -600, y: 480, width: 300, height: 120))
}

#if AMBIENT_MODEL_TEST_SUPPORT
    @Test @MainActor func edgeRealScrollMeasurementCapsThenShrinksWithoutReplacingTheEditor()
        async throws
    {
        let root = FileManager.default.temporaryDirectory.appending(
            path: "ambient-edge-height-" + UUID().uuidString, directoryHint: .isDirectory)
        defer { try? FileManager.default.removeItem(at: root) }
        try await writeVisualFixtureTemplates(seed: root)
        let memory = MemorySession(databaseURL: root.appending(path: "cases/memory.sqlite3"))
        let server = try AssistanceHTTPServer(mode: "clarification")
        defer { server.stop() }
        let coordinator = ChatGPTCoordinator(
            store: AssistanceTestStore(), memory: memory,
            modelBridge: LocalAssistanceBridge(url: server.url))
        let chat = ChatGPTModel(coordinator: coordinator)
        let model = AssistantModel(session: memory)
        await chat.refreshFeed()
        await model.reload()
        await model.loadManualPause()
        let usable = CGRect(x: 0, y: 0, width: 1200, height: 800)
        let panel = NSPanel(
            contentRect: CGRect(x: 760, y: 184, width: 420, height: 600),
            styleMask: [.titled, .closable, .nonactivatingPanel, .fullSizeContentView],
            backing: .buffered, defer: true)
        panel.isReleasedWhenClosed = false
        panel.titlebarAppearsTransparent = true
        panel.titleVisibility = .hidden
        defer { panel.close() }
        var measurements: [CGFloat] = []
        let host = NSHostingView(
            rootView: AssistanceCardView(
                model: model, chatGPT: chat, edge: true,
                contentHeightChanged: { height in
                    measurements.append(height)
                    panel.setFrame(
                        EdgeCardController.placement(
                            in: usable, existing: panel.frame, contentHeight: height),
                        display: false)
                }))
        panel.contentView = host
        try await settleEdgeLayout(panel, measurements: { measurements })
        let height = try #require(measurements.last)
        #expect(height > 40 && height < 250)
        #expect(panel.frame.height < 250)
        let scroll = try #require(edgeDescendants(host).compactMap { $0 as? NSScrollView }.first)
        #expect(
            try #require(scroll.documentView).frame.height <= scroll.contentView.bounds.height + 1)
        print("D107 compact", height, panel.frame, panel.contentLayoutRect)

        try await coordinator.setCreditReport(true)
        _ = try await coordinator.runCheck(profile: .high)
        for index in 0..<8 {
            let task = try await memory.createTask("Height fixture \(index)")
            _ = try await memory.addNote(to: task, text: "Saved height fixture", at: 1)
            let preview = try await coordinator.prepareAssistance(
                try await memory.task(task.id), notes: [])
            _ = try await coordinator.runAssistance(preview.requestID)
        }
        await chat.reload()
        await chat.reloadAssistanceAvailability()
        try await settleEdgeLayout(panel, measurements: { measurements })
        #expect(chat.assistanceReady)
        #expect(try #require(measurements.last) > 600)
        #expect(panel.frame.height == 600)
        #expect(try #require(scroll.documentView).frame.height > scroll.contentView.bounds.height)
        #expect(edgeDescendants(host).filter { $0 is NSScrollView }.count == 1)
        print("D107 long", measurements.last as Any, panel.frame, scroll.documentView?.frame as Any)
        let document = try #require(scroll.documentView)
        scroll.contentView.scroll(
            to: CGPoint(x: 0, y: document.bounds.maxY - scroll.contentView.bounds.height))
        scroll.reflectScrolledClipView(scroll.contentView)
        try await settleEdgeLayout(panel, measurements: { measurements })
        #expect(scroll.contentView.bounds.minY > 0)
        #expect(panel.frame.height == 600)

        chat.updateReply("Independent unsent sizing draft")
        await chat.waitForReplyPreview()
        try await settleEdgeLayout(panel, measurements: { measurements })
        let editor = try #require(
            edgeDescendants(host).compactMap { $0 as? NSTextView }
                .first { $0.string == "Independent unsent sizing draft" })
        #expect(panel.makeFirstResponder(editor))
        for proposal in try await coordinator.proposalFeed() {
            try await coordinator.setProposalVisibility(proposal.target, visible: false)
        }
        await chat.refreshFeed()
        try await settleEdgeLayout(panel, measurements: { measurements })
        #expect(panel.firstResponder === editor)
        #expect(editor.string == "Independent unsent sizing draft")
        print("D107 editor", measurements.last as Any, panel.frame)

        let smallerScreen = CGRect(x: 0, y: 284, width: 1200, height: 500)
        let editorHeight = try #require(measurements.last)
        panel.setFrame(
            EdgeCardController.placement(
                in: smallerScreen, existing: panel.frame,
                contentHeight: editorHeight), display: false)
        try await settleEdgeLayout(panel, measurements: { measurements })
        #expect(panel.frame.height == 500)
        #expect(panel.firstResponder === editor)
        #expect(editor.string == "Independent unsent sizing draft")
        print("D107 smaller screen with editor", panel.frame)

        await chat.discardReply()
        try await settleEdgeLayout(panel, measurements: { measurements })
        #expect(panel.frame.height < 250)
        #expect(panel.frame.maxY == 784)
        #expect(
            try #require(scroll.documentView).frame.height <= scroll.contentView.bounds.height + 1)
        #expect(panel.contentView === host)
        #expect(!panel.isVisible)
        print("D107 shrunk", measurements.last as Any, panel.frame, "reports", measurements.count)
        try await coordinator.shutdown()
        try await memory.close()
    }

    @MainActor private func settleEdgeLayout(_ panel: NSPanel, measurements: () -> [CGFloat])
        async throws
    {
        var previousFrame = CGRect.null
        var previousHeight: CGFloat?
        var stable = 0
        for _ in 0..<500 {
            panel.contentView?.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(10))
            let height = measurements().last
            stable =
                height != nil && previousFrame == panel.frame && previousHeight == height
                ? stable + 1 : 0
            previousFrame = panel.frame
            previousHeight = height
            if stable == 5 { return }
        }
        Issue.record("The real edge layout did not settle")
    }

    @MainActor private func edgeDescendants(_ view: NSView) -> [NSView] {
        view.subviews.flatMap { [$0] + edgeDescendants($0) }
    }
#endif
