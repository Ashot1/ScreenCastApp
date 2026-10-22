import SwiftUI

/// The main window's content section; a case follows the model's selection (ADR-023).
enum MainSection { case suggestions, task }

struct CaseSidebar: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    @Binding var section: MainSection
    @State private var completedExpanded = false
    var body: some View {
        // Rows are buttons, not `List` selection: the system selection fill is the accent color
        // whenever the list has focus, and the design's selected row is white (26B).
        List {
            Button {
                section = .suggestions
            } label: {
                // Not a `Label`: the sidebar draws label icons in the accent color.
                HStack(spacing: 8) {
                    Image(systemName: "tray").accessibilityHidden(true)
                    Text("feed.title", bundle: .module)
                }
                .fontWeight(section == .suggestions ? .semibold : nil)
                .frame(maxWidth: .infinity, alignment: .leading).contentShape(.rect)
            }
            .buttonStyle(.plain)
            .badge(chatGPT.feed.count)
            .accessibilityAddTraits(section == .suggestions ? .isSelected : [])
            // A plain row, not a section header: a sidebar section header is collapsible and its
            // single accessibility element would hide the «+» button.
            HStack {
                Text("tasks.list", bundle: .module).font(.title2.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                Button(action: newTask) {
                    Label {
                        Text("tasks.new", bundle: .module)
                    } icon: {
                        Image(systemName: "plus")
                    }
                }
                .labelStyle(.iconOnly).buttonStyle(.glass).buttonBorderShape(.circle)
                .help(Text("tasks.new", bundle: .module))
            }
            .padding(.top, 12)
            if let snapshot = model.snapshot {
                ForEach(snapshot.openTasks, content: taskRow)
                if !snapshot.completedTasks.isEmpty {
                    DisclosureGroup(isExpanded: completedExpansion(snapshot)) {
                        ForEach(snapshot.completedTasks, content: taskRow)
                    } label: {
                        HStack(spacing: 6) {
                            Text("tasks.completed", bundle: .module)
                            Text(snapshot.completedTasks.count, format: .number)
                        }
                        .font(.subheadline).foregroundStyle(.secondary)
                    }
                }
                PageControls(
                    first: model.tasksAfter == 0
                        ? nil : { Task { await model.pageTasks(after: 0) } },
                    next: snapshot.tasks.nextAfter.map { next in
                        { Task { await model.pageTasks(after: next) } }
                    }
                )
            } else if model.busy {
                ProgressView { Text("loading", bundle: .module) }
            }
        }
        .listStyle(.sidebar)
        .toolbar(removing: .sidebarToggle)
        // Empty memory (F18): centered in the sidebar, outside the list rows.
        .overlay {
            if model.snapshot?.tasks.items.isEmpty == true {
                ContentUnavailableView {
                    Label {
                        Text("tasks.empty", bundle: .module)
                    } icon: {
                        Image(systemName: "doc")
                    }
                } description: {
                    Text("tasks.emptyHint", bundle: .module)
                } actions: {
                    Button(action: newTask) { Text("tasks.new", bundle: .module) }
                        .buttonStyle(.glass)
                }
            }
        }
        .safeAreaBar(edge: .bottom) {
            SidebarStatus(model: model).padding(10)
        }
        // The settings icon is part of the sidebar, on the traffic-light row (the window has no
        // toolbar). A top safe-area bar would sit below the titlebar, so the icon overlays it; the
        // overlay is only as large as the button and leaves the rest of the titlebar draggable.
        .overlay(alignment: .topTrailing) {
            SettingsLink {
                Label {
                    Text("settings.title", bundle: .module)
                } icon: {
                    Image(systemName: "gearshape")
                }
            }
            .labelStyle(.iconOnly).buttonStyle(.borderless)
            .help(Text("settings.title", bundle: .module))
            .padding(.trailing, 14).padding(.top, 10)
            .ignoresSafeArea()
        }
    }
    /// The open new-task editor is only shown again; inside it the old path asks about the
    /// unsaved draft.
    private func newTask() {
        if model.isCreatingTask && section == .suggestions {
            section = .task
        } else {
            Task { await model.requestSelection(.newTask) }
        }
    }
    /// The case open in the model; none while a new task is created.
    private var openTaskID: UInt64? {
        model.isCreatingTask ? nil : model.snapshot?.selectedTask?.id
    }
    /// ADR-023: no case is shown as selected while «Suggestions» or a new task is shown.
    private var shownTaskID: UInt64? { section == .suggestions ? nil : openTaskID }
    /// Choosing another task goes through the model, so the unsaved-changes alert decides and
    /// «Cancel» keeps the previous row and section.
    private func select(_ id: UInt64) {
        if id == openTaskID {
            section = .task
        } else {
            Task { await model.requestSelection(.task(id)) }
        }
    }
    /// The group opens by itself while its row is selected, so the selection is never hidden.
    private func completedExpansion(_ snapshot: MemorySnapshot) -> Binding<Bool> {
        Binding(
            get: {
                completedExpanded
                    || snapshot.completedTasks.contains { $0.id == shownTaskID }
            },
            set: { completedExpanded = $0 })
    }
    /// The edited row stays marked while another section is shown.
    private func taskRow(_ task: MemoryTask) -> some View {
        let shown = task.id == shownTaskID
        let unsaved = task.id == openTaskID && model.hasUnsavedChanges
        return Button {
            select(task.id)
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: task.text).lineLimit(2)
                if unsaved {
                    Text("intent.unsaved", bundle: .module).font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading).contentShape(.rect)
        }
        .buttonStyle(.plain)
        .listRowBackground(
            shown
                ? RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(Color(nsColor: .textBackgroundColor)).padding(.horizontal, 10)
                : nil
        )
        .accessibilityLabel(Text(verbatim: task.text))
        .accessibilityValue(unsaved ? Text("intent.unsaved", bundle: .module) : Text(verbatim: ""))
        .accessibilityAddTraits(shown ? .isSelected : [])
    }
}

/// The sidebar's compact status capsule: ring, a short status, and one round action — the pause
/// toggle, or reading the saved pause while it is unknown. The full texts stay in Settings.
struct SidebarStatus: View {
    let model: AssistantModel
    var body: some View {
        HStack(spacing: 8) {
            StatusRing(kind: model.statusKind)
            VStack(alignment: .leading, spacing: 1) {
                Text(
                    LocalizedStringKey(
                        model.statusKind == .assistance ? "status.help" : model.statusKey),
                    bundle: .module
                )
                .font(.subheadline.weight(.semibold))
                Text("status.unavailable", bundle: .module)
                    .font(.caption).foregroundStyle(.secondary)
            }
            .help(Text("status.cloudUnavailable", bundle: .module))
            Spacer(minLength: 4)
            Group {
                if model.manualPause == nil {
                    AssistantPauseReadButton(model: model)
                } else {
                    AssistantPauseButton(model: model)
                }
            }
            .labelStyle(.iconOnly).buttonStyle(.glass).buttonBorderShape(.circle)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(in: .rect(cornerRadius: 18))
        .accessibilityElement(children: .contain)
        .task { await model.loadManualPause() }
    }
}
