import ServiceManagement
import SwiftUI

struct LoginItemView: View {
    let model: LoginItemModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                Text(LocalizedStringKey(model.statusKey), bundle: .module)
                if let failureKey = model.failureKey {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failureKey), bundle: .module))
                }
                switch model.status {
                case .notRegistered:
                    Button {
                        model.enable()
                    } label: {
                        Text("loginItem.enable", bundle: .module)
                    }
                case .enabled:
                    Button {
                        model.disable()
                    } label: {
                        Text("loginItem.disable", bundle: .module)
                    }
                case .requiresApproval:
                    Button {
                        model.openSystemSettings()
                    } label: {
                        Text("loginItem.openSettings", bundle: .module)
                    }
                    Button {
                        model.disable()
                    } label: {
                        Text("loginItem.remove", bundle: .module)
                    }
                default:
                    EmptyView()
                }
                Button {
                    model.refreshByUser()
                } label: {
                    Text("loginItem.refresh", bundle: .module)
                }
            }
            .onAppear { model.refresh() }
            .onChange(of: scenePhase) { _, phase in
                if phase == .active { model.refresh() }
            }
        } header: {
            Text("loginItem.title", bundle: .module)
        } footer: {
            Text("loginItem.help", bundle: .module)
        }

    }
}
