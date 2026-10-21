import SwiftUI

enum CaseDeletion {
    case task
    case note(UInt64)
}

/// The case sections; a segmented picker switches them and only the shown one is built (D76: a
/// source read continues while another tab is shown).
enum CaseTab {
    case plan, notes, links, sources, help
    static func tabs(for task: MemoryTask) -> [CaseTab] {
        task.lifecycle == .open
            ? [.plan, .notes, .links, .sources, .help] : [.plan, .notes, .links]
    }
    var titleKey: LocalizedStringKey {
        switch self {
        case .plan: "plan.title"
        case .notes: "notes.title"
        case .links: "links.title"
        case .sources: "case.sources"
        case .help: "case.help"
        }
    }
}

struct CaseDetailView: View {
    @Bindable var model: AssistantModel
    let chatGPT: ChatGPTModel
    let snapshot: MemorySnapshot
    @Binding var linkTarget: UInt64?
    @Binding var deletion: CaseDeletion?
    @Binding var tab: CaseTab
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            editor
            if let task = snapshot.selectedTask, !model.isCreatingTask {
                VStack(alignment: .leading, spacing: 16) {
                    CaseHeader(model: model, task: task, deletion: $deletion)
                    Picker(
                        selection: Binding(get: { shownTab(task) }, set: { tab = $0 })
                    ) {
                        ForEach(CaseTab.tabs(for: task), id: \.self) { item in
                            Text(item.titleKey, bundle: .module).tag(item)
                        }
                    } label: {
                        Text("case.sections", bundle: .module)
                    }
                    .pickerStyle(.segmented).labelsHidden()
                    switch shownTab(task) {
                    case .plan: ManualPlanView(model: model, snapshot: snapshot)
                    case .notes:
                        Button {
                            Task { await model.requestSelection(.newNote) }
                        } label: {
                            Text("notes.add", bundle: .module)
                        }.disabled(task.lifecycle == .completed)
                        notes
                    case .links: links
                    case .sources: sources(task)
                    case .help:
                        SelectedAssistanceView(
                            model: model, chatGPT: chatGPT, task: task,
                            notes: snapshot.evidence.items)
                    }
                }
                // Assistance availability follows the open case, whichever tab is shown.
                .task(id: task.lifecycle == .open ? task.id : nil) {
                    if task.lifecycle == .open { await chatGPT.reloadAssistanceAvailability() }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    /// A completed case has no sources or help; its hidden tab shows the plan.
    private func shownTab(_ task: MemoryTask) -> CaseTab {
        CaseTab.tabs(for: task).contains(tab) ? tab : .plan
    }
    @ViewBuilder private func sources(_ task: MemoryTask) -> some View {
        if model.integrations.isEnabled(.localProjects) {
            LocalProjectsView(
                model: model, chatGPT: chatGPT, task: task,
                projects: model.localProjects)
        } else {
            IntegrationDisabledView(kind: .localProjects)
        }
        if model.integrations.isEnabled(.codex) {
            CodexSourcesView(
                model: model, chatGPT: chatGPT, task: task,
                source: model.codexSources)
        } else {
            IntegrationDisabledView(kind: .codex)
        }
        if model.integrations.isEnabled(.github) {
            IntegrationSourcesView(
                model: model, chatGPT: chatGPT, task: task,
                source: model.githubSources)
        } else {
            IntegrationDisabledView(kind: .github)
        }
        if model.integrations.isEnabled(.jira) {
            IntegrationSourcesView(
                model: model, chatGPT: chatGPT, task: task,
                source: model.jiraSources)
        } else {
            IntegrationDisabledView(kind: .jira)
        }
        if model.integrations.isEnabled(.todoist) {
            IntegrationSourcesView(
                model: model, chatGPT: chatGPT, task: task,
                source: model.todoistSources)
        } else {
            IntegrationDisabledView(kind: .todoist)
        }
    }
    private var editor: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(LocalizedStringKey(model.editorKey), bundle: .module).font(.headline)
            TextEditor(text: $model.draft).font(.body).frame(minHeight: 120).padding(6)
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(.secondary.opacity(0.3)))
                .accessibilityLabel(
                    Text(LocalizedStringKey(model.editorKey), bundle: .module))
            PlanDraftFields(model: model)
            if model.hasUnsavedChanges {
                Text("intent.unsaved", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button {
                    Task { await model.save() }
                } label: {
                    Text(
                        model.isCompletingTask
                            ? "lifecycle.confirmComplete"
                            : model.isEditingNote ? "notes.save" : "intent.save", bundle: .module)
                }
                .buttonStyle(.borderedProminent).disabled(
                    model.draft.isEmpty || !model.hasUnsavedChanges)
                Button {
                    model.discardChanges()
                } label: {
                    Text("intent.discard", bundle: .module)
                }
                .disabled(!model.hasUnsavedChanges && !model.isEditingNote && !model.isCreatingTask)
            }
        }
    }
    private var notes: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("notes.title", bundle: .module).font(.title3.bold())
            if snapshot.evidence.items.isEmpty {
                Text("notes.empty", bundle: .module).foregroundStyle(.secondary)
            }
            ForEach(snapshot.evidence.items) { note in
                EvidenceRow(
                    note: note, targets: snapshot.tasks.items.filter { $0.id != note.taskID },
                    disabled: model.hasUnsavedChanges,
                    edit: { Task { await model.requestSelection(.note(note.id)) } },
                    delete: { deletion = .note(note.id) },
                    move: { target in Task { await model.moveNote(note.id, to: target) } })
            }
            PageControls(
                first: model.evidenceAfter == 0
                    ? nil : { Task { await model.pageNotes(after: 0) } },
                next: snapshot.evidence.nextAfter.map { next in
                    { Task { await model.pageNotes(after: next) } }
                })
            ForEach(snapshot.decisions.items) { decision in
                HStack {
                    Text("decision.moved", bundle: .module)
                    Text(
                        decision.accepted ? "notes.confirmation" : "links.rejected", bundle: .module
                    )
                }.font(.caption).foregroundStyle(.secondary)
            }
            PageControls(
                next: snapshot.decisions.nextAfter.map { next in
                    { Task { await model.pageDecisions(after: next) } }
                })
        }
    }
    private var links: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("links.title", bundle: .module).font(.title3.bold())
            if snapshot.links.items.isEmpty {
                Text("links.empty", bundle: .module).foregroundStyle(.secondary)
            }
            ForEach(snapshot.links.items) { link in
                HStack {
                    Button {
                        Task { await model.requestSelection(.task(link.otherTaskID)) }
                    } label: {
                        Text(verbatim: link.otherTask.text).lineLimit(2)
                    }
                    Spacer()
                    if link.related {
                        Button {
                            Task { await model.removeLink(link) }
                        } label: {
                            Text("links.remove", bundle: .module)
                        }
                        .disabled(model.hasUnsavedChanges)
                    } else {
                        Text("links.rejected", bundle: .module).font(.caption).foregroundStyle(
                            .secondary)
                    }
                }
            }
            PageControls(
                first: model.linksAfter == 0 ? nil : { Task { await model.pageLinks(after: 0) } },
                next: snapshot.links.nextAfter.map { next in
                    { Task { await model.pageLinks(after: next) } }
                })
            Picker(selection: $linkTarget) {
                Text("links.target", bundle: .module).tag(Optional<UInt64>.none)
                ForEach(
                    snapshot.tasks.items.filter {
                        $0.id != snapshot.selectedTask?.id && $0.lifecycle == .open
                    }
                ) {
                    task in
                    Text(verbatim: String(task.text.prefix(80))).tag(Optional(task.id))
                }
            } label: {
                Text("links.target", bundle: .module)
            }
            Button {
                if let target = snapshot.tasks.items.first(where: { $0.id == linkTarget }) {
                    Task { await model.link(to: target, related: true) }
                }
            } label: {
                Text("links.add", bundle: .module)
            }
            .disabled(
                model.hasUnsavedChanges
                    || !snapshot.tasks.items.contains {
                        $0.id == linkTarget && $0.id != snapshot.selectedTask?.id
                    })
        }
    }
}

/// The case title, its full selectable text, the lifecycle dot and the «⋯» menu of the existing
/// case actions (ADR-023 #13) with their previous guards.
private struct CaseHeader: View {
    let model: AssistantModel
    let task: MemoryTask
    @Binding var deletion: CaseDeletion?
    @State private var confirmReopen = false
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(task.lifecycle == .open ? Theme.ready.text : Color.secondary)
                            .frame(width: 6, height: 6).accessibilityHidden(true)
                        Text(
                            task.lifecycle == .completed ? "lifecycle.completed" : "lifecycle.open",
                            bundle: .module)
                    }
                    .font(.caption).foregroundStyle(.secondary)
                    Text(verbatim: task.text).font(.system(size: 26, weight: .semibold))
                        .lineLimit(1).truncationMode(.tail).help(task.text)
                        .accessibilityAddTraits(.isHeader)
                }
                Spacer()
                actions
            }
            // The title above already reads the full text to VoiceOver.
            Text(verbatim: task.text).foregroundStyle(.secondary).textSelection(.enabled)
                .accessibilityHidden(true)
            if task.initiativeSuppressed {
                Text("lifecycle.suppressed", bundle: .module).font(.caption)
            }
        }
        .confirmationDialog(
            Text("lifecycle.reopen", bundle: .module), isPresented: $confirmReopen,
            titleVisibility: .visible
        ) {
            Button {
                Task { await model.reopenTask() }
            } label: {
                Text("lifecycle.confirmReopen", bundle: .module)
            }
            Button(role: .cancel) {
            } label: {
                Text("switch.cancel", bundle: .module)
            }
        } message: {
            Text("lifecycle.reopenWarning", bundle: .module)
        }
    }
    private var actions: some View {
        Menu {
            Button {
                Task { await model.requestSelection(.task(task.id)) }
            } label: {
                Text("intent.edit", bundle: .module)
            }
            if task.lifecycle == .completed {
                Button {
                    confirmReopen = true
                } label: {
                    Text("lifecycle.reopen", bundle: .module)
                }
                .disabled(model.hasUnsavedChanges)
            } else {
                Button {
                    Task { await model.requestSelection(.completion) }
                } label: {
                    Text("lifecycle.complete", bundle: .module)
                }
            }
            Button {
                Task { await model.toggleInitiative() }
            } label: {
                Text(
                    task.initiativeSuppressed ? "lifecycle.unsuppress" : "lifecycle.suppress",
                    bundle: .module)
            }.disabled(model.hasUnsavedChanges)
            Divider()
            Button(role: .destructive) {
                deletion = .task
            } label: {
                Text("intent.delete", bundle: .module)
            }
            .disabled(model.hasUnsavedChanges)
        } label: {
            Image(systemName: "ellipsis")
        }
        .menuStyle(.button).menuIndicator(.hidden)
        .buttonStyle(.bordered).buttonBorderShape(.circle)
        .accessibilityLabel(Text("case.actions", bundle: .module))
    }
}

private struct EvidenceRow: View {
    let note: MemoryEvidence
    let targets: [MemoryTask]
    let disabled: Bool
    let edit: () -> Void
    let delete: () -> Void
    let move: (MemoryTask) -> Void
    @State private var targetID: UInt64?
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(verbatim: note.text).textSelection(.enabled)
            HStack {
                Text(kindKey, bundle: .module)
                Text(completenessKey, bundle: .module)
            }.font(.caption).foregroundStyle(.secondary)
            if let time = note.observedAtUTC {
                HStack {
                    Text("notes.time", bundle: .module)
                    Text(
                        Date(timeIntervalSince1970: TimeInterval(time)),
                        format: .dateTime.year().month().day().hour().minute())
                }.font(.caption)
            } else {
                Text("notes.unknownTime", bundle: .module).font(.caption)
            }
            switch note.origin {
            case .user: Text("notes.user", bundle: .module).font(.caption)
            case .external(let key, let version):
                if IntegrationKind(namespace: key.namespace) != nil {
                    Text("integration.history", bundle: .module).font(.caption)
                }
                DisclosureGroup {
                    LabeledContent {
                        Text(verbatim: key.namespace).textSelection(.enabled)
                    } label: {
                        Text("sources.namespace", bundle: .module)
                    }
                    LabeledContent {
                        Text(verbatim: key.scope).textSelection(.enabled)
                    } label: {
                        Text("sources.scope", bundle: .module)
                    }
                    LabeledContent {
                        Text(verbatim: key.objectID).textSelection(.enabled)
                    } label: {
                        Text("sources.object", bundle: .module)
                    }
                    if let version {
                        LabeledContent {
                            Text(verbatim: version).textSelection(.enabled)
                        } label: {
                            Text("sources.version", bundle: .module)
                        }
                    } else {
                        Text("sources.unknownVersion", bundle: .module)
                    }
                } label: {
                    Text("notes.external", bundle: .module)
                }
            }
            HStack {
                Button(action: edit) { Text("notes.edit", bundle: .module) }
                Button(role: .destructive, action: delete) { Text("notes.delete", bundle: .module) }
                    .disabled(disabled)
            }
            Picker(selection: $targetID) {
                Text("notes.move", bundle: .module).tag(Optional<UInt64>.none)
                ForEach(targets) { task in
                    Text(verbatim: String(task.text.prefix(80))).tag(Optional(task.id))
                }
            } label: {
                Text("notes.move", bundle: .module)
            }
            Button {
                if let target = targets.first(where: { $0.id == targetID }) { move(target) }
            } label: {
                Text("notes.moveAction", bundle: .module)
            }
            .disabled(disabled || !targets.contains { $0.id == targetID })
        }
        .padding().frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
    }
    private var kindKey: LocalizedStringKey {
        switch note.kind {
        case .sourceFact: "notes.fact"
        case .assumption: "notes.assumption"
        case .userConfirmation: "notes.confirmation"
        }
    }
    private var completenessKey: LocalizedStringKey {
        switch note.completeness {
        case .unknown: "notes.unknown"
        case .partial: "notes.partial"
        case .complete: "notes.complete"
        }
    }
}
