import SwiftUI

/// Adapter-owned presentation inside the shared source panel.
@MainActor protocol IntegrationPresentation: IntegrationAdapter {
    associatedtype ObjectView: View
    associatedtype CoverageView: View
    associatedtype MetadataView: View
    /// Prefix of adapter copy keys: `<prefix>.title`, `.read`, `.reading`, `.partial`, `.excluded`.
    static var copy: String { get }
    static var targetKey: String { get }
    static func objectView(_ object: Object, compact: Bool) -> ObjectView
    static func coverageView(_ coverage: Coverage) -> CoverageView
    static func metadataView(_ metadata: Metadata) -> MetadataView
}

/// Source time of a saved or observed integration object; unknown stays explicit.
struct IntegrationSourceTimeView: View {
    let time: Int64?
    var body: some View {
        LabeledContent {
            if let time {
                Text(verbatim: Date(timeIntervalSince1970: TimeInterval(time)).formatted(.iso8601))
            } else {
                Text("integration.unknown", bundle: .module)
            }
        } label: {
            Text("integration.sourceTime", bundle: .module)
        }
    }
}
struct IntegrationSourcesView<Adapter: IntegrationPresentation>: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Bindable var source: IntegrationSourcesModel<Adapter>
    private func text(_ suffix: String) -> Text {
        Text(LocalizedStringKey("\(Adapter.copy).\(suffix)"), bundle: .module)
    }
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("integration.selectProfile", bundle: .module)
                ForEach(source.profiles) { p in
                    Button {
                        Task { await source.use(p) }
                    } label: {
                        HStack {
                            if source.profile?.id == p.id { Image(systemName: "checkmark") }
                            Text(verbatim: p.name)
                        }
                    }
                }
                TextField(text: $source.target) {
                    Text(LocalizedStringKey(Adapter.targetKey), bundle: .module)
                }.onChange(of: source.target) { source.cancel() }
                if let until = source.profile?.notBeforeUTC {
                    HStack {
                        Text("integration.cooldown", bundle: .module)
                        Text(Date(timeIntervalSince1970: TimeInterval(until)), style: .date)
                        Text(Date(timeIntervalSince1970: TimeInterval(until)), style: .time)
                    }
                }
                if source.busy {
                    text("reading")
                    Button {
                        source.cancel()
                    } label: {
                        Text("integration.cancel", bundle: .module)
                    }
                } else {
                    HStack {
                        Button {
                            Task { await source.read(task: task, chatGPT: chatGPT) }
                        } label: {
                            text("read")
                        }
                        Button {
                            Task { await source.read(task: task, chatGPT: chatGPT) }
                        } label: {
                            Text("integration.refresh", bundle: .module)
                        }
                    }.disabled(
                        source.profile == nil || source.scopeExcluded || source.target.isEmpty
                            || source.changingCredential
                            || !chatGPT.assistanceReady || model.manualPause != false)
                }
                if source.scopeExcluded { text("excluded") }
                text("partial").font(.caption)
                if let window = source.window {
                    ForEach(window.categories) { Adapter.coverageView($0) }
                    Text("integration.selectHelp", bundle: .module)
                    ForEach(Array(window.objects.enumerated()), id: \.element.id) { index, object in
                        Toggle(
                            isOn: Binding(
                                get: { source.selected.contains(index) },
                                set: { _ in source.toggle(index) })
                        ) {
                            Adapter.objectView(object, compact: true)
                        }.disabled(!Adapter.bindable(object))
                    }
                    Button {
                        Task { await source.makePreview() }
                    } label: {
                        Text("integration.preview", bundle: .module)
                    }.disabled(source.selected.isEmpty || source.preparingPreview)
                }
                if let preview = source.preview {
                    ForEach(preview.objects) { Adapter.objectView($0, compact: false) }
                    DisclosureGroup {
                        Text(verbatim: preview.json).font(.system(.caption, design: .monospaced))
                            .textSelection(.enabled)
                    } label: {
                        Text("integration.preview", bundle: .module)
                    }
                    Button {
                        Task {
                            if await source.bind(chatGPT: chatGPT) {
                                await model.reload()
                                await source.loadMetadata(task: task)
                            }
                        }
                    } label: {
                        Text("integration.save", bundle: .module)
                    }
                }
                if source.saved { Text("integration.saved", bundle: .module) }
                if let failure = source.failureKey {
                    Text(LocalizedStringKey(failure), bundle: .module).foregroundStyle(.red)
                }
                ForEach(source.savedMetadata) { Adapter.metadataView($0.metadata) }
                if let after = source.metadataNext {
                    Button {
                        Task { await source.loadMetadata(task: task, after: after) }
                    } label: {
                        Text("tasks.next", bundle: .module)
                    }
                }
            }
        } label: {
            text("title")
        }
        .task(id: task.id) {
            await source.loadProfiles()
            await source.loadMetadata(task: task)
        }
        .onDisappear { source.cancel() }
        .onChange(of: chatGPT.assistanceReady) { if !chatGPT.assistanceReady { source.cancel() } }
    }
}
