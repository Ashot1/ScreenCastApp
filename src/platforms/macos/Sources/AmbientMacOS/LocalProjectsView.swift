import SwiftUI

struct LocalProjectsView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Bindable var projects: LocalProjectsModel
    @State private var deletion: LocalRootSettings?
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("source.title", bundle: .module).font(.headline)
                Text("source.scope", bundle: .module).font(.caption)
                Text("source.limits", bundle: .module).font(.caption)
                HStack {
                    Button {
                        Task { await projects.choose() }
                    } label: {
                        Text("source.choose", bundle: .module)
                    }.disabled(projects.busy)
                    Button {
                        Task { await projects.listSaved() }
                    } label: {
                        Text("source.savedSettings", bundle: .module)
                    }
                }
                if let roots = projects.savedRoots {
                    ForEach(roots) { root in
                        HStack {
                            Text(verbatim: root.path).textSelection(.enabled)
                            Button(role: .destructive) {
                                deletion = root
                            } label: {
                                Text("source.deleteSettings", bundle: .module)
                            }
                        }
                    }
                }
                if let root = projects.root {
                    Text("source.location", bundle: .module).font(.subheadline.bold())
                    Text(verbatim: root.path).textSelection(.enabled)
                    Text("source.exclusions", bundle: .module)
                    TextEditor(text: $projects.exclusionsDraft).frame(height: 80)
                    Text("source.exclusionsHelp", bundle: .module).font(.caption)
                    HStack {
                        Button {
                            Task { await model.saveLocalSourceSettings(chatGPT: chatGPT) }
                        } label: {
                            Text("source.saveSettings", bundle: .module)
                        }
                        Button {
                            Task { await projects.scan(task: task, chatGPT: chatGPT) }
                        } label: {
                            Text("source.scan", bundle: .module)
                        }
                        .disabled(
                            projects.busy || !projects.scopeSaved || model.manualPause != false
                                || !chatGPT.assistanceReady)
                        if projects.busy {
                            Button {
                                projects.cancel()
                            } label: {
                                Text("source.cancel", bundle: .module)
                            }
                        }
                    }
                }
                if projects.busy { ProgressView { Text("source.working", bundle: .module) } }
                if !chatGPT.assistanceReady { Text("source.gate", bundle: .module).font(.caption) }
                if let observation = projects.observation {
                    Text("source.observation", bundle: .module).font(.headline)
                    Text(LocalizedStringKey(coverage(observation.completeness)), bundle: .module)
                    HStack {
                        Text(
                            Date(timeIntervalSince1970: TimeInterval(observation.started)),
                            format: .dateTime)
                        Text("…")
                        Text(
                            Date(timeIntervalSince1970: TimeInterval(observation.ended)),
                            format: .dateTime)
                    }
                    ForEach(Array(reasons.enumerated()), id: \.offset) { index, key in
                        if observation.reasons & (1 << index) != 0 {
                            Text(LocalizedStringKey(key), bundle: .module).foregroundStyle(
                                .secondary)
                        }
                    }
                    if observation.branches.isEmpty { Text("source.empty", bundle: .module) }
                    ForEach(observation.branches) { branch in
                        VStack(alignment: .leading) {
                            Text(verbatim: branch.fullRef).textSelection(.enabled)
                            if let oid = branch.oid {
                                Text(verbatim: oid).font(.caption.monospaced())
                            }
                            Text(LocalizedStringKey(heads[Int(branch.head)]), bundle: .module).font(
                                .caption)
                            Button {
                                Task { await projects.selectBranch(branch) }
                            } label: {
                                Text("source.select", bundle: .module)
                            }
                        }
                    }
                }
                if let preview = projects.preview {
                    Text("source.preview", bundle: .module).font(.subheadline.bold())
                    Text(verbatim: preview).textSelection(.enabled)
                    Button {
                        Task {
                            if await projects.bind(chatGPT: chatGPT) { await model.reload() }
                        }
                    } label: {
                        Text("source.bind", bundle: .module)
                    }.disabled(projects.busy || model.manualPause != false)
                }
                if projects.saved { Text("source.saved", bundle: .module) }
                Text("source.historical", bundle: .module).font(.caption)
                if let key = projects.failureKey {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(key), bundle: .module))
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        .confirmationDialog(
            Text("source.deleteWarning", bundle: .module),
            isPresented: Binding(get: { deletion != nil }, set: { if !$0 { deletion = nil } })
        ) {
            if let deletion {
                Button(role: .destructive) {
                    Task { await model.deleteLocalSourceSettings(deletion, chatGPT: chatGPT) }
                    self.deletion = nil
                } label: {
                    Text("source.deleteSettings", bundle: .module)
                }
            }
        }
    }
    private func coverage(_ value: MemoryCompleteness) -> String {
        switch value {
        case .complete: "source.complete"
        case .partial: "source.partial"
        case .unknown: "source.unknown"
        }
    }
    private let reasons = [
        "source.reason.excluded", "source.reason.limit", "source.reason.unavailable",
        "source.reason.name", "source.reason.malformed", "source.reason.unsupported",
        "source.reason.changed", "source.reason.symbolic",
    ]
    private let heads = [
        "source.head.unknown", "source.head.current", "source.head.other", "source.head.detached",
        "source.head.unborn",
    ]
}
