import SwiftUI

/// The open editor's draft text, its plan fields and the save and discard actions. Editor pages
/// show it alone; the case and step pages show it as their edit mode. `done` runs after an
/// accepted save or a discard, so that mode closes.
struct DraftEditor: View {
    @Bindable var model: AssistantModel
    var titleFont: Font = .title.weight(.semibold)
    /// Replaces the editor's own title, e.g. with the step's position.
    var title: Text? = nil
    var done: () -> Void = {}
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if model.isCompletingTask, let task = model.snapshot?.selectedTask {
                HStack(spacing: 4) {
                    Text("case.title", bundle: .module)
                    Text(verbatim: "·").accessibilityHidden(true)
                    Text(verbatim: task.text).lineLimit(1).truncationMode(.tail)
                }
                .font(.callout).foregroundStyle(.secondary)
            }
            HStack(spacing: 10) {
                (title ?? Text(LocalizedStringKey(model.editorKey), bundle: .module))
                    .font(titleFont).accessibilityAddTraits(.isHeader)
                if model.hasUnsavedChanges {
                    HStack(spacing: 5) {
                        Circle().fill(Theme.stale.text).frame(width: 6, height: 6)
                            .accessibilityHidden(true)
                        Text("intent.unsavedShort", bundle: .module)
                    }
                    .font(.callout).foregroundStyle(Theme.stale.text)
                }
            }
            // A dependency is chosen from the plan's steps below; its draft holds step IDs.
            if !model.isEditingDependency {
                VStack(alignment: .leading, spacing: 8) {
                    if let caption {
                        Text(caption, bundle: .module).font(.callout).foregroundStyle(.secondary)
                    }
                    TextEditor(text: $model.draft).font(.body).frame(minHeight: 110)
                        .scrollContentBackground(.hidden).padding(10)
                        .ambientInset()
                        .accessibilityLabel(
                            Text(caption ?? LocalizedStringKey(model.editorKey), bundle: .module))
                }
            }
            PlanDraftFields(model: model)
            // What completion deletes stays visible next to its confirmation (D83).
            if model.isCompletingTask {
                Label {
                    Text("lifecycle.cleanup", bundle: .module)
                } icon: {
                    Image(systemName: "exclamationmark.triangle").foregroundStyle(
                        Theme.stale.text)
                }
                .font(.callout).padding(.top, 8)
            }
            HStack {
                Button {
                    Task { if await model.save() { done() } }
                } label: {
                    Text(
                        model.isCompletingTask
                            ? "lifecycle.confirmComplete"
                            : model.isEditingNote ? "notes.save" : "intent.save", bundle: .module)
                }
                .ambientPrimary().disabled(
                    model.draft.isEmpty || !model.hasUnsavedChanges)
                // Always available: on an editor page and in an edit mode it is also the way back.
                Button {
                    model.discardChanges()
                    done()
                } label: {
                    Text("intent.discard", bundle: .module)
                }
                .buttonStyle(.bordered)
            }
            .buttonBorderShape(.capsule).controlSize(.large)
        }
    }
    private var caption: LocalizedStringKey? {
        switch model.editorKind {
        case .step, .newStep: "plan.stepText"
        case .completion: "lifecycle.summary"
        default: nil
        }
    }
}
