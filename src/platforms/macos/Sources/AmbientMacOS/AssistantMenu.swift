import AppKit
import SwiftUI

extension AssistantModel {
    var statusKey: String {
        switch manualPause {
        case true: "status.paused"
        case false: "status.stoppedAnalysis"
        case nil: "status.pauseUnknown"
        }
    }
}

public struct AssistantMenuBarLabel: View {
    private let model: AssistantModel
    public init(model: AssistantModel) { self.model = model }
    public var body: some View {
        HStack {
            Text("app.name", bundle: .module)
            Text(LocalizedStringKey(model.statusKey), bundle: .module)
        }
        .accessibilityElement(children: .combine)
    }
}

public struct AssistantMenu: View {
    private let model: AssistantModel
    @Environment(\.openWindow) private var openWindow
    public init(model: AssistantModel) { self.model = model }
    public var body: some View {
        Text(LocalizedStringKey(model.statusKey), bundle: .module)
        AssistantPauseButton(model: model)
        Button {
            Task { await model.refreshManualPause() }
        } label: {
            Text("pause.read", bundle: .module)
        }
        .disabled(model.busy || model.closing)
        if let failure = model.failure { Text(failure.message) }
        Divider()
        Button {
            openWindow(id: "memory")
        } label: {
            Text("menu.openMemory", bundle: .module)
        }
        SettingsLink { Text("settings.title", bundle: .module) }
        Divider()
        Button {
            NSApplication.shared.terminate(nil)
        } label: {
            Text("menu.quit", bundle: .module)
        }
        .keyboardShortcut("q")
    }
}

struct AssistantPauseButton: View {
    let model: AssistantModel
    var body: some View {
        Button {
            Task { await model.toggleManualPause() }
        } label: {
            Text(
                model.manualPause == true ? "pause.remove" : "pause.enable", bundle: .module)
        }
        .disabled(!model.canToggleManualPause)
    }
}
