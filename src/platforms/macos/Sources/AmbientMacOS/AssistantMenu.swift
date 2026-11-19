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
    /// The saved pause/closing state drawn in windows.
    var statusKind: StatusRing.Kind {
        if closing { return .closing }
        switch manualPause {
        case true: return .paused
        case false: return .assistance
        case nil: return .unknown
        }
    }
    func menuStatusKey(_ observation: ObservationDisplay) -> String {
        statusKind == .assistance ? observation.state.key : statusKey
    }
    func menuStatusSymbol(_ observation: ObservationDisplay) -> String {
        statusKind == .assistance ? observation.state.symbol : statusSymbol
    }
    /// The menu bar's monochrome symbol for pause/closing (D75: no colored ring there).
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
    private let observation: ObservationDisplay
    public init(model: AssistantModel, observation: ObservationSession) {
        self.model = model
        self.observation = observation.display
    }
    public var body: some View {
        // ADR-023 R4: a `Label` here shows only the symbol; the HStack keeps the «Ambient» title.
        HStack {
            Image(systemName: model.menuStatusSymbol(observation))
            Text("app.name", bundle: .module)
        }
    }
}

public struct AssistantMenu: View {
    private let model: AssistantModel
    private let observation: ObservationDisplay
    @Environment(\.openWindow) private var openWindow
    public init(model: AssistantModel, observation: ObservationSession) {
        self.model = model
        self.observation = observation.display
    }
    public var body: some View {
        Group {
            Label {
                Text(LocalizedStringKey(model.menuStatusKey(observation)), bundle: .module)
            } icon: {
                Image(systemName: model.menuStatusSymbol(observation))
            }
            // A menu item has no native subtitle; the detail is its own disabled line.
            if model.closing {
                Text("status.closingDetail", bundle: .module)
            }
            Divider()
            AssistantPauseButton(model: model)
            // As in the windows, reading is offered only while the saved pause is unknown.
            if model.manualPause == nil { AssistantPauseReadButton(model: model) }
            if let failure = model.failure {
                Label(failure.message, systemImage: "exclamationmark.circle")
            }
            Divider()
            Button {
                openWindow(id: "memory")
            } label: {
                Label {
                    Text("menu.openMemory", bundle: .module)
                } icon: {
                    Image(systemName: "doc")
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
                Text("menu.quit", bundle: .module)
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
                Text(pauseKey, bundle: .module)
            } icon: {
                Image(systemName: model.manualPause == true ? "play" : "pause")
            }
        }
        .help(Text(pauseKey, bundle: .module))
        .disabled(!model.canToggleManualPause)
    }
    private var pauseKey: LocalizedStringKey {
        model.manualPause == true ? "pause.remove" : "pause.enable"
    }
}

/// Reads the saved pause again; callers offer it only while the pause is unknown.
struct AssistantPauseReadButton: View {
    let model: AssistantModel
    var body: some View {
        Button {
            Task { await model.refreshManualPause() }
        } label: {
            Label {
                Text("pause.read", bundle: .module)
            } icon: {
                Image(systemName: "arrow.clockwise")
            }
        }
        .help(Text("pause.read", bundle: .module))
        .disabled(model.busy || model.closing)
    }
}
