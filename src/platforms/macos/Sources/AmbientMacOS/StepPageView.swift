import SwiftUI

/// The step's own page (ADR-023 F11, ADR-024 26C): back to the plan, the step with its actions and
/// prompt, its edit mode and its checks. A completed case never reaches it: Core clears the plan on
/// completion and `selectPlan` refuses a completed case.
struct StepPage: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    let task: MemoryTask
    let step: MemoryStep
    @Binding var editing: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            navigation
            // The edit mode replaces the step's content, as on the design's step edit page.
            if model.editorKind == .step && (editing || model.hasUnsavedChanges) {
                DraftEditor(
                    model: model, titleFont: .title3.weight(.semibold),
                    title: position.map { Text("plan.editingStepNumber \($0)", bundle: .module) }
                ) { editing = false }
            } else {
                content
            }
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }
    }
    private var position: Int? { model.stepPositions?[step.id] }
    private var content: some View {
        VStack(alignment: .leading, spacing: 24) {
            VStack(alignment: .leading, spacing: 10) {
                if let status = StepStatus.text(step: step, task: task) {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(
                                step.ready && step.completedAtUTC == nil
                                    ? Theme.ready.text : Color.secondary
                            )
                            .frame(width: 6, height: 6).accessibilityHidden(true)
                        status
                    }
                    .font(.caption).foregroundStyle(.secondary)
                }
                Text(verbatim: step.description).font(.system(size: 26, weight: .semibold))
                    .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                if let waiting = model.waitingNumbers(for: step) {
                    Text("plan.waitsFor \(waiting)", bundle: .module).foregroundStyle(.secondary)
                }
                if let dependents = model.waitingDependentNumbers(of: step) {
                    let text = AssistantModel.list(dependents)
                    Text(
                        dependents.count == 1
                            ? "plan.waitedByOne \(text)" : "plan.waitedByMany \(text)",
                        bundle: .module
                    )
                    .foregroundStyle(.secondary)
                }
            }
            StepActions(model: model, step: step)
            if let prompt = step.prompt {
                VStack(alignment: .leading, spacing: 8) {
                    Text("plan.prompt", bundle: .module).font(.callout).foregroundStyle(.secondary)
                    Text(verbatim: prompt).font(.callout.monospaced()).textSelection(.enabled)
                        .padding(.horizontal, 20).padding(.vertical, 16)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .ambientInset(cornerRadius: 16)
                }
            }
            StepChecks(model: model, snapshot: snapshot, step: step)
        }
    }
    private var navigation: some View {
        HStack(spacing: 10) {
            Button {
                Task { await model.requestSelection(.task(task.id)) }
            } label: {
                Label {
                    Text("plan.back", bundle: .module)
                } icon: {
                    Image(systemName: "chevron.left")
                }
                .labelStyle(.iconOnly)
            }
            .buttonStyle(.glass).buttonBorderShape(.circle).controlSize(.large)
            .help(Text("plan.back", bundle: .module))
            HStack(spacing: 4) {
                Text("plan.title", bundle: .module)
                Text(verbatim: "·").accessibilityHidden(true)
                Text(verbatim: task.text).lineLimit(1).truncationMode(.tail)
            }
            .font(.callout).foregroundStyle(.secondary)
            Spacer()
            if let position, let count = model.stepPositions?.count {
                Text("plan.stepOf \(position) \(count)", bundle: .module)
                    .font(.callout).foregroundStyle(.secondary)
            }
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
            .buttonStyle(.glass).buttonBorderShape(.circle).controlSize(.large)
            .accessibilityLabel(Text("plan.stepActions", bundle: .module))
        }
    }
}

/// Copy the prompt and the link, and complete or reopen the step; shared by the «Следующий шаг»
/// block and the step page.
struct StepActions: View {
    let model: AssistantModel
    let step: MemoryStep
    var body: some View {
        HStack(spacing: 10) {
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
            // The link pill shows the stored link itself and copies it.
            if let reference = step.reference {
                Button {
                    model.copy(reference)
                } label: {
                    Label {
                        Text(verbatim: reference).lineLimit(1).truncationMode(.middle)
                            .frame(maxWidth: 220)
                    } icon: {
                        Image(systemName: "link")
                    }
                }
                .buttonStyle(.bordered).buttonBorderShape(.capsule)
                .accessibilityLabel(Text("plan.copyReference", bundle: .module))
                .accessibilityValue(Text(verbatim: reference))
                .help(Text(verbatim: reference))
            }
            Spacer(minLength: 8)
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
            .buttonStyle(.plain).disabled(model.hasUnsavedChanges)
        }
        .controlSize(.large)
    }
}

/// The checks of the selected step; the result is chosen from a system menu (ADR-023 #2).
struct StepChecks: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    let step: MemoryStep
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                title.font(.callout).foregroundStyle(.secondary)
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
                .buttonStyle(.borderless).font(.callout.weight(.medium))
            }
            .padding(.bottom, 10)
            ForEach(snapshot.checks.items) { check in
                Divider()
                HStack(spacing: 12) {
                    resultIcon(check.result)
                    VStack(alignment: .leading, spacing: 3) {
                        Button {
                            Task { await model.requestSelection(.check(check.id)) }
                        } label: {
                            Text(verbatim: check.description).multilineTextAlignment(.leading)
                        }
                        .buttonStyle(.plain)
                        .accessibilityHint(Text("plan.editCheck", bundle: .module))
                        recorded(check).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    resultMenu(check)
                    Button(role: .destructive) {
                        Task { await model.deleteCheck(check.id) }
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.borderless).foregroundStyle(.secondary)
                    .accessibilityLabel(Text("plan.deleteCheck", bundle: .module))
                    .help(Text("plan.deleteCheck", bundle: .module))
                    .disabled(model.hasUnsavedChanges)
                }
                .padding(.vertical, 12)
            }
        }
    }
    /// «Проверки · 1 из 2»: passed of all checks; a step's checks always load whole (Core keeps at
    /// most 8 per step).
    private var title: Text {
        guard let count = snapshot.checks.wholeCount(fromStart: true), count > 0 else {
            return Text("plan.checks", bundle: .module)
        }
        let passed = snapshot.checks.items.filter { $0.result == .passed }.count
        return Text("plan.checksCount \(passed) \(count)", bundle: .module)
    }
    private func recorded(_ check: MemoryCheck) -> Text {
        guard let time = check.confirmedAtUTC else {
            return Text("plan.checkPending", bundle: .module)
        }
        let date = Date(timeIntervalSince1970: TimeInterval(time))
        return Text(
            "\(Text("plan.checkConfirmed", bundle: .module)), \(date, format: .dateTime.day().month())"
        )
    }
    /// A system menu drawn as the design's tone capsule with its own chevron.
    private func resultMenu(_ check: MemoryCheck) -> some View {
        let tone: Tone? =
            switch check.result {
            case .unknown: nil
            case .passed: Theme.ready
            case .failed: Theme.error
            }
        return Menu {
            ForEach([MemoryCheckResult.unknown, .passed, .failed], id: \.rawValue) { result in
                Button {
                    Task { await model.setCheck(check.id, result: result) }
                } label: {
                    Text(checkKey(result), bundle: .module)
                }
                .disabled(check.result == result)
            }
        } label: {
            HStack(spacing: 4) {
                Text(checkKey(check.result), bundle: .module)
                Image(systemName: "chevron.down").font(.caption2.weight(.bold))
                    .accessibilityHidden(true)
            }
            .font(.callout.weight(.medium))
            .foregroundStyle(tone.map { AnyShapeStyle($0.text) } ?? AnyShapeStyle(.primary))
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(
                tone.map { AnyShapeStyle($0.fill) } ?? AnyShapeStyle(.quaternary), in: Capsule())
        }
        .menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden)
        .fixedSize()
        .disabled(model.hasUnsavedChanges)
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
        .font(.title3).accessibilityHidden(true)
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
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.tertiary)
            } else if step.ready {
                Image(systemName: "smallcircle.filled.circle").foregroundStyle(Theme.ready.text)
            } else {
                Image(systemName: "circle.dashed").foregroundStyle(.secondary)
            }
        }
        .font(.title3).accessibilityHidden(true)
    }
}

/// The step's status text; «waiting» only for a confirmed plan, as before. Readiness comes only
/// from Core.
enum StepStatus {
    static func text(step: MemoryStep, task: MemoryTask) -> Text? {
        if step.completedAtUTC != nil { return Text("plan.done", bundle: .module) }
        if step.ready { return Text("plan.ready", bundle: .module) }
        if task.planCompleteness == .complete { return Text("plan.blocked", bundle: .module) }
        return nil
    }
}
