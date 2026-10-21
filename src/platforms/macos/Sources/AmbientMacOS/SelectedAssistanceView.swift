import SwiftUI

struct SelectedAssistanceView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    let notes: [MemoryEvidence]
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("assistance.title", bundle: .module).font(.headline)
                Text("assistance.help", bundle: .module)
                Text("assistance.saved", bundle: .module).font(.caption)
                HStack {
                    Text("assistance.selected", bundle: .module)
                    Text(chatGPT.selectedAssistanceNotes.count, format: .number)
                }
                ForEach(notes.filter { $0.assistanceSelectable }) { note in
                    Toggle(
                        isOn: Binding(
                            get: { chatGPT.selectedAssistanceNotes.contains(note.id) },
                            set: { selected in
                                Task {
                                    await chatGPT.selectAssistanceNote(note.id, selected: selected)
                                }
                            })
                    ) {
                        VStack(alignment: .leading) {
                            if case .external(let key, _) = note.origin,
                                IntegrationKind(namespace: key.namespace) != nil
                            {
                                Text("integration.history", bundle: .module).font(.caption)
                            }
                            Text(verbatim: note.text).lineLimit(3)
                        }
                    }.disabled(chatGPT.busy)
                }
                if !chatGPT.assistanceReady {
                    Text("assistance.gate", bundle: .module).foregroundStyle(.secondary)
                }
                HStack {
                    Button {
                        Task { await chatGPT.prepareAssistance(task) }
                    } label: {
                        Text("feed.new", bundle: .module)
                    }
                    .disabled(
                        chatGPT.busy || !chatGPT.assistanceReady || model.manualPause != false)
                    Button {
                        Task { await chatGPT.resetAssistance(clearNotes: true) }
                    } label: {
                        Text("assistance.clear", bundle: .module)
                    }
                }
                if let preview = chatGPT.assistancePreview, preview.taskID == task.id,
                    preview.revision == task.revision
                {
                    Text("feed.replace", bundle: .module).font(.caption)
                    DisclosureGroup {
                        ScrollView([.horizontal, .vertical]) {
                            Text(verbatim: preview.context).font(
                                .system(.caption, design: .monospaced)
                            ).textSelection(.enabled)
                        }.frame(maxHeight: 220)
                    } label: {
                        Text("assistance.preview", bundle: .module)
                    }
                    Button {
                        Task { await chatGPT.sendAssistance(task) }
                    } label: {
                        Text("assistance.send", bundle: .module)
                    }
                    .disabled(chatGPT.busy || model.manualPause != false)
                }
                if chatGPT.assisting {
                    Text("assistance.working", bundle: .module)
                    Button {
                        Task { await chatGPT.cancel() }
                    } label: {
                        Text("check.cancel", bundle: .module)
                    }
                    .disabled(chatGPT.state == .cancelling)
                }
                if let failure = chatGPT.assistanceFailure {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure.key), bundle: .module))
                }

            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
