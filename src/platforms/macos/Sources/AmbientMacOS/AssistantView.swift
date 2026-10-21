import SwiftUI

public struct AssistantView: View {
    private let model: AssistantModel
    private let chatGPT: ChatGPTModel
    @State private var linkTarget: UInt64?
    @State private var deletion: CaseDeletion?
    @State private var section = MainSection.task
    @State private var tab = CaseTab.plan
    // Edit modes of the case text and of the step: here, so a section switch keeps them.
    @State private var editingTask = false
    @State private var editingStep = false
    public init(model: AssistantModel, chatGPT: ChatGPTModel) {
        self.model = model
        self.chatGPT = chatGPT
    }
    public var body: some View {
        NavigationSplitView {
            CaseSidebar(model: model, chatGPT: chatGPT, section: $section)
                .navigationSplitViewColumnWidth(244)
        } detail: {
            Group {
                switch section {
                // The feed owns its scroll (one scroll, per-window position by proposal ID).
                case .suggestions:
                    suggestions.safeAreaInset(edge: .top) {
                        if model.failure != nil {
                            memoryFailure.padding(.horizontal, 36).padding(.top, 16)
                        }
                    }
                case .task:
                    ScrollView {
                        VStack(alignment: .leading, spacing: 20) {
                            memoryFailure
                            if let snapshot = model.snapshot {
                                CaseDetailView(
                                    model: model, chatGPT: chatGPT, snapshot: snapshot,
                                    linkTarget: $linkTarget, deletion: $deletion, tab: $tab,
                                    editingTask: $editingTask, editingStep: $editingStep)
                                Text("memory.help", bundle: .module).font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .padding(.horizontal, 36).padding(.vertical, 32)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(nsColor: .textBackgroundColor))
        }
        .toolbar(removing: .title)
        .frame(minWidth: 740, minHeight: 640)
        .disabled(model.busy || model.closing)
        .task {
            if model.snapshot == nil { await model.reload() }
            await model.integrations.load()
        }
        .onChange(of: model.isCreatingTask ? nil : model.snapshot?.selectedTask) { _, _ in
            chatGPT.selectedTaskChanged()
        }
        // The section follows every selection path, including the unsaved-changes alert buttons.
        .onChange(of: model.snapshot?.selectedTask?.id) { _, _ in
            section = .task
            closeEditModes()
        }
        .onChange(of: model.snapshot?.selectedStep?.id) { _, _ in editingStep = false }
        // Opening a step or another editor changes the edited entity, so its mode closes too.
        .onChange(of: model.editorKind) { _, kind in
            if kind != .task { editingTask = false }
            if kind != .step { editingStep = false }
        }
        .onChange(of: model.isCreatingTask) { _, _ in section = .task }
        // Case and pause changes cancel source reads in the model (D76); the root adds the two
        // facts the model does not see: assistance became unavailable and the window closed.
        .onChange(of: chatGPT.assistanceReady) { _, ready in if !ready { model.cancelSourceViews() }
        }
        .onDisappear { model.cancelSourceViews() }
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
                closeEditModes()
                Task { await model.saveAndSwitch(to: accepted) }
            } label: {
                Text(
                    model.isCompletingTask ? "lifecycle.confirmComplete" : "switch.save",
                    bundle: .module)
            }
            Button(role: .destructive) {
                let accepted = model.pendingSelection
                closeEditModes()
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
    /// A failed save keeps its draft visible: the fields also show while there are unsaved changes.
    private func closeEditModes() {
        editingTask = false
        editingStep = false
    }
    @ViewBuilder private var memoryFailure: some View {
        if let failure = model.failure {
            Callout(
                tone: Theme.error, systemImage: "exclamationmark.circle",
                title: Text("error.title", bundle: .module),
                message: Text(failure.message)
            ) {
                Button {
                    Task { await model.reload() }
                } label: {
                    Text("error.retry", bundle: .module)
                }
            }
        }
    }
    @ViewBuilder private var suggestions: some View {
        if !chatGPT.feed.isEmpty || !chatGPT.replyDraft.isEmpty || chatGPT.assisting {
            AssistanceCardView(model: model, chatGPT: chatGPT)
        } else {
            ContentUnavailableView {
                Label {
                    Text("feed.empty", bundle: .module)
                } icon: {
                    Image(systemName: "tray")
                }
            }
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
