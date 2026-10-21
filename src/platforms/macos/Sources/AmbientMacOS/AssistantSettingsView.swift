import SwiftUI

public struct AssistantSettingsView: View {
    private let model: AssistantModel
    private let chatGPT: ChatGPTModel
    @State private var section = SettingsSection.general
    // Owned here, not by the General section: a section switch must not reset a permission
    // check or hide a launch-at-login failure.
    @State private var permissions = LocalPermissions()
    @State private var loginItem = LoginItemModel()
    @Environment(\.openWindow) private var openWindow

    public init(model: AssistantModel, chatGPT: ChatGPTModel) {
        self.model = model
        self.chatGPT = chatGPT
    }

    public var body: some View {
        NavigationSplitView {
            List(SettingsSection.allCases, selection: $section) { item in
                Label {
                    item.title
                } icon: {
                    Image(systemName: item.systemImage)
                }
            }
            .listStyle(.sidebar)
            .safeAreaBar(edge: .bottom) {
                GlassEffectContainer {
                    // Settings has no root lock like the memory window, so the capsule gets it here.
                    SidebarStatus(model: model).disabled(model.busy || model.closing)
                }
                .padding(10)
            }
            .navigationSplitViewColumnWidth(244)
        } detail: {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    section.title.font(.system(size: 22, weight: .semibold))
                    switch section {
                    case .general: general
                    case .chatGPT: ChatGPTSettingsView(chatGPT: chatGPT)
                    case .usage: ConnectionCheckView(model: model, chatGPT: chatGPT)
                    case .integrations: IntegrationSwitchesView(model: model, chatGPT: chatGPT)
                    case .github: GitHubSettingsView(source: model.githubSources, chatGPT: chatGPT)
                    case .jira: JiraSettingsView(source: model.jiraSources, chatGPT: chatGPT)
                    case .todoist:
                        TodoistSettingsView(source: model.todoistSources, chatGPT: chatGPT)
                    case .plugins: PluginsSettingsView(plugins: chatGPT.plugins, chatGPT: chatGPT)
                    }
                }
                .padding(.horizontal, 36).padding(.vertical, 30)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(nsColor: .textBackgroundColor))
        }
        // The window has a fixed size, so a collapsed sidebar would leave no way to switch sections.
        .toolbar(removing: .sidebarToggle)
        .frame(width: 940, height: 800)
        // Stays on the root: reload also invalidates the reply, so it must not rerun per section.
        .task {
            await chatGPT.reload()
            await chatGPT.reloadChecks()
        }
    }

    @ViewBuilder private var general: some View {
        Text("settings.cloud", bundle: .module).font(.headline)
        Text("settings.description", bundle: .module)
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
        Text("reply.temporary", bundle: .module).font(.caption).foregroundStyle(.secondary)
        Button {
            openWindow(id: "memory")
        } label: {
            Text("memory.manage", bundle: .module)
        }
        .buttonStyle(.bordered).buttonBorderShape(.capsule)
    }
}

/// The eight Settings sections in sidebar order (ADR-023).
enum SettingsSection: CaseIterable, Identifiable {
    case general, chatGPT, usage, integrations, github, jira, todoist, plugins
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
        }
    }
}
