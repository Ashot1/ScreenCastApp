import SwiftUI

/// The open editor's draft text, its plan fields and the save and discard actions. Editor pages
/// show it alone; the case and step pages show it as their edit mode. `done` runs after an
/// accepted save or a discard, so that mode closes.
struct DraftEditor: View {
    @Bindable var model: AssistantModel
    var titleFont: Font = .title.weight(.semibold)
    var done: () -> Void = {}
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Text(LocalizedStringKey(model.editorKey), bundle: .module).font(titleFont)
                    .accessibilityAddTraits(.isHeader)
                if model.hasUnsavedChanges {
                    Pill(text: Text("intent.unsaved", bundle: .module), tone: Theme.stale)
                }
            }
            TextEditor(text: $model.draft).font(.body).frame(minHeight: 120)
                .scrollContentBackground(.hidden).padding(8)
                .background(.fill.quaternary, in: RoundedRectangle(cornerRadius: 12))
                .accessibilityLabel(
                    Text(LocalizedStringKey(model.editorKey), bundle: .module))
            PlanDraftFields(model: model)
            HStack {
                Button {
                    Task { if await model.save() { done() } }
                } label: {
                    Text(
                        model.isCompletingTask
                            ? "lifecycle.confirmComplete"
                            : model.isEditingNote ? "notes.save" : "intent.save", bundle: .module)
                }
                .buttonStyle(.borderedProminent).disabled(
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
            .buttonBorderShape(.capsule)
        }
    }
}
