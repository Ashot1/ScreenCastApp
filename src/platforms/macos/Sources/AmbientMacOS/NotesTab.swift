import SwiftUI

/// The case's notes (page 5): one row per note with its metadata line and «⋯» menu; further pages
/// and the move log sit under «Ещё заметки и журнал переноса». «+» lives next to the case tabs.
struct NotesTab: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    @Binding var deletion: CaseDeletion?
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
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
                if note.id != snapshot.evidence.items.last?.id { Divider() }
            }
            if model.evidenceAfter != 0 || snapshot.evidence.nextAfter != nil
                || !snapshot.decisions.items.isEmpty
            {
                DisclosureGroup {
                    VStack(alignment: .leading, spacing: 8) {
                        PageControls(
                            first: model.evidenceAfter == 0
                                ? nil : { Task { await model.pageNotes(after: 0) } },
                            next: snapshot.evidence.nextAfter.map { next in
                                { Task { await model.pageNotes(after: next) } }
                            })
                        ForEach(snapshot.decisions.items) { decision in
                            HStack(spacing: 4) {
                                Text("decision.moved", bundle: .module)
                                Text(verbatim: "·").accessibilityHidden(true)
                                Text(
                                    decision.accepted ? "notes.confirmation" : "links.rejected",
                                    bundle: .module)
                            }
                        }
                        PageControls(
                            next: snapshot.decisions.nextAfter.map { next in
                                { Task { await model.pageDecisions(after: next) } }
                            })
                    }
                    .font(.caption).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 6)
                } label: {
                    Text("notes.more", bundle: .module).font(.callout)
                }
                .padding(.top, 8)
            }
        }
    }
}

/// A note with its metadata line; edit, move and delete live in the «⋯» menu with their previous
/// guards, and «Перенести в дело…» opens the previous target picker and confirmation in the row.
private struct EvidenceRow: View {
    let note: MemoryEvidence
    let targets: [MemoryTask]
    let disabled: Bool
    let edit: () -> Void
    let delete: () -> Void
    let move: (MemoryTask) -> Void
    @State private var moving = false
    @State private var targetID: UInt64?
    @State private var showsOrigin = false
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top) {
                Text(verbatim: note.text).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                actions
            }
            metadata.font(.caption).foregroundStyle(.secondary)
            if showsOrigin, case .external(let key, let version) = note.origin {
                origin(key, version).font(.caption).padding(10).ambientInset(cornerRadius: 10)
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
                    .labelsHidden()
                    Button {
                        if let target = targets.first(where: { $0.id == targetID }) { move(target) }
                    } label: {
                        Text("notes.moveAction", bundle: .module)
                    }
                    .ambientPrimary()
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
        .padding(.vertical, 2)
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
        Text(kindKey, bundle: .module).fontWeight(.semibold).foregroundStyle(.primary)
        if separated { separator }
        Text(completenessKey, bundle: .module)
        if separated { separator }
        if let time = note.observedAtUTC {
            Text(
                Date(timeIntervalSince1970: TimeInterval(time)),
                format: .dateTime.day().month().hour().minute())
        } else {
            Text("notes.unknownTime", bundle: .module)
        }
        if separated { separator }
        if case .user = note.origin {
            Text("notes.user", bundle: .module)
        } else {
            // The note carries no address to open; the link shows its saved source details.
            Button {
                showsOrigin.toggle()
            } label: {
                if let source = note.source {
                    Text("notes.fromHistory \(brand(source))", bundle: .module)
                } else {
                    Text("notes.external", bundle: .module)
                }
            }
            .buttonStyle(.link)
            .accessibilityValue(
                Text(showsOrigin ? "notes.detailsShown" : "notes.detailsHidden", bundle: .module))
        }
    }
    @ViewBuilder private func origin(_ key: MemorySourceKey, _ version: String?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if IntegrationKind(namespace: key.namespace) != nil {
                Text("integration.history", bundle: .module).foregroundStyle(.secondary)
            }
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
    /// The product whose history saved the note; local projects are read from Git.
    private func brand(_ source: BuiltInIntegration) -> String {
        switch source {
        case .localProjects: "Git"
        case .codex: "Codex"
        case .github: "GitHub"
        case .jira: "Jira"
        case .todoist: "Todoist"
        }
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
