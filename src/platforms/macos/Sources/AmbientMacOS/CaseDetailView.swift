import SwiftUI

enum CaseDeletion {
    case task
    case note(UInt64)
}

struct CaseDetailView: View {
    @Bindable var model: AssistantModel
    let chatGPT: ChatGPTModel
    let snapshot: MemorySnapshot
    @Binding var linkTarget: UInt64?
    @Binding var deletion: CaseDeletion?
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            editor
            if let task = snapshot.selectedTask, !model.isCreatingTask {
                Text(verbatim: task.text).textSelection(.enabled)
                if task.lifecycle == .open {
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
                    SelectedAssistanceView(
                        model: model, chatGPT: chatGPT, task: task,
                        notes: snapshot.evidence.items)
                }
                HStack {
                    Button {
                        Task { await model.requestSelection(.newNote) }
                    } label: {
                        Text("notes.add", bundle: .module)
                    }.disabled(task.lifecycle == .completed)
                    Spacer()
                    Button(role: .destructive) {
                        deletion = .task
                    } label: {
                        Text("intent.delete", bundle: .module)
                    }
                    .disabled(model.hasUnsavedChanges)
                }
                Divider()
                ManualPlanView(model: model, snapshot: snapshot)
                Divider()
                notes
                Divider()
                links
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
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
