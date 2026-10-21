import SwiftUI

public struct AssistantView: View {
    @Bindable private var model: AssistantModel
    private let chatGPT: ChatGPTModel
    @State private var linkTarget: UInt64?
    @State private var deletion: Deletion?
    private enum Deletion {
        case task
        case note(UInt64)
    }
    public init(model: AssistantModel, chatGPT: ChatGPTModel) {
        self.model = model
        self.chatGPT = chatGPT
    }
    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack {
                    Text("app.name", bundle: .module).font(.largeTitle.bold())
                    Spacer()
                    SettingsLink { Text("settings.title", bundle: .module) }
                }
                AssistantStatusView(model: model)
                if !chatGPT.feed.isEmpty || !chatGPT.replyDraft.isEmpty
                    || chatGPT.assisting
                {
                    GroupBox { AssistanceCardView(model: model, chatGPT: chatGPT) }
                }
                if let failure = model.failure {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("error.title", bundle: .module).font(.headline)
                        Text(failure.message)
                        Button {
                            Task { await model.reload() }
                        } label: {
                            Text("error.retry", bundle: .module)
                        }
                    }
                    .padding().frame(maxWidth: .infinity, alignment: .leading)
                    .background(.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
                }
                if let snapshot = model.snapshot {
                    memory(snapshot)
                } else if model.busy {
                    ProgressView { Text("loading", bundle: .module) }
                }
            }
            .padding(24)
        }
        .frame(minWidth: 740, minHeight: 640)
        .disabled(model.busy || model.closing)
        .task {
            if model.snapshot == nil { await model.reload() }
            await model.integrations.load()
        }
        .onChange(of: model.isCreatingTask ? nil : model.snapshot?.selectedTask) { _, _ in
            chatGPT.selectedTaskChanged()
        }
        .onChange(of: model.manualPause) { _, _ in
            Task {
                await chatGPT.resetAssistance()
                await chatGPT.reloadAssistanceAvailability()
            }
        }
        .alert(
            Text("switch.title", bundle: .module),
            isPresented: Binding(
                get: { model.pendingSelection != nil }, set: { if !$0 { model.cancelSelection() } }
            )
        ) {
            Button {
                let accepted = model.pendingSelection
                Task { await model.saveAndSwitch(to: accepted) }
            } label: {
                Text(
                    model.isCompletingTask ? "lifecycle.confirmComplete" : "switch.save",
                    bundle: .module)
            }
            Button(role: .destructive) {
                let accepted = model.pendingSelection
                Task { await model.discardAndSwitch(to: accepted) }
            } label: {
                Text("switch.discard", bundle: .module)
            }
            Button(role: .cancel) {
                model.cancelSelection()
            } label: {
                Text("switch.cancel", bundle: .module)
            }
        } message: {
            Text(model.isCompletingTask ? "lifecycle.cleanup" : "switch.message", bundle: .module)
        }
        .confirmationDialog(
            Text("delete.title", bundle: .module),
            isPresented: Binding(
                get: { deletion != nil }, set: { if !$0 { deletion = nil } }
            ), titleVisibility: .visible
        ) {
            Button(role: .destructive) {
                let accepted = deletion
                deletion = nil
                Task {
                    switch accepted {
                    case .task: await model.deleteTask()
                    case .note(let id): await model.deleteNote(id)
                    case nil: break
                    }
                }
            } label: {
                Text("delete.confirm", bundle: .module)
            }
            Button(role: .cancel) {
                deletion = nil
            } label: {
                Text("switch.cancel", bundle: .module)
            }
        } message: {
            Text("delete.message", bundle: .module)
        }
    }
    private func memory(_ snapshot: MemorySnapshot) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("memory.title", bundle: .module).font(.title2.bold())
            HStack(alignment: .top, spacing: 24) {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text("tasks.list", bundle: .module).font(.headline)
                        Spacer()
                        Button {
                            Task { await model.requestSelection(.newTask) }
                        } label: {
                            Text("tasks.new", bundle: .module)
                        }
                    }
                    if snapshot.tasks.items.isEmpty {
                        Text("tasks.empty", bundle: .module).foregroundStyle(.secondary)
                    }
                    ForEach(snapshot.tasks.items) { task in
                        Button {
                            Task { await model.requestSelection(.task(task.id)) }
                        } label: {
                            HStack(alignment: .top) {
                                Text(verbatim: task.text).lineLimit(2).frame(
                                    maxWidth: .infinity, alignment: .leading)
                                if snapshot.selectedTask?.id == task.id && !model.isCreatingTask {
                                    Image(systemName: "checkmark").accessibilityLabel(
                                        Text("tasks.selected", bundle: .module))
                                }
                            }
                        }
                        .buttonStyle(.bordered)
                    }
                    HStack {
                        Button {
                            Task { await model.pageTasks(after: 0) }
                        } label: {
                            Text("tasks.first", bundle: .module)
                        }
                        if let next = snapshot.tasks.nextAfter {
                            Button {
                                Task { await model.pageTasks(after: next) }
                            } label: {
                                Text("tasks.next", bundle: .module)
                            }
                        }
                    }
                }
                .frame(width: 240)
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
                        notes(snapshot)
                        Divider()
                        links(snapshot)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            Text("memory.help", bundle: .module).font(.caption).foregroundStyle(.secondary)
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
    private func notes(_ snapshot: MemorySnapshot) -> some View {
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
            HStack {
                Button {
                    Task { await model.pageNotes(after: 0) }
                } label: {
                    Text("tasks.first", bundle: .module)
                }
                if let next = snapshot.evidence.nextAfter {
                    Button {
                        Task { await model.pageNotes(after: next) }
                    } label: {
                        Text("tasks.next", bundle: .module)
                    }
                }
            }
            ForEach(snapshot.decisions.items) { decision in
                HStack {
                    Text("decision.moved", bundle: .module)
                    Text(
                        decision.accepted ? "notes.confirmation" : "links.rejected", bundle: .module
                    )
                }.font(.caption).foregroundStyle(.secondary)
            }
            if let next = snapshot.decisions.nextAfter {
                Button {
                    Task { await model.pageDecisions(after: next) }
                } label: {
                    Text("tasks.next", bundle: .module)
                }
            }
        }
    }
    private func links(_ snapshot: MemorySnapshot) -> some View {
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
            HStack {
                Button {
                    Task { await model.pageLinks(after: 0) }
                } label: {
                    Text("tasks.first", bundle: .module)
                }
                if let next = snapshot.links.nextAfter {
                    Button {
                        Task { await model.pageLinks(after: next) }
                    } label: {
                        Text("tasks.next", bundle: .module)
                    }
                }
            }
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

struct AssistantStatusView: View {
    let model: AssistantModel
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label {
                Text(LocalizedStringKey(model.statusKey), bundle: .module)
            } icon: {
                Image(systemName: "pause.circle.fill")
            }.font(.headline)
            Text("status.cloudUnavailable", bundle: .module)
            AssistantPauseButton(model: model)
            if model.manualPause == nil {
                Button {
                    Task { await model.refreshManualPause() }
                } label: {
                    Text("pause.read", bundle: .module)
                }
                .disabled(model.busy || model.closing)
            }
            Text("pause.help", bundle: .module).font(.caption).foregroundStyle(.secondary)
        }
        .padding().frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
        .task { await model.loadManualPause() }
    }
}
