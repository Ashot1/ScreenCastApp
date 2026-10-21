import SwiftUI

public struct AssistantSettingsView: View {
    private let model: AssistantModel
    private let chatGPT: ChatGPTModel
    @State private var confirmRecovery = false
    @Environment(\.openWindow) private var openWindow

    public init(model: AssistantModel, chatGPT: ChatGPTModel) {
        self.model = model
        self.chatGPT = chatGPT
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text("settings.title", bundle: .module).font(.title2.bold())
                Text("settings.cloud", bundle: .module).font(.headline)
                Text("settings.description", bundle: .module)
                AssistantStatusView(model: model)
                if let failure = model.failure { Text(failure.message).foregroundStyle(.red) }
                LocalPermissionsView()
                LoginItemView()
                VoiceSettingsView(voice: chatGPT.voice)
                GroupBox {
                    VStack(alignment: .leading, spacing: 10) {
                        Text(LocalizedStringKey(chatGPT.state.key), bundle: .module).font(.headline)
                        Text("auth.help", bundle: .module)
                        if let failure = chatGPT.failure {
                            Text(LocalizedStringKey(failure.key), bundle: .module).foregroundStyle(
                                .red)
                        }
                        if chatGPT.state == .unknown {
                            Text("auth.recovery", bundle: .module)
                            Button {
                                confirmRecovery = true
                            } label: {
                                Text("auth.recover", bundle: .module)
                            }
                        } else if chatGPT.busy {
                            Button {
                                Task { await chatGPT.cancel() }
                            } label: {
                                Text("auth.cancel", bundle: .module)
                            }
                            .disabled(chatGPT.state == .cancelling)
                        } else {
                            HStack {
                                Button {
                                    Task { await chatGPT.connect() }
                                } label: {
                                    Text("auth.continue", bundle: .module)
                                }
                                if chatGPT.state == .saved || chatGPT.state == .expired {
                                    Button {
                                        Task { await chatGPT.connect(refresh: true) }
                                    } label: {
                                        Text("auth.refresh", bundle: .module)
                                    }
                                }
                            }
                        }
                        Button {
                            Task { await chatGPT.signOut() }
                        } label: {
                            Text("auth.signOut", bundle: .module)
                        }
                        Text("auth.localHelp", bundle: .module).font(.caption)
                    }.frame(maxWidth: .infinity, alignment: .leading)
                } label: {
                    Text("auth.title", bundle: .module)
                }
                IntegrationSwitchesView(model: model, chatGPT: chatGPT)
                PluginsSettingsView(plugins: chatGPT.plugins, chatGPT: chatGPT)
                GitHubSettingsView(source: model.githubSources, chatGPT: chatGPT)
                JiraSettingsView(source: model.jiraSources, chatGPT: chatGPT)
                ConnectionCheckView(model: model, chatGPT: chatGPT)
                Text("reply.temporary", bundle: .module).font(.caption).foregroundStyle(.secondary)
                Button {
                    openWindow(id: "memory")
                } label: {
                    Text("memory.manage", bundle: .module)
                }
            }
            .padding(24)
        }
        .frame(width: 480, height: 680)
        .task {
            await chatGPT.reload()
            await chatGPT.reloadChecks()
        }
        .confirmationDialog(
            Text("auth.confirmRecovery", bundle: .module), isPresented: $confirmRecovery
        ) {
            Button {
                Task { await chatGPT.recover() }
            } label: {
                Text("auth.recover", bundle: .module)
            }
            Button(role: .cancel) {
            } label: {
                Text("auth.dismiss", bundle: .module)
            }
        } message: {
            Text("auth.recovery", bundle: .module)
        }
    }
}
