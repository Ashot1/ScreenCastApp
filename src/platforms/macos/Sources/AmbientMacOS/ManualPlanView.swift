import SwiftUI

struct ManualPlanView: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    @State private var confirmReopen = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let task = snapshot.selectedTask {
                Text(
                    task.lifecycle == .completed ? "lifecycle.completed" : "lifecycle.open",
                    bundle: .module
                ).font(.headline)
                if task.initiativeSuppressed { Text("lifecycle.suppressed", bundle: .module) }
                Button {
                    Task { await model.toggleInitiative() }
                } label: {
                    Text(
                        task.initiativeSuppressed ? "lifecycle.unsuppress" : "lifecycle.suppress",
                        bundle: .module)
                }.disabled(model.hasUnsavedChanges)
                Text("lifecycle.suppressionHelp", bundle: .module).font(.caption).foregroundStyle(
                    .secondary)
                if task.lifecycle == .completed {
                    Button {
                        confirmReopen = true
                    } label: {
                        Text("lifecycle.reopen", bundle: .module)
                    }
                    .disabled(model.hasUnsavedChanges)
                } else {
                    plan(task)
                    Button {
                        Task { await model.requestSelection(.completion) }
                    } label: {
                        Text("lifecycle.complete", bundle: .module)
                    }
                }
                references
            }
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }
        .confirmationDialog(
            Text("lifecycle.reopen", bundle: .module), isPresented: $confirmReopen,
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
            Text("lifecycle.reopenWarning", bundle: .module)
        }
    }
    private func plan(_ task: MemoryTask) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("plan.title", bundle: .module).font(.title3.bold())
            Text(completenessKey(task.planCompleteness), bundle: .module)
            Button {
                Task { await model.confirmPlan() }
            } label: {
                Text("plan.confirm", bundle: .module)
            }
            .disabled(model.hasUnsavedChanges || task.planCompleteness == .complete)
            Button {
                Task { await model.requestSelection(.newStep) }
            } label: {
                Text("plan.newStep", bundle: .module)
            }
            if snapshot.steps.items.isEmpty {
                Text("plan.empty", bundle: .module).foregroundStyle(.secondary)
            }
            ForEach(snapshot.steps.items) { step in
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("plan.stepID", bundle: .module)
                        Text(verbatim: String(step.id))
                    }
                    .font(.caption).foregroundStyle(.secondary)
                    Text(verbatim: step.description).textSelection(.enabled)
                    if step.completedAtUTC != nil {
                        Text("plan.done", bundle: .module)
                    } else if step.ready {
                        Text("plan.ready", bundle: .module).foregroundStyle(.green)
                    } else if task.planCompleteness == .complete {
                        Text("plan.blocked", bundle: .module)
                    }
                    if let prompt = step.prompt {
                        DisclosureGroup {
                            Text(verbatim: prompt).textSelection(.enabled)
                        } label: {
                            Text("plan.prompt", bundle: .module)
                        }
                        Button {
                            model.copy(prompt)
                        } label: {
                            Text("plan.copyPrompt", bundle: .module)
                        }
                    }
                    if let reference = step.reference {
                        Text(verbatim: reference).textSelection(.enabled)
                        Button {
                            model.copy(reference)
                        } label: {
                            Text("plan.copyReference", bundle: .module)
                        }
                    }
                    HStack {
                        Button {
                            Task { await model.requestSelection(.step(step.id)) }
                        } label: {
                            Text("plan.editStep", bundle: .module)
                        }
                        Button {
                            Task { await model.completeStep(step) }
                        } label: {
                            Text(
                                step.completedAtUTC == nil ? "plan.completeStep" : "plan.undoStep",
                                bundle: .module)
                        }.disabled(model.hasUnsavedChanges)
                        Button(role: .destructive) {
                            Task { await model.deleteStep(step.id) }
                        } label: {
                            Text("plan.deleteStep", bundle: .module)
                        }
                        .disabled(model.hasUnsavedChanges)
                    }
                }.padding().frame(maxWidth: .infinity, alignment: .leading)
                    .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 8))
            }
            pageButtons(.steps, next: snapshot.steps.nextAfter)
            checks
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
            pageButtons(.dependencies, next: snapshot.dependencies.nextAfter)
        }
    }
    @ViewBuilder private var checks: some View {
        if let step = snapshot.selectedStep {
            Divider()
            HStack {
                Text("plan.checks", bundle: .module).font(.headline)
                Text(verbatim: "#\(step.id)")
            }
            Button {
                Task { await model.requestSelection(.newCheck(step.id)) }
            } label: {
                Text("plan.newCheck", bundle: .module)
            }
            ForEach(snapshot.checks.items) { check in
                VStack(alignment: .leading, spacing: 6) {
                    Text(verbatim: check.description).textSelection(.enabled)
                    Text(checkKey(check.result), bundle: .module).font(.caption)
                    HStack {
                        ForEach([MemoryCheckResult.unknown, .passed, .failed], id: \.rawValue) {
                            result in
                            Button {
                                Task { await model.setCheck(check.id, result: result) }
                            } label: {
                                Text(checkKey(result), bundle: .module)
                            }
                            .disabled(model.hasUnsavedChanges || check.result == result)
                        }
                    }
                    HStack {
                        Button {
                            Task { await model.requestSelection(.check(check.id)) }
                        } label: {
                            Text("plan.editCheck", bundle: .module)
                        }
                        Button(role: .destructive) {
                            Task { await model.deleteCheck(check.id) }
                        } label: {
                            Text("plan.deleteCheck", bundle: .module)
                        }
                        .disabled(model.hasUnsavedChanges)
                    }
                }
            }
        }
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
                pageButtons(.references, next: snapshot.references.nextAfter)
            }
        }
    }
    private func pageButtons(_ kind: MemoryPlanPage, next: UInt64?) -> some View {
        HStack {
            Button {
                Task { await model.pagePlan(kind, after: 0) }
            } label: {
                Text("tasks.first", bundle: .module)
            }
            if let next {
                Button {
                    Task { await model.pagePlan(kind, after: next) }
                } label: {
                    Text("tasks.next", bundle: .module)
                }
            }
        }
    }
    private func completenessKey(_ value: MemoryCompleteness) -> LocalizedStringKey {
        switch value {
        case .unknown: "plan.unknown"
        case .partial: "plan.partial"
        case .complete: "plan.complete"
        }
    }
    private func checkKey(_ value: MemoryCheckResult) -> LocalizedStringKey {
        switch value {
        case .unknown: "plan.unknownCheck"
        case .passed: "plan.passed"
        case .failed: "plan.failed"
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
                if let snapshot = model.snapshot {
                    HStack {
                        Button {
                            Task { await model.pageLinks(after: 0) }
                        } label: {
                            Text("tasks.first", bundle: .module)
                        }
                        if let next = snapshot.links.nextAfter {
                            Button {
                                Task { await model.pageLinks(after: next) }
                            } label: {
                                Text("tasks.next", bundle: .module)
                            }
                        }
                    }
                }
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
