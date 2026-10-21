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
    @Binding var source: BuiltInIntegration
    @Binding var editingTask: Bool
    @Binding var editingStep: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            switch model.casePage {
            case .editor: DraftEditor(model: model)
            case .step(let step):
                if let task = snapshot.selectedTask {
                    StepPage(
                        model: model, snapshot: snapshot, task: task, step: step,
                        editing: $editingStep)
                }
            case .tabs:
                if let task = snapshot.selectedTask { tabs(task) }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        // Assistance availability follows the open case, whichever page or tab is shown.
        .task(id: openTaskID) {
            if openTaskID != nil { await chatGPT.reloadAssistanceAvailability() }
        }
    }
    private var openTaskID: UInt64? {
        snapshot.selectedTask.flatMap { $0.lifecycle == .open ? $0.id : nil }
    }
    @ViewBuilder private func tabs(_ task: MemoryTask) -> some View {
        CaseHeader(model: model, task: task, deletion: $deletion, editing: $editingTask)
        // The model already edits the case; the mode only shows the fields, and an unsaved draft
        // is always visible.
        if model.editorKind == .task && (editingTask || model.hasUnsavedChanges) {
            DraftEditor(model: model, titleFont: .title3.weight(.semibold)) { editingTask = false }
        }
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
        case .notes: NotesTab(model: model, snapshot: snapshot, task: task, deletion: $deletion)
        case .links: LinksTab(model: model, snapshot: snapshot, linkTarget: $linkTarget)
        case .sources: SourcesTab(model: model, chatGPT: chatGPT, task: task, source: $source)
        case .help:
            SelectedAssistanceView(
                model: model, chatGPT: chatGPT, task: task,
                notes: snapshot.evidence.items)
        }
    }
    /// A completed case has no sources or help; its hidden tab shows the plan.
    private func shownTab(_ task: MemoryTask) -> CaseTab {
        CaseTab.tabs(for: task).contains(tab) ? tab : .plan
    }
}

/// The case title, its full selectable text, the lifecycle dot and the «⋯» menu of the existing
/// case actions (ADR-023 #13) with their previous guards.
private struct CaseHeader: View {
    let model: AssistantModel
    let task: MemoryTask
    @Binding var deletion: CaseDeletion?
    @Binding var editing: Bool
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
                editing = true
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
