import SwiftUI

struct CodexSourcesView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Bindable var source: CodexSourcesModel
    @State private var deletion: CodexSelection?
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("codex.title", bundle: .module).font(.headline)
                Text("codex.scope", bundle: .module).font(.caption)
                Text("codex.limits", bundle: .module).font(.caption)
                HStack {
                    Button {
                        Task { await source.choose() }
                    } label: {
                        Text("codex.choose", bundle: .module)
                    }.disabled(source.busy)
                    Button {
                        Task { await source.listSaved() }
                    } label: {
                        Text("source.savedSettings", bundle: .module)
                    }
                }
                if let settings = source.settings {
                    ForEach(settings) { setting in
                        HStack {
                            Button {
                                source.use(setting)
                            } label: {
                                Text(verbatim: setting.path)
                            }.disabled(source.busy)
                            Button(role: .destructive) {
                                deletion = setting
                            } label: {
                                Text("source.deleteSettings", bundle: .module)
                            }
                        }
                    }
                }
                if let path = source.chosenPath {
                    Text(verbatim: path).textSelection(.enabled)
                    HStack {
                        Button {
                            Task { await model.saveCodexSourceSettings(chatGPT: chatGPT) }
                        } label: {
                            Text("source.saveSettings", bundle: .module)
                        }.disabled(source.busy)
                        Button {
                            Task { await source.read(task: task, chatGPT: chatGPT) }
                        } label: {
                            Text("codex.read", bundle: .module)
                        }.disabled(!canRead)
                        if source.busy {
                            Button {
                                source.cancel()
                            } label: {
                                Text("source.cancel", bundle: .module)
                            }
                        }
                    }
                }
                if source.busy { ProgressView { Text("source.working", bundle: .module) } }
                if !chatGPT.assistanceReady { Text("source.gate", bundle: .module).font(.caption) }
                if let observation = source.observation {
                    Text(
                        Date(timeIntervalSince1970: TimeInterval(observation.observedAt)),
                        format: .dateTime)
                    ForEach(Array(reasons.enumerated()), id: \.offset) { index, key in
                        if observation.reasons & (1 << index) != 0 {
                            Text(LocalizedStringKey(key), bundle: .module).font(.caption)
                        }
                    }
                    if observation.events.isEmpty { Text("codex.empty", bundle: .module) }
                    ScrollView {
                        VStack(alignment: .leading, spacing: 12) {
                            ForEach(observation.events) { event in
                                VStack(alignment: .leading) {
                                    Text(LocalizedStringKey(event.kindKey), bundle: .module).font(
                                        .subheadline.bold())
                                    if let at = event.observedAt {
                                        Text(
                                            Date(timeIntervalSince1970: TimeInterval(at)),
                                            format: .dateTime
                                        ).font(.caption)
                                    }
                                    if event.kind >= 6 {
                                        Text("codex.projection", bundle: .module).font(.caption)
                                    }
                                    if event.partial {
                                        Text("codex.textOnly", bundle: .module).font(.caption)
                                    }
                                    Text(verbatim: event.text).textSelection(.enabled)
                                    Button {
                                        Task { await source.selectEvent(event) }
                                    } label: {
                                        Text("source.select", bundle: .module)
                                    }
                                }
                            }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }.frame(maxHeight: 300)
                    HStack {
                        Button {
                            Task { await source.read(task: task, chatGPT: chatGPT, earlier: true) }
                        } label: {
                            Text("codex.earlier", bundle: .module)
                        }.disabled(!canRead || observation.cursor == 0)
                        Button {
                            Task { await source.read(task: task, chatGPT: chatGPT) }
                        } label: {
                            Text("codex.refresh", bundle: .module)
                        }.disabled(!canRead)
                    }
                }
                if let preview = source.preview {
                    Text("source.preview", bundle: .module).font(.headline)
                    ScrollView { Text(verbatim: preview).textSelection(.enabled) }.frame(
                        maxHeight: 200)
                    Button {
                        Task {
                            if await source.bind(chatGPT: chatGPT) {
                                await model.reload()
                                await source.loadMetadata(task: task)
                            }
                        }
                    } label: {
                        Text("source.bind", bundle: .module)
                    }.disabled(source.busy || model.manualPause != false)
                }
                if source.saved { Text("source.saved", bundle: .module) }
                ForEach(source.savedMetadata) { m in
                    Text(LocalizedStringKey(CodexEvent.kindKey(m.kind)), bundle: .module).font(
                        .caption)
                }
                Text("codex.coverage", bundle: .module).font(.caption)
                if let key = source.failureKey {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(key), bundle: .module))
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        .task(id: task) { await source.loadMetadata(task: task) }
        .confirmationDialog(
            Text("source.deleteWarning", bundle: .module),
            isPresented: Binding(get: { deletion != nil }, set: { if !$0 { deletion = nil } })
        ) {
            if let deletion {
                Button(role: .destructive) {
                    Task { await model.deleteCodexSourceSettings(deletion, chatGPT: chatGPT) }
                    self.deletion = nil
                } label: {
                    Text("source.deleteSettings", bundle: .module)
                }
            }
        }
    }
    private var canRead: Bool {
        !source.busy && source.scopeSaved && model.manualPause == false && chatGPT.assistanceReady
    }
    private let reasons = [
        "codex.reason.partial", "codex.reason.limit", "codex.reason.malformed",
        "codex.reason.unsupported", "codex.reason.ordinal", "codex.reason.compacted",
        "codex.reason.filtered", "codex.reason.turn",
    ]
}
