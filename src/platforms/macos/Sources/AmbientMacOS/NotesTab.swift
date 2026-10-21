import SwiftUI

/// The case's notes: one row per note with its «⋯» menu, then the note and move-decision pages.
struct NotesTab: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    let task: MemoryTask
    @Binding var deletion: CaseDeletion?
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("notes.title", bundle: .module).font(.title3.bold())
                Spacer()
                Button {
                    Task { await model.requestSelection(.newNote) }
                } label: {
                    Label {
                        Text("notes.add", bundle: .module)
                    } icon: {
                        Image(systemName: "plus")
                    }
                }
                .buttonStyle(.bordered).buttonBorderShape(.capsule)
                .disabled(task.lifecycle == .completed)
            }
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
                Divider()
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
}

/// A note with its metadata line; edit, move and delete live in the «⋯» menu with their previous
/// guards, and «Move to task» opens the previous target picker and confirmation in the row.
private struct EvidenceRow: View {
    let note: MemoryEvidence
    let targets: [MemoryTask]
    let disabled: Bool
    let edit: () -> Void
    let delete: () -> Void
    let move: (MemoryTask) -> Void
    @State private var moving = false
    @State private var targetID: UInt64?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top) {
                Text(verbatim: note.text).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                actions
            }
            metadata.font(.caption).foregroundStyle(.secondary)
            if case .external(let key, let version) = note.origin {
                if IntegrationKind(namespace: key.namespace) != nil {
                    Text("integration.history", bundle: .module).font(.caption)
                        .foregroundStyle(.secondary)
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
                .font(.caption)
            }
            if moving {
                HStack {
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
                    .buttonStyle(.borderedProminent)
                    .disabled(disabled || !targets.contains { $0.id == targetID })
                    Button {
                        moving = false
                    } label: {
                        Text("switch.cancel", bundle: .module)
                    }
                }
                .buttonBorderShape(.capsule).controlSize(.small)
            }
        }
        .padding(.vertical, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    /// Kind · completeness · time · origin, as the design's metadata line; a column without the
    /// separators when the line does not fit.
    private var metadata: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 4) { metadataParts(separated: true) }
            VStack(alignment: .leading, spacing: 2) { metadataParts(separated: false) }
        }
    }
    @ViewBuilder private func metadataParts(separated: Bool) -> some View {
        let separator = Text(verbatim: "·").accessibilityHidden(true)
        Text(kindKey, bundle: .module).fontWeight(.medium)
        if separated { separator }
        Text(completenessKey, bundle: .module)
        if separated { separator }
        if let time = note.observedAtUTC {
            HStack(spacing: 4) {
                Text("notes.time", bundle: .module)
                Text(
                    Date(timeIntervalSince1970: TimeInterval(time)),
                    format: .dateTime.year().month().day().hour().minute())
            }
        } else {
            Text("notes.unknownTime", bundle: .module)
        }
        if case .user = note.origin {
            if separated { separator }
            Text("notes.user", bundle: .module)
        }
    }
    private var actions: some View {
        Menu {
            Button(action: edit) { Text("notes.edit", bundle: .module) }
            Button {
                moving = true
            } label: {
                Text("notes.move", bundle: .module)
            }
            .disabled(disabled)
            Divider()
            Button(role: .destructive, action: delete) { Text("notes.delete", bundle: .module) }
                .disabled(disabled)
        } label: {
            Image(systemName: "ellipsis")
        }
        .menuStyle(.button).menuIndicator(.hidden)
        .buttonStyle(.bordered).buttonBorderShape(.circle).controlSize(.small)
        .accessibilityLabel(Text("notes.actions", bundle: .module))
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
