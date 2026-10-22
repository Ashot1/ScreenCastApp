import SwiftUI

/// One observed field; missing, explicitly empty and unusable values stay distinct.
struct TodoistFieldRow: View {
    let key: String
    let value: TodoistPresence<String>?
    var body: some View {
        if let value {
            LabeledContent {
                switch value {
                case .value(let text): Text(verbatim: text).textSelection(.enabled)
                case .null: Text("todoist.null", bundle: .module)
                case .invalid: Text("todoist.invalid", bundle: .module)
                case .missing: Text("todoist.missing", bundle: .module)
                }
            } label: {
                Text(LocalizedStringKey(key), bundle: .module)
            }
        }
    }
}
struct TodoistMetadataView: View {
    let metadata: TodoistMetadata
    private var fields: TodoistFields { metadata.content.fields }
    private func time(_ value: TodoistPresence<TodoistTime>?) -> TodoistPresence<String>? {
        value?.flatMap {
            .value(Date(timeIntervalSince1970: TimeInterval($0.utcSeconds)).formatted(.iso8601))
        }
    }
    private func flag(_ value: TodoistPresence<Bool>?) -> TodoistPresence<String>? {
        value?.flatMap { .value($0 ? "true" : "false") }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("integration.history", bundle: .module).font(.caption).foregroundStyle(.secondary)
            Text(verbatim: metadata.account + " • " + metadata.content.objectID)
            Text(LocalizedStringKey("todoist.kind." + fields.kind), bundle: .module)
            Text("todoist.projection", bundle: .module).font(.caption)
            TodoistFieldRow(key: "todoist.project", value: fields.projectID.map { .value($0) })
            TodoistFieldRow(key: "todoist.name", value: fields.name)
            TodoistFieldRow(key: "todoist.owner", value: fields.userID)
            TodoistFieldRow(key: "todoist.responsible", value: fields.responsibleUID)
            TodoistFieldRow(key: "todoist.workspace", value: fields.workspaceID)
            TodoistFieldRow(key: "todoist.archived", value: flag(fields.isArchived))
            TodoistFieldRow(key: "integration.author", value: fields.postedUID)
            TodoistFieldRow(key: "todoist.dueDate", value: fields.due?.flatMap { $0.date })
            TodoistFieldRow(key: "todoist.dueText", value: fields.due?.flatMap { $0.string })
            TodoistFieldRow(key: "todoist.timezone", value: fields.due?.flatMap { $0.timezone })
            TodoistFieldRow(key: "todoist.deadline", value: fields.deadline?.flatMap { $0.date })
            TodoistFieldRow(
                key: "todoist.duration",
                value: fields.duration?.flatMap { d in
                    d.amount.flatMap { amount in d.unit.flatMap { .value("\(amount) \($0)") } }
                })
            TodoistFieldRow(
                key: "integration.created", value: time(fields.addedAt ?? fields.createdAt))
            TodoistFieldRow(
                key: "todoist.updated", value: time(fields.updatedAt ?? fields.postedAt))
            ForEach(Array(metadata.content.losses.enumerated()), id: \.offset) { _, loss in
                TodoistLossView(reason: loss)
            }
            ForEach(metadata.coverage) { TodoistCoverageView(coverage: $0) }
            LabeledContent {
                Text(
                    verbatim:
                        "\(Date(timeIntervalSince1970: TimeInterval(metadata.intervalStart)).formatted(.iso8601)) — \(Date(timeIntervalSince1970: TimeInterval(metadata.intervalEnd)).formatted(.iso8601))"
                )
            } label: {
                Text("integration.interval", bundle: .module)
            }
        }
    }
}
/// `missing:due.date`-style field losses or a named partial reason.
struct TodoistLossView: View {
    let reason: String
    var body: some View {
        if let colon = reason.firstIndex(of: ":") {
            HStack {
                Text(LocalizedStringKey("todoist." + reason[..<colon]), bundle: .module)
                Text(verbatim: String(reason[reason.index(after: colon)...]))
            }.font(.caption)
        } else {
            Text(LocalizedStringKey("todoist.reason." + reason), bundle: .module).font(.caption)
        }
    }
}
struct TodoistCoverageView: View {
    let coverage: TodoistCoverage
    var body: some View {
        VStack(alignment: .leading) {
            HStack {
                Text(LocalizedStringKey("todoist.category." + coverage.kind), bundle: .module)
                Text(
                    LocalizedStringKey(
                        coverage.status == "partial"
                            ? "todoist.status.partial" : "integration.status.\(coverage.status)"),
                    bundle: .module)
                Text("integration.pages", bundle: .module)
                Text(verbatim: "\(coverage.pages)")
                Text("integration.objects", bundle: .module)
                Text(verbatim: "\(coverage.objects)")
            }.font(.caption)
            ForEach(coverage.reasons, id: \.self) { TodoistLossView(reason: $0) }
        }
    }
}
struct TodoistObjectView: View {
    let object: TodoistObject
    var compact = false
    var body: some View {
        VStack(alignment: .leading) {
            TodoistMetadataView(metadata: object.metadata)
            IntegrationSourceTimeView(time: object.observedAtUTC)
            if !compact {
                LabeledContent {
                    Text(verbatim: object.key.scope)
                } label: {
                    Text("todoist.account", bundle: .module)
                }
                LabeledContent {
                    Text(verbatim: object.key.objectID)
                } label: {
                    Text("integration.objectID", bundle: .module)
                }
                LabeledContent {
                    Text(verbatim: object.version)
                } label: {
                    Text("sources.version", bundle: .module)
                }
            }
            // Literal Markdown: shown as text, never rendered and never with active links.
            Text(verbatim: object.text).lineLimit(compact ? 3 : nil).textSelection(.enabled)
            if !object.bindable { Text("integration.ambiguous", bundle: .module) }
        }
    }
}
struct TodoistSettingsView: View {
    @Bindable var source: TodoistSourcesModel
    let chatGPT: ChatGPTModel
    @State private var confirmDelete = false
    @State private var confirmAccount = false
    @State private var pendingAccount = ""
    @State private var pendingExcluded = false
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("todoist.help", bundle: .module)
                Text("todoist.accountHelp", bundle: .module).font(.caption)
                ForEach(source.profiles) { p in
                    Button {
                        Task { await source.use(p) }
                    } label: {
                        Text(verbatim: p.name + " • " + (p.scope ?? "…"))
                    }
                }
                Button {
                    Task { await source.use(nil) }
                } label: {
                    Text("integration.newProfile", bundle: .module)
                }
                TextField(text: $source.draft.name) { Text("integration.name", bundle: .module) }
                Button {
                    Task { await source.saveProfile(chatGPT: chatGPT) }
                } label: {
                    Text("integration.saveProfile", bundle: .module)
                }
                if let profile = source.profile {
                    LabeledContent {
                        if let account = profile.scope {
                            Text(verbatim: account)
                        } else {
                            Text("todoist.accountPending", bundle: .module)
                        }
                    } label: {
                        Text("todoist.account", bundle: .module)
                    }
                    if let account = profile.scope {
                        Button {
                            pendingAccount = account
                            pendingExcluded = true
                            confirmAccount = true
                        } label: {
                            Text("todoist.exclude", bundle: .module)
                        }.disabled(source.exclusions.contains(account))
                    }
                    SecureField(text: $source.draft.token) {
                        Text("todoist.token", bundle: .module)
                    }
                    Text("todoist.secretHelp", bundle: .module).font(.caption)
                    Text("todoist.replaceHelp", bundle: .module).font(.caption)
                    Button {
                        Task { await source.replaceSecret(chatGPT: chatGPT) }
                    } label: {
                        Text("todoist.replaceToken", bundle: .module)
                    }.disabled(source.draft.token.isEmpty)
                    if let available = source.secretAvailable {
                        Text(
                            LocalizedStringKey(
                                available ? "todoist.secretAvailable" : "todoist.secretMissing"),
                            bundle: .module)
                    }
                    Button(role: .destructive) {
                        confirmDelete = true
                    } label: {
                        Text("todoist.delete", bundle: .module)
                    }
                }
                if !source.exclusions.isEmpty {
                    Text("todoist.exclusions", bundle: .module)
                    ForEach(source.exclusions, id: \.self) { account in
                        HStack {
                            Text(verbatim: account)
                            Button {
                                pendingAccount = account
                                pendingExcluded = false
                                confirmAccount = true
                            } label: {
                                Text("todoist.allow", bundle: .module)
                            }
                        }
                    }
                }
                if let failure = source.failureKey {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure), bundle: .module))
                }
            }.disabled(source.changingCredential)
        } label: {
            Text("todoist.title", bundle: .module)
        }
        .task { await source.loadProfiles() }
        .confirmationDialog(Text("todoist.delete", bundle: .module), isPresented: $confirmDelete) {
            Button(role: .destructive) {
                Task { await source.deleteProfile(chatGPT: chatGPT) }
            } label: {
                Text("todoist.delete", bundle: .module)
            }
            Button(role: .cancel) {
            } label: {
                Text("auth.dismiss", bundle: .module)
            }
        } message: {
            Text("todoist.deleteHelp", bundle: .module)
        }
        .confirmationDialog(Text(verbatim: pendingAccount), isPresented: $confirmAccount) {
            Button {
                Task {
                    await source.setScopeExcluded(
                        pendingAccount, excluded: pendingExcluded, chatGPT: chatGPT)
                }
            } label: {
                Text(
                    LocalizedStringKey(pendingExcluded ? "todoist.exclude" : "todoist.allow"),
                    bundle: .module)
            }
            Button(role: .cancel) {
            } label: {
                Text("auth.dismiss", bundle: .module)
            }
        } message: {
            Text(
                LocalizedStringKey(pendingExcluded ? "todoist.excludeHelp" : "todoist.allowHelp"),
                bundle: .module)
        }
    }
}
extension TodoistAdapter: IntegrationPresentation {
    static let copy = "todoist"
    static let targetKey = "todoist.task"
    static func objectView(_ object: TodoistObject, compact: Bool) -> TodoistObjectView {
        TodoistObjectView(object: object, compact: compact)
    }
    static func coverageView(_ coverage: TodoistCoverage) -> TodoistCoverageView {
        TodoistCoverageView(coverage: coverage)
    }
    static func metadataView(_ metadata: TodoistMetadata) -> TodoistMetadataView {
        TodoistMetadataView(metadata: metadata)
    }
}
