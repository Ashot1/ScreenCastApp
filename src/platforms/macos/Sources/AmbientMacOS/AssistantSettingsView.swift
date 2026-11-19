import SwiftUI

public struct AssistantSettingsView: View {
    private let model: AssistantModel
    private let chatGPT: ChatGPTModel
    private let observation: ObservationSession
    @State private var section = SettingsSection.general
    // Owned here, not by the General section: a section switch must not reset a permission
    // check or hide a launch-at-login failure.
    @State private var permissions = LocalPermissions()
    @State private var loginItem = LoginItemModel()
    @Environment(\.openWindow) private var openWindow

    public init(model: AssistantModel, chatGPT: ChatGPTModel, observation: ObservationSession) {
        self.model = model
        self.chatGPT = chatGPT
        self.observation = observation
    }

    public var body: some View {
        NavigationSplitView {
            VStack(alignment: .leading, spacing: 0) {
                Text("settings.title", bundle: .module)
                    .font(.system(size: 26, weight: .semibold))
                    .padding(.horizontal, 24).padding(.top, 36).padding(.bottom, 14)
                List(SettingsSection.allCases, selection: $section) { item in
                    Label {
                        item.title
                    } icon: {
                        Image(systemName: item.systemImage)
                    }
                }
                .listStyle(.sidebar)
            }
            .safeAreaBar(edge: .bottom) {
                GlassEffectContainer {
                    // Settings has no root lock like the memory window, so the capsule gets it here.
                    SidebarStatus(model: model).disabled(model.busy || model.closing)
                }
                .padding(10)
            }
            .navigationSplitViewColumnWidth(244)
            // Keep section navigation available in the accepted fixed-size window.
            .toolbar(removing: .sidebarToggle)
        } detail: {
            VStack(alignment: .leading, spacing: 0) {
                section.title.font(.system(size: 26, weight: .semibold))
                    .padding(.horizontal, 28).padding(.top, 32)
                Group {
                    switch section {
                    case .general: Form { general }
                    case .chatGPT: ChatGPTSettingsView(chatGPT: chatGPT)
                    case .usage: ConnectionCheckView(model: model, chatGPT: chatGPT)
                    case .integrations: IntegrationSwitchesView(model: model, chatGPT: chatGPT)
                    case .github: GitHubSettingsView(source: model.githubSources, chatGPT: chatGPT)
                    case .jira: JiraSettingsView(source: model.jiraSources, chatGPT: chatGPT)
                    case .todoist:
                        TodoistSettingsView(source: model.todoistSources, chatGPT: chatGPT)
                    case .plugins: PluginsSettingsView(plugins: chatGPT.plugins, chatGPT: chatGPT)
                    case .observation: ObservationSettingsView(model: model, session: observation)
                    }
                }
                .formStyle(.grouped)
                .scrollContentBackground(.hidden)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(
                Color(nsColor: .textBackgroundColor),
                in: RoundedRectangle(cornerRadius: 20, style: .continuous)
            )
            .padding(8)
            .ignoresSafeArea(.container, edges: .top)
        }
        .toolbar(removing: .title)
        .containerBackground(.ultraThinMaterial, for: .window)
        .frame(width: 940, height: 800)
        // Stays on the root: reload also invalidates the reply, so it must not rerun per section.
        .task {
            await chatGPT.reload()
            await chatGPT.reloadChecks()
        }
    }

    @ViewBuilder private var general: some View {
        Section {
            Text("settings.cloud", bundle: .module).font(.headline)
            Text("settings.description", bundle: .module).foregroundStyle(.secondary)
        }
        AssistantStatusView(model: model)
        if let failure = model.failure {
            Callout(
                tone: Theme.error, systemImage: "exclamationmark.circle",
                title: Text("error.title", bundle: .module),
                message: Text(failure.message))
        }
        LocalPermissionsView(permissions: $permissions)
        LoginItemView(model: loginItem)
        VoiceSettingsView(voice: chatGPT.voice)
        Section {
            DisclosureGroup {
                Text("reply.temporary", bundle: .module).foregroundStyle(.secondary)
            } label: {
                Text("settings.details", bundle: .module)
            }
            Button {
                openWindow(id: "memory")
            } label: {
                Text("memory.manage", bundle: .module)
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule)
        }
    }
}

/// Settings sections retain their accepted order; observation is the ninth section (ADR-025).
enum SettingsSection: CaseIterable, Identifiable {
    case general, chatGPT, usage, integrations, github, jira, todoist, plugins, observation
    var id: Self { self }
    var title: Text {
        switch self {
        case .general: Text("settings.general", bundle: .module)
        case .chatGPT: Text(verbatim: "ChatGPT")
        case .usage: Text("settings.usage", bundle: .module)
        case .integrations: Text("integrations.title", bundle: .module)
        case .github: Text("integrations.github", bundle: .module)
        case .jira: Text("integrations.jira", bundle: .module)
        case .todoist: Text("integrations.todoist", bundle: .module)
        case .plugins: Text("plugins.title", bundle: .module)
        case .observation: Text("observation.title", bundle: .module)
        }
    }
    var systemImage: String {
        switch self {
        case .general: "gearshape"
        case .chatGPT: "bubble.left"
        case .usage: "chart.bar"
        case .integrations: "switch.2"
        case .github: "chevron.left.forwardslash.chevron.right"
        case .jira: "checklist"
        case .todoist: "checkmark.circle"
        case .plugins: "puzzlepiece.extension"
        case .observation: "eye"
        }
    }
}
