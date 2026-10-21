import SwiftUI

/// The five sources as a second row of tabs, names only (ADR-023 #4, #5). Only the shown source is
/// built; switching sources does not cancel a read, which the model and the window root own (D76).
struct SourcesTab: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Binding var source: BuiltInIntegration
    var body: some View {
        Picker(selection: $source) {
            ForEach(BuiltInIntegration.allCases, id: \.self) { item in
                Text(LocalizedStringKey(item.titleKey), bundle: .module).tag(item)
            }
        } label: {
            Text("case.sources", bundle: .module)
        }
        .pickerStyle(.segmented).labelsHidden()
        if model.integrations.isEnabled(source) {
            switch source {
            case .localProjects:
                LocalProjectsView(
                    model: model, chatGPT: chatGPT, task: task, projects: model.localProjects)
            case .codex:
                CodexSourcesView(
                    model: model, chatGPT: chatGPT, task: task, source: model.codexSources)
            case .github:
                IntegrationSourcesView(
                    model: model, chatGPT: chatGPT, task: task, source: model.githubSources)
            case .jira:
                IntegrationSourcesView(
                    model: model, chatGPT: chatGPT, task: task, source: model.jiraSources)
            case .todoist:
                IntegrationSourcesView(
                    model: model, chatGPT: chatGPT, task: task, source: model.todoistSources)
            }
        } else {
            IntegrationDisabledView(kind: source)
        }
    }
}
