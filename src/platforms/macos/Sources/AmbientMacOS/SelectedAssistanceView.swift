import SwiftUI

/// Help for the case (page 9): the evidence switches («Выбрано сведений: N из 32»), the request and
/// reset, then the preview card with the exact context and the send capsule, the work row and the
/// error block. What is sent stays visible in the header line (D83).
struct SelectedAssistanceView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    let notes: [MemoryEvidence]
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                Text(
                    "assistance.selected \(chatGPT.selectedAssistanceNotes.count)", bundle: .module
                )
                .font(.headline)
                DetailsFootnote(text: Text("assistance.help", bundle: .module)) {
                    Text("assistance.details", bundle: .module)
                    Text("assistance.saved", bundle: .module)
                }
            }
            let selectable = notes.filter { $0.assistanceSelectable }
            if !selectable.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(selectable) { note in
                        evidenceSwitch(note)
                        if note.id != selectable.last?.id { Divider() }
                    }
                }
                .padding(.horizontal, 14).ambientInset(cornerRadius: 16)
            }
            if !chatGPT.assistanceReady {
                Text("assistance.gate", bundle: .module).font(.caption)
                    .foregroundStyle(.secondary)
            }
            HStack(spacing: 12) {
                Button {
                    Task { await chatGPT.prepareAssistance(task) }
                } label: {
                    Text("feed.new", bundle: .module)
                }
                .ambientPrimary()
                .disabled(
                    chatGPT.busy || !chatGPT.assistanceReady || model.manualPause != false)
                Button {
                    Task { await chatGPT.resetAssistance(clearNotes: true) }
                } label: {
                    Text("assistance.clear", bundle: .module)
                }
                .buttonStyle(.plain)
            }
            if let preview = chatGPT.assistancePreview, preview.taskID == task.id,
                preview.revision == task.revision
            {
                previewCard(preview)
            } else {
                Label {
                    Text("assistance.feedNote", bundle: .module)
                } icon: {
                    Image(systemName: "info.circle")
                }
                .font(.caption).foregroundStyle(.secondary)
            }
            if chatGPT.assisting {
                WorkRow(
                    title: Text(
                        chatGPT.state == .cancelling
                            ? "assistance.cancelling" : "assistance.working",
                        bundle: .module),
                    cancel: { Task { await chatGPT.cancel() } },
                    disabled: chatGPT.state == .cancelling
                )
                .ambientInset()
            }
            if let failure = chatGPT.assistanceFailure {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    title: Text("assistance.failed", bundle: .module),
                    message: Text(LocalizedStringKey(failure.key), bundle: .module))
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    /// Switch first, then the integration tag and the evidence text; the text stays the switch's
    /// accessibility label.
    private func evidenceSwitch(_ note: MemoryEvidence) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Toggle(
                isOn: Binding(
                    get: { chatGPT.selectedAssistanceNotes.contains(note.id) },
                    set: { selected in
                        Task { await chatGPT.selectAssistanceNote(note.id, selected: selected) }
                    })
            ) {
                Text(verbatim: note.text)
            }
            .toggleStyle(.switch).labelsHidden()
            .disabled(chatGPT.busy)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if note.source != nil {
                    Pill(text: Text("integration.historyTag", bundle: .module))
                }
                Text(verbatim: note.text).lineLimit(3)
            }
            .accessibilityHidden(true)
            Spacer(minLength: 0)
        }
        .padding(.vertical, 10)
    }
    private func previewCard(_ preview: AssistancePreview) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("assistance.previewTitle", bundle: .module).font(.headline)
                Text(
                    "assistance.previewContents \(chatGPT.selectedAssistanceNotes.count)",
                    bundle: .module
                )
                .font(.caption).foregroundStyle(.secondary)
            }
            Label {
                Text("feed.replace", bundle: .module)
            } icon: {
                Image(systemName: "exclamationmark.triangle")
            }
            .font(.callout).foregroundStyle(Theme.stale.text)
            DisclosureGroup {
                ScrollView([.horizontal, .vertical]) {
                    Text(verbatim: preview.context).font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 220).padding(10)
                .background(.background, in: RoundedRectangle(cornerRadius: 12))
            } label: {
                Text("assistance.preview", bundle: .module).font(.callout.weight(.medium))
            }
            Button {
                Task { await chatGPT.sendAssistance(task) }
            } label: {
                Text("assistance.send", bundle: .module)
            }
            .ambientPrimary()
            .disabled(chatGPT.busy || model.manualPause != false)
        }
        .padding(16).frame(maxWidth: .infinity, alignment: .leading)
        .ambientInset(cornerRadius: 18)
    }
}
