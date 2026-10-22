import SwiftUI

/// The Codex source (page 8): the session file row with its read action and «⋯» menu, the event
/// radio list with «Раньше», «Привязать событие» with an optional preview and the footnote.
struct CodexSourcesView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Bindable var source: CodexSourcesModel
    @State private var deletion: CodexSelection?
    @State private var showsPreview = false
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            fileRow
            if let settings = source.settings { savedFiles(settings) }
            if source.busy {
                WorkRow(
                    title: Text("source.working", bundle: .module), cancel: { source.cancel() },
                    disabled: false
                )
                .ambientInset()
            }
            if !chatGPT.assistanceReady {
                Text("source.gate", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
            if let observation = source.observation { events(observation) }
            if source.saved { Text("source.saved", bundle: .module).font(.callout) }
            if !source.savedMetadata.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("codex.savedEvents", bundle: .module).font(.callout.weight(.medium))
                    ForEach(source.savedMetadata) { m in
                        Text(LocalizedStringKey(CodexEvent.kindKey(m.kind)), bundle: .module)
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if let key = source.failureKey {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    message: Text(LocalizedStringKey(key), bundle: .module))
            }
            DetailsFootnote(text: Text("codex.footnote", bundle: .module)) {
                Text("codex.scope", bundle: .module)
                Text("codex.limits", bundle: .module)
                Text("codex.coverage", bundle: .module)
            }
            .padding(.top, 8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .task(id: task) { await source.loadMetadata(task: task) }
        .confirmationDialog(
            Text("codex.deleteTitle", bundle: .module),
            isPresented: Binding(get: { deletion != nil }, set: { if !$0 { deletion = nil } }),
            titleVisibility: .visible
        ) {
            if let deletion {
                Button(role: .destructive) {
                    Task { await model.deleteCodexSourceSettings(deletion, chatGPT: chatGPT) }
                    self.deletion = nil
                } label: {
                    Text("delete.confirm", bundle: .module)
                }
            }
            Button(role: .cancel) {
            } label: {
                Text("switch.cancel", bundle: .module)
            }
        } message: {
            if let deletion { Text("codex.deleteWarning \(shown(deletion.path))", bundle: .module) }
        }
        .dialogIcon(Image(systemName: "doc"))
    }
    /// The chosen session file, when it was read and whether the window is partial; the read (or
    /// saving the file choice first) and the file actions.
    private var fileRow: some View {
        HStack(spacing: 12) {
            Image(systemName: "apple.terminal").font(.title3).foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                if let path = source.chosenPath {
                    Text(verbatim: shown(path)).font(.callout.monospaced()).textSelection(.enabled)
                        .lineLimit(1).truncationMode(.middle).help(Text(verbatim: path))
                } else {
                    Text("codex.none", bundle: .module).font(.callout)
                }
                if let observation = source.observation {
                    HStack(spacing: 4) {
                        Text("codex.readAt", bundle: .module)
                        Text(
                            Date(timeIntervalSince1970: TimeInterval(observation.observedAt)),
                            format: .dateTime.day().month().hour().minute())
                        if observation.reasons != 0 {
                            Text(verbatim: "·").accessibilityHidden(true)
                            Text("source.partial", bundle: .module)
                                .foregroundStyle(Theme.stale.text)
                        }
                    }
                    .font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 8)
            if source.chosenPath == nil {
                Button {
                    Task { await source.choose() }
                } label: {
                    Text("codex.choose", bundle: .module)
                }
                .disabled(source.busy)
            } else if source.scopeSaved {
                Button {
                    Task { await source.read(task: task, chatGPT: chatGPT) }
                } label: {
                    Text(
                        source.observation == nil ? "codex.read" : "codex.refresh", bundle: .module)
                }
                .disabled(!canRead)
            } else {
                Button {
                    Task { await model.saveCodexSourceSettings(chatGPT: chatGPT) }
                } label: {
                    Text("source.saveSettings", bundle: .module)
                }
                .disabled(source.busy)
            }
            Menu {
                Button {
                    Task { await source.choose() }
                } label: {
                    Text("codex.choose", bundle: .module)
                }
                .disabled(source.busy)
                Button {
                    Task { await source.listSaved() }
                } label: {
                    Text("codex.savedFiles", bundle: .module)
                }
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.button).menuIndicator(.hidden).buttonBorderShape(.circle)
            .accessibilityLabel(Text("source.actions", bundle: .module))
        }
        .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
        .padding(14)
        .ambientInset(cornerRadius: 16)
    }
    private func savedFiles(_ settings: [CodexSelection]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("codex.savedFiles", bundle: .module).font(.callout.weight(.medium))
            ForEach(settings) { setting in
                HStack {
                    Button {
                        source.use(setting)
                    } label: {
                        Text(verbatim: shown(setting.path)).font(.caption.monospaced())
                            .lineLimit(1).truncationMode(.middle)
                    }
                    .buttonStyle(.link).disabled(source.busy)
                    Spacer()
                    Button(role: .destructive) {
                        deletion = setting
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(Text("codex.deleteSettings", bundle: .module))
                    .help(Text("codex.deleteSettings", bundle: .module))
                }
            }
        }
    }
    @ViewBuilder private func events(_ observation: CodexObservation) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("codex.which", bundle: .module).font(.headline)
                Spacer()
                Button {
                    Task { await source.read(task: task, chatGPT: chatGPT, earlier: true) }
                } label: {
                    Text("codex.earlier", bundle: .module)
                }
                .buttonStyle(.plain).foregroundStyle(.secondary)
                .disabled(!canRead || observation.cursor == 0)
            }
            ForEach(Array(reasons.enumerated()), id: \.offset) { index, key in
                if observation.reasons & (1 << index) != 0 {
                    Text(LocalizedStringKey(key), bundle: .module).font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if observation.events.isEmpty {
                Text("codex.empty", bundle: .module).foregroundStyle(.secondary)
            } else {
                // The radio shows the model's choice; picking an event asks the model for its preview.
                Picker(
                    selection: Binding(
                        get: { source.selectedIndex },
                        set: { index in
                            if let event = observation.events.first(where: { $0.id == index }) {
                                Task { await source.selectEvent(event) }
                            }
                        })
                ) {
                    ForEach(observation.events) { event in
                        eventRow(event).tag(Optional(event.id))
                    }
                } label: {
                    Text("codex.which", bundle: .module)
                }
                .pickerStyle(.radioGroup).labelsHidden()
            }
            HStack(spacing: 12) {
                Button {
                    Task {
                        if await source.bind(chatGPT: chatGPT) {
                            await model.reload()
                            await source.loadMetadata(task: task)
                        }
                    }
                } label: {
                    Text("codex.bind", bundle: .module)
                }
                .ambientPrimary()
                .disabled(source.busy || model.manualPause != false || source.preview == nil)
                Button {
                    showsPreview.toggle()
                } label: {
                    Text("source.preview", bundle: .module)
                }
                .buttonStyle(.plain).disabled(source.preview == nil)
            }
            .padding(.top, 4)
            if showsPreview, let preview = source.preview {
                ScrollView {
                    Text(verbatim: preview).font(.caption.monospaced()).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 200).padding(12).ambientInset(cornerRadius: 12)
            }
        }
    }
    private func eventRow(_ event: CodexEvent) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Group {
                if let at = event.observedAt {
                    Text(
                        Date(timeIntervalSince1970: TimeInterval(at)),
                        format: .dateTime.hour().minute())
                } else {
                    Text(verbatim: "—")
                }
            }
            .font(.callout.monospacedDigit()).foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 4) {
                    Text(LocalizedStringKey(event.kindKey), bundle: .module)
                    if event.kind >= 6 {
                        Text(verbatim: "·").accessibilityHidden(true)
                        Text("codex.projection", bundle: .module)
                    }
                    if event.partial {
                        Text(verbatim: "·").accessibilityHidden(true)
                        Text("codex.textOnly", bundle: .module)
                    }
                }
                .font(.caption).foregroundStyle(.secondary)
                Text(verbatim: event.text).fixedSize(horizontal: false, vertical: true)
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
