import SwiftUI

struct GitHubMetadataView: View {
    let metadata: GitHubMetadata
    private func date(_ value: Int64?) -> String? {
        value.map { Date(timeIntervalSince1970: TimeInterval($0)).formatted(.iso8601) }
    }
    private func row(_ key: String, _ value: String?, unknown: Bool = false) -> some View {
        LabeledContent {
            if let value {
                HStack {
                    if unknown { Text("integration.unknown", bundle: .module) }
                    Text(verbatim: value).textSelection(.enabled)
                }
            } else {
                Text("github.notProvided", bundle: .module)
            }
        } label: {
            Text(LocalizedStringKey(key), bundle: .module)
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("integration.history", bundle: .module).font(.caption).foregroundStyle(.secondary)
            Text(verbatim: "\(metadata.repositoryFullName) #\(metadata.prNumber)")
            Text(LocalizedStringKey("github.kind.\(metadata.fields.kind)"), bundle: .module)
            row("integration.author", metadata.fields.author)
            if let state = metadata.fields.state {
                row("github.state", state.raw, unknown: state.unknown)
            }
            if metadata.appProjection { Text("github.projection", bundle: .module).font(.caption) }
            if metadata.fields.bodyProvided == false { Text("github.bodyMissing", bundle: .module) }
            DisclosureGroup {
                row("github.prID", String(metadata.parentPRID))
                if metadata.fields.kind != "review" {
                    row("integration.created", date(metadata.fields.createdAt))
                }
                if metadata.fields.kind == "pr" {
                    row("github.head", metadata.fields.headSHA)
                    row("github.base", metadata.fields.baseSHA)
                    LabeledContent {
                        Image(systemName: metadata.fields.merged == true ? "checkmark" : "xmark")
                    } label: {
                        Text("github.merged", bundle: .module)
                    }
                }
                if metadata.fields.kind == "review" || metadata.fields.kind == "inline" {
                    row("github.commit", metadata.fields.commitID)
                }
                if metadata.fields.kind == "inline" {
                    row("github.originalCommit", metadata.fields.originalCommitID)
                    row("github.reviewID", metadata.fields.reviewID.map(String.init))
                    row("github.replyID", metadata.fields.replyID.map(String.init))
                    row("github.path", metadata.fields.path)
                    row("github.line", metadata.fields.line.map(String.init))
                    row("github.originalLine", metadata.fields.originalLine.map(String.init))
                    row(
                        "github.side", metadata.fields.side?.raw,
                        unknown: metadata.fields.side?.unknown == true)
                    row(
                        "github.subject", metadata.fields.subjectType?.raw,
                        unknown: metadata.fields.subjectType?.unknown == true)
                    row("github.startLine", metadata.fields.startLine.map(String.init))
                    row(
                        "github.originalStartLine",
                        metadata.fields.originalStartLine.map(String.init))
                    row(
                        "github.startSide", metadata.fields.startSide?.raw,
                        unknown: metadata.fields.startSide?.unknown == true)
                }
                row(
                    "integration.interval",
                    "\(Date(timeIntervalSince1970: TimeInterval(metadata.intervalStart)).formatted(.iso8601)) — \(Date(timeIntervalSince1970: TimeInterval(metadata.intervalEnd)).formatted(.iso8601))"
                )
            } label: {
                Text("sources.title", bundle: .module)
            }
        }
    }
}
struct GitHubObjectView: View {
    let object: GitHubObject
    var compact = false
    var body: some View {
        VStack(alignment: .leading) {
            GitHubMetadataView(metadata: object.metadata)
            IntegrationSourceTimeView(time: object.observedAtUTC)
            if !compact {
                LabeledContent {
                    Text(verbatim: object.key.scope)
                } label: {
                    Text("github.repositoryID", bundle: .module)
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
struct GitHubCoverageView: View {
    let coverage: GitHubCoverage
    var body: some View {
        HStack {
            Text(LocalizedStringKey("github.kind.\(coverage.kind)"), bundle: .module)
            Text(
                LocalizedStringKey(
                    ["not_read", "read"].contains(coverage.status)
                        ? "integration.status.\(coverage.status)"
                        : "github.status.\(coverage.status)"
                ), bundle: .module)
            Text("integration.pages", bundle: .module)
            Text(verbatim: "\(coverage.pages)")
            Text("github.losses", bundle: .module)
            Text(verbatim: "\(coverage.losses)")
        }.font(.caption)
    }
}
struct GitHubSettingsView: View {
    @Bindable var source: GitHubSourcesModel
    let chatGPT: ChatGPTModel
    @State private var confirmDelete = false
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("github.help", bundle: .module)
                Text("github.allowlistHelp", bundle: .module).font(.caption)
                ForEach(source.profiles) { p in
                    Button(p.name) { Task { await source.use(p) } }
                }
                Button {
                    Task { await source.use(nil) }
                } label: {
                    Text("integration.newProfile", bundle: .module)
                }
                TextField(text: $source.draft.name) { Text("integration.name", bundle: .module) }
                Text("github.repositories", bundle: .module)
                TextEditor(text: $source.draft.repositories).frame(minHeight: 70)
                Text("github.settingsHelp", bundle: .module).font(.caption)
                Button {
                    Task { await source.saveProfile(chatGPT: chatGPT) }
                } label: {
                    Text("integration.saveProfile", bundle: .module)
                }
                if source.profile != nil {
                    SecureField(text: $source.draft.token) {
                        Text("github.token", bundle: .module)
                    }
                    Text("github.tokenHelp", bundle: .module).font(.caption)
                    Text("github.replaceHelp", bundle: .module).font(.caption)
                    Button {
                        Task { await source.replaceSecret(chatGPT: chatGPT) }
                    } label: {
                        Text("github.replaceToken", bundle: .module)
                    }.disabled(source.draft.token.isEmpty)
                    if let available = source.secretAvailable {
                        Text(
                            LocalizedStringKey(
                                available ? "github.secretAvailable" : "github.secretMissing"),
                            bundle: .module)
                    }
                    Button(role: .destructive) {
                        confirmDelete = true
                    } label: {
                        Text("github.delete", bundle: .module)
                    }
                }
                if let failure = source.failureKey {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure), bundle: .module))
                }
            }.disabled(source.changingCredential)
        } label: {
            Text("github.title", bundle: .module)
        }
        .task { await source.loadProfiles() }
        .confirmationDialog(Text("github.delete", bundle: .module), isPresented: $confirmDelete) {
            Button(role: .destructive) {
                Task { await source.deleteProfile(chatGPT: chatGPT) }
            } label: {
                Text("github.delete", bundle: .module)
            }
            Button(role: .cancel) {
            } label: {
                Text("auth.dismiss", bundle: .module)
            }
        } message: {
            Text("github.deleteHelp", bundle: .module)
        }
    }
}
extension GitHubAdapter: IntegrationPresentation {
    static let copy = "github"
    static let targetKey = "github.url"
    static func objectView(_ object: GitHubObject, compact: Bool) -> GitHubObjectView {
        GitHubObjectView(object: object, compact: compact)
    }
    static func coverageView(_ coverage: GitHubCoverage) -> GitHubCoverageView {
        GitHubCoverageView(coverage: coverage)
    }
    static func metadataView(_ metadata: GitHubMetadata) -> GitHubMetadataView {
        GitHubMetadataView(metadata: metadata)
    }
}
