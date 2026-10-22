import SwiftUI

struct ManualPlanView: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if let task = snapshot.selectedTask {
                if task.lifecycle == .open { plan(task) }
                references
            }
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }
    }
    private func plan(_ task: MemoryTask) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if snapshot.steps.items.isEmpty {
                Text("plan.empty", bundle: .module).foregroundStyle(.secondary)
                    .padding(.vertical, 8)
            }
            // A row opens the step's page, which holds the step's actions and checks.
            ForEach(snapshot.steps.items) { step in
                Button {
                    Task { await model.requestSelection(.step(step.id)) }
                } label: {
                    row(step, task)
                }
                .buttonStyle(.plain)
            }
            pageControls(.steps, after: model.stepsAfter, next: snapshot.steps.nextAfter)
            Button {
                Task { await model.requestSelection(.newStep) }
            } label: {
                Label {
                    Text("plan.newStep", bundle: .module)
                } icon: {
                    Image(systemName: "plus").frame(width: 20)
                }
                .foregroundStyle(.secondary).padding(.vertical, 8)
            }
            .buttonStyle(.plain)
            HStack(spacing: 6) {
                Text(completenessKey(task.planCompleteness), bundle: .module)
                    .foregroundStyle(.secondary)
                if task.planCompleteness != .complete {
                    Button {
                        Task { await model.confirmPlan() }
                    } label: {
                        Text("plan.confirm", bundle: .module)
                    }
                    .buttonStyle(.link)
                    .help(Text("plan.confirmHelp", bundle: .module))
                    .disabled(model.hasUnsavedChanges)
                }
            }
            .font(.callout).padding(.top, 16)
            dependencies
        }
    }
    private func row(_ step: MemoryStep, _ task: MemoryTask) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            StepStatusIcon(step: step)
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: step.description)
                    .fontWeight(step.completedAtUTC == nil && step.ready ? .medium : .regular)
                    .foregroundStyle(step.completedAtUTC == nil ? .primary : .secondary)
                    .multilineTextAlignment(.leading)
                if let waiting = model.waitingNumbers(for: step) {
                    Text("plan.waitsFor \(waiting)", bundle: .module)
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityValue(StepStatus.text(step: step, task: task) ?? Text(verbatim: ""))
    }
    /// The dependency list and its removal; the design has no place for it, so it stays quiet
    /// under the plan.
    private var dependencies: some View {
        VStack(alignment: .leading, spacing: 6) {
            if !snapshot.dependencies.items.isEmpty {
                Text("plan.dependencies", bundle: .module).font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
            }
            ForEach(snapshot.dependencies.items) { dependency in
                HStack(spacing: 6) {
                    model.stepName(dependency.predecessor)
                    Image(systemName: "arrow.right").accessibilityHidden(true)
                    model.stepName(dependency.dependent)
                    Button(role: .destructive) {
                        Task { await model.deleteDependency(dependency.id) }
                    } label: {
                        Label {
                            Text("plan.deleteDependency", bundle: .module)
                        } icon: {
                            Image(systemName: "xmark")
                        }
                        .labelStyle(.iconOnly)
                    }
                    .buttonStyle(.borderless)
                    .help(Text("plan.deleteDependency", bundle: .module))
                    .disabled(model.hasUnsavedChanges)
                }
                .font(.callout).foregroundStyle(.secondary).lineLimit(1)
            }
            pageControls(
                .dependencies, after: model.dependenciesAfter,
                next: snapshot.dependencies.nextAfter)
            Button {
                Task { await model.requestSelection(.newDependency) }
            } label: {
                Label {
                    Text("plan.newDependency", bundle: .module)
                } icon: {
                    Image(systemName: "plus")
                }
            }
            .buttonStyle(.borderless).font(.callout)
        }
        .padding(.top, 8)
    }
    private var references: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !snapshot.references.items.isEmpty {
                Text("lifecycle.references", bundle: .module).font(.headline)
                VStack(spacing: 0) {
                    ForEach(snapshot.references.items) { reference in
                        if reference.id != snapshot.references.items.first?.id { Divider() }
                        HStack(spacing: 10) {
                            Image(systemName: "link").foregroundStyle(.secondary)
                                .accessibilityHidden(true)
                            Text(verbatim: reference.text).font(.callout.monospaced())
                                .lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                            Spacer(minLength: 8)
                            Button {
                                model.copy(reference.text)
                            } label: {
                                Text("plan.copy", bundle: .module)
                            }
                            .buttonStyle(.link)
                        }
                        .padding(.horizontal, 14).padding(.vertical, 10)
                    }
                }
                .ambientInset()
                pageControls(
                    .references, after: model.referencesAfter, next: snapshot.references.nextAfter)
            }
        }
    }
    private func pageControls(_ kind: MemoryPlanPage, after: UInt64, next: UInt64?) -> some View {
        PageControls(
            first: after == 0 ? nil : { Task { await model.pagePlan(kind, after: 0) } },
            next: next.map { next in { Task { await model.pagePlan(kind, after: next) } } })
    }
    private func completenessKey(_ value: MemoryCompleteness) -> LocalizedStringKey {
        switch value {
        case .unknown: "plan.unknown"
        case .partial: "plan.partial"
        case .complete: "plan.complete"
        }
    }
}

/// The design's «Следующий шаг» block above the case tabs, on a quiet inset fill (no glass inside
/// the sheet).
struct NextStepBlock: View {
    let model: AssistantModel
    let step: MemoryStep
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Circle().fill(Theme.ready.text).frame(width: 8, height: 8)
                    .accessibilityHidden(true)
                Text("plan.next", bundle: .module)
                if let positions = model.stepPositions, let position = positions[step.id] {
                    Text(verbatim: "·").accessibilityHidden(true)
                    Text("plan.position \(position) \(positions.count)", bundle: .module)
                }
            }
            .font(.callout).foregroundStyle(.secondary)
            Text(verbatim: step.description).font(.system(size: 18, weight: .medium))
                .textSelection(.enabled)
            StepActions(model: model, step: step)
        }
        .padding(.vertical, 20).padding(.horizontal, 22)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.fill.quaternary, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("plan.next", bundle: .module))
    }
}

extension AssistantModel {
    /// «Шаг N» by plan position, or the step text when positions are unknown (ADR-024, 26C).
    func stepName(_ id: UInt64) -> Text {
        if let position = stepPositions?[id] {
            return Text("plan.step \(position)", bundle: .module)
        }
        let text = snapshot?.steps.items.first { $0.id == id }?.description
        return Text(verbatim: text ?? "…")
    }
    /// Positions of the unfinished predecessors a step waits for, or nil when unknown or none.
    func waitingNumbers(for step: MemoryStep) -> String? {
        numbers(snapshot?.waitingSteps(for: step, dependenciesFromStart: dependenciesAfter == 0))
            .map(Self.list)
    }
    /// Positions of the unfinished steps waiting for this one, or nil when unknown or none.
    func waitingDependentNumbers(of step: MemoryStep) -> [Int]? {
        numbers(
            snapshot?.waitingDependents(of: step, dependenciesFromStart: dependenciesAfter == 0))
    }
    static func list(_ numbers: [Int]) -> String {
        numbers.map(String.init).joined(separator: ", ")
    }
    /// The IDs come from the loaded steps, which are the whole plan whenever positions are known.
    private func numbers(_ ids: [UInt64]?) -> [Int]? {
        guard let ids, !ids.isEmpty, let positions = stepPositions else { return nil }
        return ids.compactMap { positions[$0] }.sorted()
    }
}

struct PlanDraftFields: View {
    @Bindable var model: AssistantModel
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if model.isEditingStep {
                switchRow($model.draftHasPrompt) { Text("plan.hasPrompt", bundle: .module) }
                if model.draftHasPrompt {
                    TextEditor(text: $model.draftPrompt).font(.callout.monospaced())
                        .frame(minHeight: 90).scrollContentBackground(.hidden).padding(10)
                        .ambientInset()
                        .accessibilityLabel(Text("plan.prompt", bundle: .module))
                }
                switchRow($model.draftHasReference) { Text("plan.hasReference", bundle: .module) }
                if model.draftHasReference {
                    TextField(text: $model.draftReference) {
                        Text("plan.reference", bundle: .module)
                    }
                    .textFieldStyle(.plain).font(.callout.monospaced()).padding(10)
                    .ambientInset()
                }
            }
            if model.isEditingDependency { dependency }
            if model.isCompletingTask { completion }
        }
    }
    /// The design's switch-first row; the hidden toggle label stays its accessibility label.
    private func switchRow(_ isOn: Binding<Bool>, @ViewBuilder label: () -> Text) -> some View {
        let label = label()
        return HStack(spacing: 10) {
            Toggle(isOn: isOn) { label }.toggleStyle(.switch).labelsHidden()
            label.accessibilityHidden(true)
            Spacer(minLength: 0)
        }
    }
    /// Both steps are chosen from the loaded plan; the draft keeps their IDs as before.
    private var dependency: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 12) {
                stepPicker($model.draft, "plan.predecessor")
                stepPicker($model.draftReference, "plan.dependent")
            }
            Text("plan.dependencyHelp", bundle: .module).font(.callout)
                .foregroundStyle(.secondary)
        }
    }
    private func stepPicker(_ selection: Binding<String>, _ title: LocalizedStringKey)
        -> some View
    {
        VStack(alignment: .leading, spacing: 6) {
            Text(title, bundle: .module).font(.callout).foregroundStyle(.secondary)
            Picker(selection: selection) {
                Text(verbatim: "—").tag("")
                ForEach(model.snapshot?.steps.items ?? []) { step in
                    stepTitle(step).tag(String(step.id))
                }
            } label: {
                Text(title, bundle: .module)
            }
            .pickerStyle(.menu).labelsHidden()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    private func stepTitle(_ step: MemoryStep) -> Text {
        guard let position = model.stepPositions?[step.id] else {
            return Text(verbatim: step.description)
        }
        let number = Text("plan.step \(position)", bundle: .module)
        return Text("\(number) · \(step.description)")
    }
    private var completion: some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Text("lifecycle.keepLinks", bundle: .module).font(.headline)
                    Text("lifecycle.keepLinksHelp", bundle: .module).font(.callout)
                        .foregroundStyle(.secondary)
                }
                let links = model.snapshot?.links.items.filter(\.related) ?? []
                if !links.isEmpty {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(links) { link in
                            if link.id != links.first?.id { Divider() }
                            switchRow(keep(link)) { Text(verbatim: link.otherTask.text) }.lineLimit(
                                2
                            )
                            .padding(.horizontal, 14).padding(.vertical, 9)
                        }
                    }
                    .ambientInset()
                }
                PageControls(
                    first: model.linksAfter == 0
                        ? nil : { Task { await model.pageLinks(after: 0) } },
                    next: model.snapshot?.links.nextAfter.map { next in
                        { Task { await model.pageLinks(after: next) } }
                    })
            }
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Text("lifecycle.references", bundle: .module).font(.headline)
                    Text(
                        "plan.position \(model.draftReferences.count) \(Self.referenceLimit)",
                        bundle: .module
                    )
                    .font(.callout).foregroundStyle(.secondary)
                }
                if !model.draftReferences.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(model.draftReferences.indices, id: \.self) { index in
                            if index > 0 { Divider() }
                            HStack(spacing: 10) {
                                Image(systemName: "link").foregroundStyle(.secondary)
                                    .accessibilityHidden(true)
                                TextField(text: reference(index)) {
                                    Text("plan.reference", bundle: .module)
                                }
                                .textFieldStyle(.plain).font(.callout.monospaced())
                                Button(role: .destructive) {
                                    model.draftReferences.remove(at: index)
                                } label: {
                                    Text("lifecycle.removeReference", bundle: .module)
                                }
                                .buttonStyle(.borderless)
                            }
                            .padding(.horizontal, 14).padding(.vertical, 9)
                        }
                    }
                    .ambientInset()
                }
                HStack(spacing: 8) {
                    Button {
                        model.draftReferences.append("")
                    } label: {
                        Label {
                            Text("lifecycle.addReference", bundle: .module)
                        } icon: {
                            Image(systemName: "plus")
                        }
                    }
                    .buttonStyle(.bordered).buttonBorderShape(.capsule)
                    .disabled(model.draftReferences.count >= Self.referenceLimit)
                    Text("lifecycle.referencesLimit \(Self.referenceLimit)", bundle: .module)
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
        }
    }
    /// Core keeps at most this many useful references on a completed case.
    private static let referenceLimit = 32
    /// A discard empties the references while their rows still update once; a row index past
    /// the end reads empty instead of trapping.
    private func reference(_ index: Int) -> Binding<String> {
        Binding(
            get: {
                model.draftReferences.indices.contains(index) ? model.draftReferences[index] : ""
            },
            set: { value in
                if model.draftReferences.indices.contains(index) {
                    model.draftReferences[index] = value
                }
            })
    }
    private func keep(_ link: MemoryLink) -> Binding<Bool> {
        Binding(
            get: { model.draftKeepLinks.contains(link.otherTaskID) },
            set: { value in
                if value {
                    model.draftKeepLinks.insert(link.otherTaskID)
                } else {
                    model.draftKeepLinks.remove(link.otherTaskID)
                }
            })
    }
}
