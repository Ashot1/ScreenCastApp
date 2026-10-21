import AppKit
import SwiftUI

extension AssistantModel {
    var statusKey: String {
        switch statusKind {
        case .closing: "status.closing"
        case .paused: "status.paused"
        case .assistance: "status.stoppedAnalysis"
        case .unknown: "status.pauseUnknown"
        }
    }
    /// The state behind `statusKey`: drawn as the ring in windows and as a symbol in the menu bar.
    var statusKind: StatusRing.Kind {
        if closing { return .closing }
        switch manualPause {
        case true: return .paused
        case false: return .assistance
        case nil: return .unknown
        }
    }
    /// The menu bar's monochrome symbol for `statusKind` (D75: no colored ring there).
    var statusSymbol: String {
        switch statusKind {
        case .assistance: "circle"
        case .paused: "pause.circle"
        case .unknown: "questionmark.circle"
        case .closing: "hourglass"
        }
    }
}

public struct AssistantMenuBarLabel: View {
    private let model: AssistantModel
    public init(model: AssistantModel) { self.model = model }
    public var body: some View {
        // ADR-023 R4: a `Label` here shows only the symbol; the HStack keeps the «Ambient» title.
        HStack {
            Image(systemName: model.statusSymbol)
            Text("app.name", bundle: .module)
        }
    }
}

public struct AssistantMenu: View {
    private let model: AssistantModel
    @Environment(\.openWindow) private var openWindow
    public init(model: AssistantModel) { self.model = model }
    public var body: some View {
        Group {
            Label {
                Text(LocalizedStringKey(model.statusKey), bundle: .module)
            } icon: {
                Image(systemName: model.statusSymbol)
            }
            AssistantPauseButton(model: model)
            Button {
                Task { await model.refreshManualPause() }
            } label: {
                Label {
                    Text("pause.read", bundle: .module)
                } icon: {
                    Image(systemName: "arrow.clockwise")
                }
            }
            .disabled(model.busy || model.closing)
            if let failure = model.failure {
                Label(failure.message, systemImage: "exclamationmark.triangle")
            }
            Divider()
            Button {
                openWindow(id: "memory")
            } label: {
                Label {
                    Text("menu.openMemory", bundle: .module)
                } icon: {
                    Image(systemName: "macwindow")
                }
            }
            SettingsLink {
                Label {
                    Text("settings.title", bundle: .module)
                } icon: {
                    Image(systemName: "gearshape")
                }
            }
            Divider()
            Button {
                NSApplication.shared.terminate(nil)
            } label: {
                Label {
                    Text("menu.quit", bundle: .module)
                } icon: {
                    Image(systemName: "power")
                }
            }
            .keyboardShortcut("q")
        }
        // ADR-023 R4: item icons show on macOS 27 with the explicit `.menu` style and this label style.
        .labelStyle(.titleAndIcon)
    }
}

struct AssistantPauseButton: View {
    let model: AssistantModel
    var body: some View {
        Button {
            Task { await model.toggleManualPause() }
        } label: {
            Label {
                Text(
                    model.manualPause == true ? "pause.remove" : "pause.enable", bundle: .module)
            } icon: {
                Image(systemName: model.manualPause == true ? "play.circle" : "pause.circle")
            }
        }
        .disabled(!model.canToggleManualPause)
    }
}
