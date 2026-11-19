import SwiftUI

struct JiraMetadataView: View {
    let metadata: JiraMetadata
    private func row(_ key: String, _ value: JiraPresence<String>?) -> some View {
        LabeledContent {
            switch value {
            case .value(let text): Text(verbatim: text).textSelection(.enabled)
            case .null: Text("jira.null", bundle: .module)
            case .invalid: Text("jira.invalid", bundle: .module)
            case .missing, .none: Text("jira.missing", bundle: .module)
            }
        } label: {
            Text(LocalizedStringKey(key), bundle: .module)
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("integration.history", bundle: .module).font(.caption).foregroundStyle(.secondary)
            Text(verbatim: metadata.cloudID + " • " + metadata.content.observedIssueKey)
            Text(LocalizedStringKey("jira.kind." + metadata.content.tag), bundle: .module)
            Text("jira.projection", bundle: .module).font(.caption)
            row("jira.status", metadata.content.fields.status?.flatMap { $0.name })
            row("jira.issueType", metadata.content.fields.issueType?.flatMap { $0.name })
            row("jira.project", metadata.content.fields.project?.flatMap { $0.name })
            row("integration.author", metadata.content.fields.author?.flatMap { $0.displayName })
            if let target = metadata.content.fields.targetID { Text(verbatim: target) }
            if let label = metadata.content.fields.label { Text(verbatim: label) }
            if let body = metadata.content.fields.description ?? metadata.content.fields.body {
                row("jira.body", body.text)
                ForEach(body.losses, id: \.self) { JiraLossView(reason: $0) }
            }
            if let items = metadata.content.fields.items {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    VStack(alignment: .leading) {
                        row("jira.field", item.field)
                        row("jira.from", item.fromString)
                        row("jira.to", item.toString)
                    }
                }
            }
            ForEach(Array(metadata.content.losses.enumerated()), id: \.offset) { _, loss in
                JiraLossView(reason: loss)
            }
            ForEach(metadata.coverage) { category in
                JiraCoverageView(coverage: category)
            }
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
struct JiraLossView: View {
    let reason: String
    var stateKey: String {
        switch reason.split(separator: ":", maxSplits: 1).first {
        case "missing": "jira.missing"
        case "null": "jira.null"
        default: "jira.invalid"
        }
    }
    var body: some View {
        if let colon = reason.firstIndex(of: ":") {
            HStack {
                Text(
                    LocalizedStringKey(stateKey),
                    bundle: .module)
                Text(verbatim: String(reason[reason.index(after: colon)...]))
            }.font(.caption)
        } else {
            Text(LocalizedStringKey("jira.reason." + reason), bundle: .module).font(.caption)
        }
    }
}
struct JiraCoverageView: View {
    let coverage: JiraCoverage
    var body: some View {
        VStack(alignment: .leading) {
            HStack {
                Text(LocalizedStringKey("jira.category." + coverage.kind), bundle: .module)
                Text(
                    LocalizedStringKey(
                        coverage.status == "partial"
                            ? "jira.status.partial" : "integration.status.\(coverage.status)"),
                    bundle: .module)
                Text("integration.pages", bundle: .module)
                Text(verbatim: "\(coverage.pages)")
                Text("integration.objects", bundle: .module)
                Text(verbatim: "\(coverage.objects)")
            }.font(.caption)
            if let total = coverage.observedTotal {
                LabeledContent {
                    Text(verbatim: "\(total)")
                } label: {
                    Text("jira.total", bundle: .module)
                }
            }
            if coverage.omittedPrefix { Text("jira.omittedPrefix", bundle: .module) }
            ForEach(coverage.reasons, id: \.self) { JiraLossView(reason: $0) }
        }
    }
}
struct JiraObjectView: View {
    let object: JiraObject
    var compact = false
    var body: some View {
        VStack(alignment: .leading) {
            JiraMetadataView(metadata: object.metadata)
            IntegrationSourceTimeView(time: object.observedAtUTC)
            if !compact {
                LabeledContent {
                    Text(verbatim: object.key.scope)
                } label: {
                    Text("jira.cloudID", bundle: .module)
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
            Text(verbatim: object.text).lineLimit(compact ? 3 : nil).textSelection(.enabled)
            if !object.bindable { Text("integration.ambiguous", bundle: .module) }
        }
    }
}
struct JiraSettingsView: View {
    @Bindable var source: JiraSourcesModel
    let chatGPT: ChatGPTModel
    @State private var confirmDelete = false
    @State private var confirmSite = false
    @State private var pendingSite = ""
    @State private var pendingExcluded = false
    var body: some View {
        Form {
            Section {
                DisclosureGroup {
                    Text("jira.help", bundle: .module)
                    Text("jira.siteHelp", bundle: .module)
                } label: {
                    Text("settings.details", bundle: .module)
                }
            } header: {
                Text("jira.title", bundle: .module)
            }
            Section {
                ForEach(source.profiles) { p in
                    Button {
                        Task { await source.use(p) }
                    } label: {
                        Text(verbatim: p.name + " • " + (p.scope ?? ""))
                    }
                }
                Button {
                    Task { await source.use(nil) }
                } label: {
                    Text("integration.newProfile", bundle: .module)
                }
            }
            Section {
                TextField(text: $source.draft.name) { Text("integration.name", bundle: .module) }
                TextField(text: $source.draft.cloudID) { Text("jira.cloudID", bundle: .module) }
                    .disabled(
                        source.profile != nil)
                Button {
                    Task { await source.saveProfile(chatGPT: chatGPT) }
                } label: {
                    Text("integration.saveProfile", bundle: .module)
                }
                Button {
                    pendingSite = source.draft.cloudID
                    pendingExcluded = true
                    confirmSite = true
                } label: {
                    Text("jira.exclude", bundle: .module)
                }.disabled(
                    source.draft.cloudID.isEmpty || source.exclusions.contains(source.draft.cloudID)
                )
            }
            if source.profile != nil {
                Section {
                    TextField(text: $source.draft.email) { Text("jira.email", bundle: .module) }
                    SecureField(text: $source.draft.token) { Text("jira.token", bundle: .module) }
                    DisclosureGroup {
                        Text("jira.secretHelp", bundle: .module).font(.caption)
                        Text("jira.replaceHelp", bundle: .module).font(.caption)
                    } label: {
                        Text("settings.details", bundle: .module)
                    }
                    Button {
                        Task { await source.replaceSecret(chatGPT: chatGPT) }
                    } label: {
                        Text("jira.replaceToken", bundle: .module)
                    }.disabled(source.draft.email.isEmpty || source.draft.token.isEmpty)
                    if let available = source.secretAvailable {
                        Text(
                            LocalizedStringKey(
                                available ? "jira.secretAvailable" : "jira.secretMissing"),
                            bundle: .module)
                    }
                    Button(role: .destructive) {
                        confirmDelete = true
                    } label: {
                        Text("jira.delete", bundle: .module)
                    }
                }
            }
            if !source.exclusions.isEmpty {
                Section {
                    ForEach(source.exclusions, id: \.self) { site in
                        HStack {
                            Text(verbatim: site)
                            Button {
                                pendingSite = site
                                pendingExcluded = false
                                confirmSite = true
                            } label: {
                                Text("jira.allow", bundle: .module)
                            }
                        }
                    }
                } header: {
                    Text("jira.exclusions", bundle: .module)
                }
            }
            if let failure = source.failureKey {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    message: Text(LocalizedStringKey(failure), bundle: .module))
            }
        }.disabled(source.changingCredential)
            .task { await source.loadProfiles() }
            .confirmationDialog(Text("jira.delete", bundle: .module), isPresented: $confirmDelete) {
                Button(role: .destructive) {
                    Task { await source.deleteProfile(chatGPT: chatGPT) }
                } label: {
                    Text("jira.delete", bundle: .module)
                }
                Button(role: .cancel) {
                } label: {
                    Text("auth.dismiss", bundle: .module)
                }
            } message: {
                Text("jira.deleteHelp", bundle: .module)
            }
            .confirmationDialog(Text(verbatim: pendingSite), isPresented: $confirmSite) {
                Button {
                    Task {
                        await source.setScopeExcluded(
                            pendingSite, excluded: pendingExcluded, chatGPT: chatGPT)
                    }
                } label: {
                    Text(
                        LocalizedStringKey(pendingExcluded ? "jira.exclude" : "jira.allow"),
                        bundle: .module)
                }
                Button(role: .cancel) {
                } label: {
                    Text("auth.dismiss", bundle: .module)
                }
            } message: {
                Text(
                    LocalizedStringKey(pendingExcluded ? "jira.excludeHelp" : "jira.allowHelp"),
                    bundle: .module)
            }
    }
}
extension JiraAdapter: IntegrationPresentation {
    static let copy = "jira"
    static let targetKey = "jira.issue"
    static func objectView(_ object: JiraObject, compact: Bool) -> JiraObjectView {
        JiraObjectView(object: object, compact: compact)
    }
    static func coverageView(_ coverage: JiraCoverage) -> JiraCoverageView {
        JiraCoverageView(coverage: coverage)
    }
    static func metadataView(_ metadata: JiraMetadata) -> JiraMetadataView {
        JiraMetadataView(metadata: metadata)
    }
}
