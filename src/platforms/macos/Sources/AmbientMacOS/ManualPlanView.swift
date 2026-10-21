import SwiftUI

struct ManualPlanView: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let task = snapshot.selectedTask {
                if task.lifecycle == .open { plan(task) }
                references
                Text("lifecycle.suppressionHelp", bundle: .module).font(.caption).foregroundStyle(
                    .secondary)
            }
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }
    }
    private func plan(_ task: MemoryTask) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            if let step = model.nextStep { next(step) }
            if snapshot.steps.items.isEmpty {
                Text("plan.empty", bundle: .module).foregroundStyle(.secondary)
            }
            // A row opens the step's page, which holds the step's actions and checks.
            ForEach(snapshot.steps.items) { step in
                Button {
                    Task { await model.requestSelection(.step(step.id)) }
                } label: {
                    HStack(alignment: .top, spacing: 10) {
                        StepStatusIcon(step: step)
                        VStack(alignment: .leading, spacing: 4) {
                            Text(verbatim: step.description)
                                .foregroundStyle(
                                    step.completedAtUTC == nil ? .primary : .secondary)
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
                        }
                        Spacer()
                        StepStatus(step: step, task: task)
                        Image(systemName: "chevron.right").foregroundStyle(.tertiary)
                            .accessibilityHidden(true)
                    }
                    .padding(.vertical, 6)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Divider()
            }
            pageControls(.steps, after: model.stepsAfter, next: snapshot.steps.nextAfter)
            Button {
                Task { await model.requestSelection(.newStep) }
            } label: {
                Label {
                    Text("plan.newStep", bundle: .module)
                } icon: {
                    Image(systemName: "plus")
                }
            }
            .buttonStyle(.borderless)
            HStack {
                Text(completenessKey(task.planCompleteness), bundle: .module)
                    .font(.caption).foregroundStyle(.secondary)
                Button {
                    Task { await model.confirmPlan() }
                } label: {
                    Text("plan.confirm", bundle: .module)
                }
                .buttonStyle(.borderless)
                .disabled(model.hasUnsavedChanges || task.planCompleteness == .complete)
            }
            Text("plan.dependencies", bundle: .module).font(.headline)
            Button {
                Task { await model.requestSelection(.newDependency) }
            } label: {
                Text("plan.newDependency", bundle: .module)
            }
            ForEach(snapshot.dependencies.items) { dependency in
                HStack {
                    Text(verbatim: "\(dependency.predecessor) → \(dependency.dependent)")
                    Button(role: .destructive) {
                        Task { await model.deleteDependency(dependency.id) }
                    } label: {
                        Text("plan.deleteDependency", bundle: .module)
                    }
                    .disabled(model.hasUnsavedChanges)
                }
            }
            pageControls(
                .dependencies, after: model.dependenciesAfter,
                next: snapshot.dependencies.nextAfter)
        }
    }
    /// The design's «Следующий шаг» block on a quiet inset fill; no glass inside the sheet.
    private func next(_ step: MemoryStep) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Circle().fill(Theme.ready.text).frame(width: 8, height: 8)
                    .accessibilityHidden(true)
                Text("plan.next", bundle: .module).font(.caption.weight(.semibold))
            }
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
    private var references: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !snapshot.references.items.isEmpty {
                Text("lifecycle.references", bundle: .module).font(.headline)
                ForEach(snapshot.references.items) { reference in
                    Text(verbatim: reference.text).textSelection(.enabled)
                    Button {
                        model.copy(reference.text)
                    } label: {
                        Text("plan.copy", bundle: .module)
                    }
                }
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

struct PlanDraftFields: View {
    @Bindable var model: AssistantModel
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if model.isEditingStep {
                Toggle(isOn: $model.draftHasPrompt) { Text("plan.hasPrompt", bundle: .module) }
                if model.draftHasPrompt {
                    TextEditor(text: $model.draftPrompt).frame(minHeight: 100)
                        .accessibilityLabel(Text("plan.prompt", bundle: .module))
                }
                Toggle(isOn: $model.draftHasReference) {
                    Text("plan.hasReference", bundle: .module)
                }
                if model.draftHasReference {
                    TextField(text: $model.draftReference) {
                        Text("plan.reference", bundle: .module)
                    }
                }
            }
            if model.isEditingDependency {
                Text("plan.predecessor", bundle: .module)
                TextField(text: $model.draftReference) { Text("plan.dependent", bundle: .module) }
                Text("plan.dependencyHelp", bundle: .module).font(.caption)
            }
            if model.isCompletingTask {
                Text("lifecycle.cleanup", bundle: .module)
                Text("lifecycle.keepLinks", bundle: .module).font(.headline)
                ForEach(model.snapshot?.links.items.filter(\.related) ?? []) { link in
                    Toggle(
                        isOn: Binding(
                            get: { model.draftKeepLinks.contains(link.otherTaskID) },
                            set: { value in
                                if value {
                                    model.draftKeepLinks.insert(link.otherTaskID)
                                } else {
                                    model.draftKeepLinks.remove(link.otherTaskID)
                                }
                            })
                    ) { Text(verbatim: link.otherTask.text).lineLimit(2) }
                }
                PageControls(
                    first: model.linksAfter == 0
                        ? nil : { Task { await model.pageLinks(after: 0) } },
                    next: model.snapshot?.links.nextAfter.map { next in
                        { Task { await model.pageLinks(after: next) } }
                    })
                Text("lifecycle.references", bundle: .module).font(.headline)
                ForEach(model.draftReferences.indices, id: \.self) { index in
                    HStack {
                        TextField(text: $model.draftReferences[index]) {
                            Text("plan.reference", bundle: .module)
                        }
                        Button(role: .destructive) {
                            model.draftReferences.remove(at: index)
                        } label: {
                            Text("lifecycle.removeReference", bundle: .module)
                        }
                    }
                }
                Button {
                    model.draftReferences.append("")
                } label: {
                    Text("lifecycle.addReference", bundle: .module)
                }
                .disabled(model.draftReferences.count >= 32)
            }
        }
    }
}
