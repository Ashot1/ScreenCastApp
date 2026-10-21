import SwiftUI

/// The main window's content section; a case follows the model's selection (ADR-023).
enum MainSection { case suggestions, task }

struct CaseSidebar: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    @Binding var section: MainSection
    @State private var completedExpanded = false
    var body: some View {
        // A refused selection changes only `pendingSelection`; reading it re-renders the list, so
        // after «Cancel» the table returns to the model's row (probe R5, 25B).
        let _ = model.pendingSelection
        List(selection: selection) {
            Button {
                section = .suggestions
            } label: {
                Label {
                    Text("feed.title", bundle: .module)
                } icon: {
                    Image(systemName: "tray")
                }
                .fontWeight(section == .suggestions ? .semibold : nil)
            }
            .badge(chatGPT.feed.count)
            .accessibilityAddTraits(section == .suggestions ? .isSelected : [])
            .selectionDisabled()
            Section {
                Button {
                    // The open new-task editor is only shown again; inside it the old path asks
                    // about the unsaved draft.
                    if model.isCreatingTask && section == .suggestions {
                        section = .task
                    } else {
                        Task { await model.requestSelection(.newTask) }
                    }
                } label: {
                    row(
                        Label {
                            Text("tasks.new", bundle: .module)
                        } icon: {
                            Image(systemName: "plus")
                        }, unsaved: model.isCreatingTask
                    )
                    .fontWeight(model.isCreatingTask && section == .task ? .semibold : nil)
                }
                .selectionDisabled()
                if let snapshot = model.snapshot {
                    if snapshot.tasks.items.isEmpty {
                        ContentUnavailableView {
                            Label {
                                Text("tasks.empty", bundle: .module)
                            } icon: {
                                Image(systemName: "checklist")
                            }
                        }
                        .selectionDisabled()
                    }
                    ForEach(snapshot.openTasks, content: taskRow)
                } else if model.busy {
                    ProgressView { Text("loading", bundle: .module) }.selectionDisabled()
                }
            } header: {
                Text("tasks.list", bundle: .module)
            }
            if let snapshot = model.snapshot {
                if !snapshot.completedTasks.isEmpty {
                    Section(isExpanded: completedExpansion(snapshot)) {
                        ForEach(snapshot.completedTasks, content: taskRow)
                    } header: {
                        Text("tasks.completed", bundle: .module)
                    }
                }
                PageControls(
                    first: model.tasksAfter == 0
                        ? nil : { Task { await model.pageTasks(after: 0) } },
                    next: snapshot.tasks.nextAfter.map { next in
                        { Task { await model.pageTasks(after: next) } }
                    }
                )
                .selectionDisabled()
            }
        }
        .listStyle(.sidebar)
        .safeAreaBar(edge: .bottom) {
            GlassEffectContainer {
                VStack(alignment: .leading, spacing: 10) {
                    SettingsLink {
                        Label {
                            Text("settings.title", bundle: .module)
                        } icon: {
                            Image(systemName: "gearshape")
                        }
                    }
                    .buttonStyle(.glass)
                    SidebarStatus(model: model)
                }
            }
            .padding(10)
        }
    }
    /// ADR-023: nothing is selected while «Suggestions» or a new task is shown; choosing another
    /// task goes through the model, so the unsaved-changes alert decides and «Cancel» keeps the
    /// previous row and section.
    private var selection: Binding<UInt64?> {
        Binding(
            get: {
                section == .suggestions || model.isCreatingTask
                    ? nil : model.snapshot?.selectedTask?.id
            },
            set: { id in
                guard let id else { return }
                if id == model.snapshot?.selectedTask?.id && !model.isCreatingTask {
                    section = .task
                } else {
                    Task { await model.requestSelection(.task(id)) }
                }
            })
    }
    /// The group opens by itself while its row is selected, so the selection is never hidden.
    private func completedExpansion(_ snapshot: MemorySnapshot) -> Binding<Bool> {
        Binding(
            get: {
                completedExpanded
                    || snapshot.completedTasks.contains { $0.id == selection.wrappedValue }
            },
            set: { completedExpanded = $0 })
    }
    private func taskRow(_ task: MemoryTask) -> some View {
        row(
            Text(verbatim: task.text).lineLimit(2),
            unsaved: !model.isCreatingTask && model.snapshot?.selectedTask?.id == task.id
        )
        .tag(Optional(task.id))
    }
    /// The edited row stays marked while another section is shown.
    private func row(_ title: some View, unsaved: Bool) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            title
            if unsaved && model.hasUnsavedChanges {
                Text("intent.unsaved", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// The sidebar's status capsule: ring, the existing status texts and the pause controls.
private struct SidebarStatus: View {
    let model: AssistantModel
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 6) {
                StatusRing(kind: ring)
                Text(
                    LocalizedStringKey(model.closing ? "status.closing" : model.statusKey),
                    bundle: .module
                )
                .font(.subheadline.weight(.semibold))
            }
            Text("status.cloudUnavailable", bundle: .module)
                .font(.caption).foregroundStyle(.secondary)
            AssistantPauseButton(model: model).buttonStyle(.glass)
            if model.manualPause == nil {
                Button {
                    Task { await model.refreshManualPause() }
                } label: {
                    Text("pause.read", bundle: .module)
                }
                .buttonStyle(.glass)
            }
            Text("pause.help", bundle: .module).font(.caption).foregroundStyle(.secondary)
        }
        .controlSize(.small)
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.fill.quaternary, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .accessibilityElement(children: .contain)
        .task { await model.loadManualPause() }
    }
    /// The three saved-pause states of `statusKey` plus quitting.
    private var ring: StatusRing.Kind {
        if model.closing { return .closing }
        switch model.manualPause {
        case true: return .paused
        case false: return .assistance
        case nil: return .unknown
        }
    }
}
