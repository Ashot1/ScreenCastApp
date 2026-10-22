import SwiftUI

/// The step's own page (ADR-023 F11): back to the plan, the step with its actions, prompt and
/// reference, its edit mode and its checks. A completed case never reaches it: Core clears the
/// plan on completion and `selectPlan` refuses a completed case.
struct StepPage: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    let task: MemoryTask
    let step: MemoryStep
    @Binding var editing: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            navigation
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    StepStatusIcon(step: step)
                    StepStatus(step: step, task: task)
                }
                Text(verbatim: step.description).font(.system(size: 26, weight: .semibold))
                    .textSelection(.enabled).accessibilityAddTraits(.isHeader)
                HStack {
                    Text("plan.stepID", bundle: .module)
                    Text(verbatim: String(step.id))
                }
                .font(.caption).foregroundStyle(.secondary)
                if let waiting = snapshot.waitingSteps(
                    for: step, dependenciesFromStart: model.dependenciesAfter == 0),
                    !waiting.isEmpty
                {
                    Text(
                        "plan.waitsFor \(waiting.map(String.init).joined(separator: ", "))",
                        bundle: .module
                    )
                    .font(.caption).foregroundStyle(.secondary)
                }
                StepActions(model: model, step: step)
            }
            if model.editorKind == .step && (editing || model.hasUnsavedChanges) {
                DraftEditor(model: model, titleFont: .title3.weight(.semibold)) { editing = false }
            }
            if let prompt = step.prompt { exact("plan.prompt", prompt) }
            if let reference = step.reference { exact("plan.reference", reference) }
            StepChecks(model: model, snapshot: snapshot, step: step)
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }
    }
    private var navigation: some View {
        HStack(spacing: 10) {
            Button {
                Task { await model.requestSelection(.task(task.id)) }
            } label: {
                Image(systemName: "chevron.left")
            }
            .buttonStyle(.bordered).buttonBorderShape(.circle)
            .accessibilityLabel(Text("plan.back", bundle: .module))
            .help(Text("plan.back", bundle: .module))
            HStack(spacing: 4) {
                Text("plan.title", bundle: .module)
                Text(verbatim: "·").accessibilityHidden(true)
                Text(verbatim: task.text).lineLimit(1).truncationMode(.tail)
            }
            .font(.caption).foregroundStyle(.secondary)
            Spacer()
            Menu {
                Button {
                    // After a plan action the model is back in the case editor; reopening the step
                    // loads its draft before the fields show.
                    Task {
                        if model.editorKind != .step {
                            await model.requestSelection(.step(step.id))
                        }
                        editing = model.editorKind == .step
                    }
                } label: {
                    Text("plan.editStep", bundle: .module)
                }
                Divider()
                Button(role: .destructive) {
                    Task { await model.deleteStep(step.id) }
                } label: {
                    Text("plan.deleteStep", bundle: .module)
                }
                .disabled(model.hasUnsavedChanges)
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.button).menuIndicator(.hidden)
            .buttonStyle(.bordered).buttonBorderShape(.circle)
            .accessibilityLabel(Text("plan.stepActions", bundle: .module))
        }
    }
    /// Prompt and reference are exact text; a quiet inset keeps them apart from the step.
    private func exact(_ title: LocalizedStringKey, _ text: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title, bundle: .module).font(.headline)
            Text(verbatim: text).font(.callout.monospaced()).textSelection(.enabled)
                .padding(14).frame(maxWidth: .infinity, alignment: .leading)
                .background(.fill.quaternary, in: RoundedRectangle(cornerRadius: 18))
        }
    }
}

/// Copy the prompt and the reference, and complete or reopen the step; shared by the «Следующий
/// шаг» block and the step page.
struct StepActions: View {
    let model: AssistantModel
    let step: MemoryStep
    var body: some View {
        HStack {
            if let prompt = step.prompt {
                Button {
                    model.copy(prompt)
                } label: {
                    Label {
                        Text("plan.copyPrompt", bundle: .module)
                    } icon: {
                        Image(systemName: "doc.on.doc")
                    }
                }
                .ambientPrimary()
            }
            if let reference = step.reference {
                Button {
                    model.copy(reference)
                } label: {
                    Label {
                        Text("plan.copyReference", bundle: .module)
                    } icon: {
                        Image(systemName: "link")
                    }
                }
                .buttonStyle(.bordered)
            }
            Spacer()
            Button {
                Task { await model.completeStep(step) }
            } label: {
                Label {
                    Text(
                        step.completedAtUTC == nil ? "plan.completeStep" : "plan.undoStep",
                        bundle: .module)
                } icon: {
                    Image(systemName: step.completedAtUTC == nil ? "checkmark" : "arrow.uturn.left")
                }
            }
            .buttonStyle(.bordered).disabled(model.hasUnsavedChanges)
        }
        .buttonBorderShape(.capsule)
    }
}

/// The checks of the selected step; the result is chosen from a system menu (ADR-023 #2).
struct StepChecks: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    let step: MemoryStep
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("plan.checks", bundle: .module).font(.headline)
                Spacer()
                Button {
                    Task { await model.requestSelection(.newCheck(step.id)) }
                } label: {
                    Label {
                        Text("plan.newCheck", bundle: .module)
                    } icon: {
                        Image(systemName: "plus")
                    }
                }
                .buttonStyle(.borderless)
            }
            ForEach(snapshot.checks.items) { check in
                Divider()
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    resultIcon(check.result)
                    VStack(alignment: .leading, spacing: 4) {
                        Button {
                            Task { await model.requestSelection(.check(check.id)) }
                        } label: {
                            Text(verbatim: check.description).multilineTextAlignment(.leading)
                        }
                        .buttonStyle(.plain)
                        .accessibilityHint(Text("plan.editCheck", bundle: .module))
                        if let time = check.confirmedAtUTC {
                            HStack {
                                Text("plan.checkConfirmed", bundle: .module)
                                Text(
                                    Date(timeIntervalSince1970: TimeInterval(time)),
                                    format: .dateTime.year().month().day().hour().minute())
                            }
                            .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    Menu {
                        ForEach([MemoryCheckResult.unknown, .passed, .failed], id: \.rawValue) {
                            result in
                            Button {
                                Task { await model.setCheck(check.id, result: result) }
                            } label: {
                                Text(checkKey(result), bundle: .module)
                            }
                            .disabled(check.result == result)
                        }
                    } label: {
                        Text(checkKey(check.result), bundle: .module)
                    }
                    .fixedSize()
                    .disabled(model.hasUnsavedChanges)
                    Button(role: .destructive) {
                        Task { await model.deleteCheck(check.id) }
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(Text("plan.deleteCheck", bundle: .module))
                    .help(Text("plan.deleteCheck", bundle: .module))
                    .disabled(model.hasUnsavedChanges)
                }
            }
        }
    }
    private func resultIcon(_ result: MemoryCheckResult) -> some View {
        Group {
            switch result {
            case .unknown: Image(systemName: "circle").foregroundStyle(.tertiary)
            case .passed:
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.ready.text)
            case .failed: Image(systemName: "xmark.circle.fill").foregroundStyle(Theme.error.text)
            }
        }
        .accessibilityHidden(true)
    }
    private func checkKey(_ value: MemoryCheckResult) -> LocalizedStringKey {
        switch value {
        case .unknown: "plan.unknownCheck"
        case .passed: "plan.passed"
        case .failed: "plan.failed"
        }
    }
}

/// The step's status icon; readiness comes only from Core.
struct StepStatusIcon: View {
    let step: MemoryStep
    var body: some View {
        Group {
            if step.completedAtUTC != nil {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.secondary)
            } else if step.ready {
                Image(systemName: "circle.inset.filled").foregroundStyle(Theme.ready.text)
            } else {
                Image(systemName: "circle.dashed").foregroundStyle(.secondary)
            }
        }
        .font(.title3).accessibilityHidden(true)
    }
}

/// The step's status label; «waiting» is shown only for a confirmed plan, as before.
struct StepStatus: View {
    let step: MemoryStep
    let task: MemoryTask
    var body: some View {
        if step.completedAtUTC != nil {
            Pill(text: Text("plan.done", bundle: .module))
        } else if step.ready {
            Pill(text: Text("plan.ready", bundle: .module), tone: Theme.ready)
        } else if task.planCompleteness == .complete {
            Pill(text: Text("plan.blocked", bundle: .module))
        }
    }
}
