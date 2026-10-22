import SwiftUI

/// The local project source (page 8): the folder row with its scan action and «⋯» menu, the
/// exclusions, the branch radio list, «Привязать ветку» with an optional preview and the footnote.
struct LocalProjectsView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Bindable var projects: LocalProjectsModel
    @State private var deletion: LocalRootSettings?
    @State private var showsPreview = false
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            folderRow
            if projects.root != nil {
                DisclosureGroup {
                    VStack(alignment: .leading, spacing: 6) {
                        TextEditor(text: $projects.exclusionsDraft).font(.callout.monospaced())
                            .frame(height: 80).scrollContentBackground(.hidden)
                            .padding(6).ambientInset(cornerRadius: 10)
                        Text("source.exclusionsHelp", bundle: .module).font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.top, 6)
                } label: {
                    Text("source.exclusions", bundle: .module).font(.callout)
                }
            }
            if let roots = projects.savedRoots { savedRoots(roots) }
            if projects.busy {
                WorkRow(
                    title: Text("source.working", bundle: .module), cancel: { projects.cancel() },
                    disabled: false
                )
                .ambientInset()
            }
            if !chatGPT.assistanceReady {
                Text("source.gate", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
            if let observation = projects.observation { branches(observation) }
            if projects.saved { Text("source.saved", bundle: .module).font(.callout) }
            if let key = projects.failureKey {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    message: Text(LocalizedStringKey(key), bundle: .module))
            }
            DetailsFootnote(text: Text("source.footnote", bundle: .module)) {
                Text("source.scope", bundle: .module)
                Text("source.limits", bundle: .module)
                Text("source.historical", bundle: .module)
            }
            .padding(.top, 8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .confirmationDialog(
            Text("source.deleteTitle", bundle: .module),
            isPresented: Binding(get: { deletion != nil }, set: { if !$0 { deletion = nil } }),
            titleVisibility: .visible
        ) {
            if let deletion {
                Button(role: .destructive) {
                    Task { await model.deleteLocalSourceSettings(deletion, chatGPT: chatGPT) }
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
            if let deletion {
                Text("source.deleteWarning \(shown(deletion.path))", bundle: .module)
            }
        }
        .dialogIcon(Image(systemName: "folder"))
    }
    /// The chosen folder, when it was scanned and how completely; the scan (or saving a changed
    /// scope first) and the folder actions.
    private var folderRow: some View {
        HStack(spacing: 12) {
            Image(systemName: "folder").font(.title3).foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                if let root = projects.root {
                    Text(verbatim: shown(root.path)).font(.callout.monospaced())
                        .textSelection(.enabled).lineLimit(1).truncationMode(.middle)
                        .help(Text(verbatim: root.path))
                } else {
                    Text("source.none", bundle: .module).font(.callout)
                }
                if let observation = projects.observation {
                    HStack(spacing: 4) {
                        Text("source.scanned", bundle: .module)
                        Text(
                            Date(timeIntervalSince1970: TimeInterval(observation.ended)),
                            format: .dateTime.day().month().hour().minute())
                        Text(verbatim: "·").accessibilityHidden(true)
                        Text(
                            LocalizedStringKey(coverage(observation.completeness)), bundle: .module
                        )
                        .foregroundStyle(
                            observation.completeness == .complete
                                ? AnyShapeStyle(.secondary) : AnyShapeStyle(Theme.stale.text))
                    }
                    .font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 8)
            if projects.root == nil {
                Button {
                    Task { await projects.choose() }
                } label: {
                    Text("source.choose", bundle: .module)
                }
                .disabled(projects.busy)
            } else if projects.scopeSaved {
                Button {
                    Task { await projects.scan(task: task, chatGPT: chatGPT) }
                } label: {
                    Text(
                        projects.observation == nil ? "source.scan" : "source.rescan",
                        bundle: .module)
                }
                .disabled(
                    projects.busy || model.manualPause != false
                        || !chatGPT.assistanceReady)
            } else {
                Button {
                    Task { await model.saveLocalSourceSettings(chatGPT: chatGPT) }
                } label: {
                    Text("source.saveSettings", bundle: .module)
                }
            }
            Menu {
                Button {
                    Task { await projects.choose() }
                } label: {
                    Text("source.choose", bundle: .module)
                }
                .disabled(projects.busy)
                Button {
                    Task { await projects.listSaved() }
                } label: {
                    Text("source.savedSettings", bundle: .module)
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
    private func savedRoots(_ roots: [LocalRootSettings]) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("source.savedSettings", bundle: .module).font(.callout.weight(.medium))
            ForEach(roots) { root in
                HStack {
                    Text(verbatim: shown(root.path)).font(.caption.monospaced()).textSelection(
                        .enabled
                    )
                    .lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button(role: .destructive) {
                        deletion = root
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(Text("source.deleteSettings", bundle: .module))
                    .help(Text("source.deleteSettings", bundle: .module))
                }
            }
        }
    }
    @ViewBuilder private func branches(_ observation: LocalObservation) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("source.which", bundle: .module).font(.headline)
            ForEach(Array(reasons.enumerated()), id: \.offset) { index, key in
                if observation.reasons & (1 << index) != 0 {
                    Text(LocalizedStringKey(key), bundle: .module).font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if observation.branches.isEmpty {
                Text("source.empty", bundle: .module).foregroundStyle(.secondary)
            } else {
                // The radio shows the model's choice; picking a branch asks the model for its preview.
                Picker(
                    selection: Binding(
                        get: { projects.selectedIndex },
                        set: { index in
                            if let branch = observation.branches.first(where: { $0.index == index })
                            {
                                Task { await projects.selectBranch(branch) }
                            }
                        })
                ) {
                    ForEach(observation.branches) { branch in
                        HStack {
                            Text(verbatim: name(branch)).font(.callout.monospaced())
                            Spacer()
                            Text(LocalizedStringKey(heads[Int(branch.head)]), bundle: .module)
                                .font(.caption).foregroundStyle(headStyle(branch.head))
                        }
                        .help(Text(verbatim: branch.oid ?? branch.fullRef))
                        .tag(Optional(branch.index))
                    }
                } label: {
                    Text("source.which", bundle: .module)
                }
                .pickerStyle(.radioGroup).labelsHidden()
            }
            HStack(spacing: 12) {
                Button {
                    Task {
                        if await projects.bind(chatGPT: chatGPT) { await model.reload() }
                    }
                } label: {
                    Text("source.bind", bundle: .module)
                }
                .ambientPrimary()
                .disabled(projects.busy || model.manualPause != false || projects.preview == nil)
                Button {
                    showsPreview.toggle()
                } label: {
                    Text("source.preview", bundle: .module)
                }
                .buttonStyle(.plain).disabled(projects.preview == nil)
            }
            .padding(.top, 4)
            if showsPreview, let preview = projects.preview {
                Text(verbatim: preview).font(.caption.monospaced()).textSelection(.enabled)
                    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
                    .ambientInset(cornerRadius: 12)
            }
        }
    }
    /// A branch under refs/heads reads by its short name, as Git shows it.
    private func name(_ branch: LocalBranch) -> String {
        branch.fullRef.hasPrefix("refs/heads/")
            ? String(branch.fullRef.dropFirst("refs/heads/".count)) : branch.fullRef
    }
    private func headStyle(_ head: UInt32) -> AnyShapeStyle {
        switch head {
        case 1: AnyShapeStyle(Theme.ready.text)
        case 3: AnyShapeStyle(Theme.stale.text)
        default: AnyShapeStyle(.secondary)
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
