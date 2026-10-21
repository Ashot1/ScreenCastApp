import Observation
import SwiftUI

/// Native view state of the five Core-owned integration switches (D61). Core enforces them;
/// this model only shows the saved values and forwards user changes.
@MainActor @Observable final class IntegrationSwitchesModel {
    private let memory: MemorySession
    // Every load or change advances this; a late answer for an older generation is dropped.
    private var generation: UInt64 = 0
    private(set) var enabled: [BuiltInIntegration: Bool] = [:]
    private(set) var changing = false
    private(set) var failureKey: String?
    init(memory: MemorySession) {
        self.memory = memory
    }
    /// Unknown until loaded; an unknown switch is treated as off so reading stays closed.
    func isEnabled(_ kind: BuiltInIntegration) -> Bool { enabled[kind] == true }
    func load() async {
        generation += 1
        let accepted = generation
        do {
            var values: [BuiltInIntegration: Bool] = [:]
            for kind in BuiltInIntegration.allCases {
                values[kind] = try await memory.integrationEnabled(kind)
            }
            guard accepted == generation else { return }
            enabled = values
        } catch {
            guard accepted == generation else { return }
            enabled = [:]
            failureKey = "error.storage"
        }
    }
    func set(_ kind: BuiltInIntegration, enabled value: Bool) async {
        guard !changing else { return }
        changing = true
        defer { changing = false }
        do {
            try await memory.setIntegrationEnabled(kind, enabled: value)
            failureKey = nil
        } catch {
            failureKey = (error as? ModelCheckFailure)?.key ?? "error.storage"
        }
        await load()
    }
}

struct IntegrationSwitchesView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("integrations.help", bundle: .module)
                ForEach(BuiltInIntegration.allCases, id: \.self) { kind in
                    Toggle(
                        isOn: Binding(
                            get: { model.integrations.isEnabled(kind) },
                            set: { value in
                                Task {
                                    await model.setIntegrationEnabled(
                                        kind, enabled: value, chatGPT: chatGPT)
                                }
                            })
                    ) {
                        Text(LocalizedStringKey(kind.titleKey), bundle: .module)
                    }.disabled(model.integrations.changing)
                }
                if let failure = model.integrations.failureKey {
                    Text(LocalizedStringKey(failure), bundle: .module).foregroundStyle(.red)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            Text("integrations.title", bundle: .module)
        }
        .task { await model.integrations.load() }
    }
}

/// Shown instead of a source's read controls while its integration is off.
struct IntegrationDisabledView: View {
    let kind: BuiltInIntegration
    var body: some View {
        GroupBox {
            Text("integrations.disabled", bundle: .module).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            Text(LocalizedStringKey(kind.titleKey), bundle: .module)
        }
    }
}

extension BuiltInIntegration {
    var titleKey: String {
        switch self {
        case .localProjects: "integrations.localProjects"
        case .codex: "integrations.codex"
        case .github: "integrations.github"
        case .jira: "integrations.jira"
        case .todoist: "integrations.todoist"
        }
    }
}
