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
/// The GitHub, Jira and Todoist source (page 7): profile and link, «Обновить», then the coverage,
/// the objects with checkboxes («Выбрано N из 32»), the preview with its JSON and «Сохранить», and
/// the saved metadata. Connected states need the user's own account (ADR-024).
struct IntegrationSourcesView<Adapter: IntegrationPresentation>: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let task: MemoryTask
    @Bindable var source: IntegrationSourcesModel<Adapter>
    private func text(_ suffix: String) -> Text {
        Text(LocalizedStringKey(Adapter.copy + "." + suffix), bundle: .module)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("integration.profile", bundle: .module).font(.callout.weight(.medium))
                    Picker(
                        selection: Binding(
                            get: { source.profile?.id },
                            set: { id in
                                if let p = source.profiles.first(where: { $0.id == id }) {
                                    Task { await source.use(p) }
                                }
                            })
                    ) {
                        Text("integration.selectProfile", bundle: .module)
                            .tag(Optional<UInt64>.none)
                        ForEach(source.profiles) { p in
                            Text(verbatim: p.name).tag(Optional(p.id))
                        }
                    } label: {
                        Text("integration.profile", bundle: .module)
                    }
                    .labelsHidden()
                }
                .frame(maxWidth: 220, alignment: .leading)
                VStack(alignment: .leading, spacing: 6) {
                    Text(LocalizedStringKey(Adapter.targetKey), bundle: .module)
                        .font(.callout.weight(.medium))
                    TextField(text: $source.target) {
                        Text(LocalizedStringKey(Adapter.targetKey), bundle: .module)
                    }
                    .textFieldStyle(.plain).font(.callout.monospaced())
                    .padding(.horizontal, 10).padding(.vertical, 7).ambientInset(cornerRadius: 10)
                    .onChange(of: source.target) { source.cancel() }
                }
            }
            if let until = source.profile?.notBeforeUTC {
                HStack {
                    Text("integration.cooldown", bundle: .module)
                    Text(Date(timeIntervalSince1970: TimeInterval(until)), style: .date)
                    Text(Date(timeIntervalSince1970: TimeInterval(until)), style: .time)
                }
                .font(.callout).foregroundStyle(Theme.stale.text)
            }
            if source.busy {
                WorkRow(title: text("reading"), cancel: { source.cancel() }, disabled: false)
                    .ambientInset()
            } else {
                Button {
                    Task { await source.read(task: task, chatGPT: chatGPT) }
                } label: {
                    Label {
                        source.window == nil
                            ? text("read") : Text("integration.refresh", bundle: .module)
                    } icon: {
                        Image(systemName: "arrow.clockwise")
                    }
                }
                .ambientPrimary()
                .disabled(
                    source.profile == nil || source.scopeExcluded || source.target.isEmpty
                        || source.changingCredential
                        || !chatGPT.assistanceReady || model.manualPause != false)
            }
            if source.scopeExcluded { text("excluded").foregroundStyle(Theme.stale.text) }
            if let window = source.window { objects(window) }
            if let preview = source.preview {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(preview.objects) {
                        Adapter.objectView($0, compact: false).padding(12).ambientInset()
                    }
                    DisclosureGroup {
                        Text(verbatim: preview.json).font(.system(.caption, design: .monospaced))
                            .textSelection(.enabled).padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .ambientInset(cornerRadius: 12)
                    } label: {
                        Text("integration.json", bundle: .module).font(.callout.weight(.medium))
                    }
                    HStack(spacing: 12) {
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
                        .ambientPrimary()
                        if source.saved {
                            Label {
                                Text("integration.saved", bundle: .module)
                            } icon: {
                                Image(systemName: "checkmark")
                            }
                            .font(.callout).foregroundStyle(Theme.ready.text)
                        }
                    }
                }
            } else if source.saved {
                Text("integration.saved", bundle: .module).font(.callout)
            }
            if let failure = source.failureKey {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    message: Text(LocalizedStringKey(failure), bundle: .module))
            }
            if !source.savedMetadata.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("integration.savedMetadata", bundle: .module)
                        .font(.callout.weight(.medium))
                    ForEach(source.savedMetadata) {
                        Adapter.metadataView($0.metadata).font(.callout).padding(12)
                            .ambientInset()
                    }
                }
            }
            PageControls(
                next: source.metadataNext.map { after in
                    { Task { await source.loadMetadata(task: task, after: after) } }
                })
            text("partial").font(.caption).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .task(id: task.id) {
            await source.loadProfiles()
            await source.loadMetadata(task: task)
        }
    }
    @ViewBuilder private func objects(
        _ window: IntegrationWindow<Adapter.Object, Adapter.Coverage>
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 6) {
                Text("integration.coverage", bundle: .module).font(.callout.weight(.medium))
                ForEach(window.categories) { Adapter.coverageView($0) }
            }
            .padding(12).frame(maxWidth: .infinity, alignment: .leading).ambientInset()
            HStack {
                Text("integration.objects", bundle: .module).font(.headline)
                Text("integration.selected \(source.selected.count)", bundle: .module)
                    .font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button {
                    Task { await source.makePreview() }
                } label: {
                    Text("integration.preview", bundle: .module)
                }
                .buttonStyle(.bordered).buttonBorderShape(.capsule)
                .disabled(source.selected.isEmpty || source.preparingPreview)
            }
            Text("integration.selectHelp", bundle: .module).font(.caption)
                .foregroundStyle(.secondary)
            ForEach(Array(window.objects.enumerated()), id: \.element.id) { index, object in
                Toggle(
                    isOn: Binding(
                        get: { source.selected.contains(index) },
                        set: { _ in source.toggle(index) })
                ) {
                    Adapter.objectView(object, compact: true)
                }
                .toggleStyle(.checkbox)
                .disabled(!Adapter.bindable(object))
                .padding(12).frame(maxWidth: .infinity, alignment: .leading).ambientInset()
            }
        }
    }
}
