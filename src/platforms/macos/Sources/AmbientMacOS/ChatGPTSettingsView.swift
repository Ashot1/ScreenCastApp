import SwiftUI

/// Settings section «ChatGPT»: the sign-in block with its recovery confirmation.
struct ChatGPTSettingsView: View {
    let chatGPT: ChatGPTModel
    @State private var confirmRecovery = false
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text(LocalizedStringKey(chatGPT.state.key), bundle: .module).font(.headline)
                Text("auth.help", bundle: .module)
                if let failure = chatGPT.failure {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure.key), bundle: .module))
                }
                if chatGPT.state == .unknown {
                    // ADR-023 #10: the previous unknown-outcome block; its button still asks
                    // for the system confirmation below.
                    Callout(
                        tone: Theme.stale, systemImage: "questionmark.circle",
                        message: Text("auth.recovery", bundle: .module)
                    ) {
                        Button {
                            confirmRecovery = true
                        } label: {
                            Text("auth.recover", bundle: .module)
                        }
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
