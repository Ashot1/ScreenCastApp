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
        case .links: "case.links"
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
        if shownTab(task) == .plan, task.lifecycle == .open, let step = model.nextStep {
            NextStepBlock(model: model, step: step)
        }
        HStack {
            Picker(
                selection: Binding(get: { shownTab(task) }, set: { tab = $0 })
            ) {
                ForEach(CaseTab.tabs(for: task), id: \.self) { item in
                    tabTitle(item).tag(item)
                }
            } label: {
                Text("case.sections", bundle: .module)
            }
            .pickerStyle(.segmented).labelsHidden().fixedSize()
            Spacer()
            if shownTab(task) == .notes {
                Button {
                    Task { await model.requestSelection(.newNote) }
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.glass).buttonBorderShape(.circle).controlSize(.large)
                .accessibilityLabel(Text("notes.add", bundle: .module))
                .help(Text("notes.add", bundle: .module))
                .disabled(task.lifecycle == .completed)
            }
        }
        switch shownTab(task) {
        case .plan: ManualPlanView(model: model, snapshot: snapshot)
        case .notes: NotesTab(model: model, snapshot: snapshot, deletion: $deletion)
        case .links: LinksTab(model: model, snapshot: snapshot, linkTarget: $linkTarget)
        case .sources: SourcesTab(model: model, chatGPT: chatGPT, task: task, source: $source)
        case .help:
            SelectedAssistanceView(
                model: model, chatGPT: chatGPT, task: task,
                notes: snapshot.evidence.items)
        }
    }
    /// «Заметки 7», «Связи 3»: a count only when the whole list is loaded.
    private func tabTitle(_ item: CaseTab) -> Text {
        let count: Int? =
            switch item {
            case .notes: snapshot.evidence.wholeCount(fromStart: model.evidenceAfter == 0)
            case .links: snapshot.links.wholeCount(fromStart: model.linksAfter == 0)
            case .plan, .sources, .help: nil
            }
        let title = Text(item.titleKey, bundle: .module)
        guard let count else { return title }
        // Interpolated text, not a table key: the count is a plain number after the title.
        return Text("\(title) \(Text(verbatim: String(count)).foregroundStyle(.secondary))")
    }
    /// A completed case has no sources or help; its hidden tab shows the plan.
    private func shownTab(_ task: MemoryTask) -> CaseTab {
        CaseTab.tabs(for: task).contains(tab) ? tab : .plan
    }
}

/// The case status, its title (full, selectable text) and the case actions with their previous
/// guards (ADR-023 #13): «Открыть снова» on a completed case, the rest in the «⋯» menu.
private struct CaseHeader: View {
    let model: AssistantModel
    let task: MemoryTask
    @Binding var deletion: CaseDeletion?
    @Binding var editing: Bool
    @State private var confirmReopen = false
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 8) {
                status
                Text(verbatim: task.text).font(.system(size: 26, weight: .semibold))
                    .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
            }
            Spacer(minLength: 12)
            if task.lifecycle == .completed {
                Button {
                    confirmReopen = true
                } label: {
                    Text("lifecycle.reopen", bundle: .module)
                }
                .buttonStyle(.bordered).buttonBorderShape(.capsule)
                .disabled(model.hasUnsavedChanges)
            }
            actions
        }
        .confirmationDialog(
            Text("lifecycle.reopenTitle", bundle: .module), isPresented: $confirmReopen,
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
            Text("lifecycle.reopenWarning \(task.text)", bundle: .module)
        }
        .dialogIcon(Image(systemName: "arrow.clockwise"))
    }
    private var status: some View {
        HStack(spacing: 6) {
            if task.lifecycle == .open {
                Circle().fill(Theme.ready.text).frame(width: 6, height: 6)
                    .accessibilityHidden(true)
                Text("case.open", bundle: .module)
            } else {
                Pill(text: Text("case.completed", bundle: .module))
            }
            if task.initiativeSuppressed {
                if task.lifecycle == .open { Text(verbatim: "·").accessibilityHidden(true) }
                Text("case.remindersOff", bundle: .module)
            }
        }
        .font(.caption).foregroundStyle(.secondary)
    }
    private var actions: some View {
        Menu {
            Button {
                editing = true
            } label: {
                Text("intent.edit", bundle: .module)
            }
            if task.lifecycle == .open {
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
            }
            .help(Text("lifecycle.suppressionHelp", bundle: .module))
            .disabled(model.hasUnsavedChanges)
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
        .buttonStyle(.glass).buttonBorderShape(.circle).controlSize(.large)
        .accessibilityLabel(Text("case.actions", bundle: .module))
    }
}
