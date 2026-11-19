import SwiftUI

/// Settings section: installed plugins, their switch, sign-in, pinned tools, call log, removal.
struct PluginsSettingsView: View {
    let plugins: PluginsModel
    let chatGPT: ChatGPTModel
    @State private var expanded: UInt64?
    @State private var removing: PluginRecord?
    var body: some View {
        Form {
            Section {
                Text("plugins.summary", bundle: .module)
                DisclosureGroup {
                    Text("plugins.help", bundle: .module)
                    Text("plugins.readOnlyWarning", bundle: .module)
                } label: {
                    Text("settings.details", bundle: .module)
                }
                Callout(
                    tone: Theme.stale, systemImage: "exclamationmark.triangle",
                    message: Text("plugins.riskSummary", bundle: .module)
                ) {
                    DisclosureGroup {
                        Text("plugins.dataNotice", bundle: .module)
                    } label: {
                        Text("settings.details", bundle: .module)
                    }
                }
            }
            Section {
                HStack {
                    Button {
                        Task { await plugins.addGoogle() }
                    } label: {
                        Text("plugins.addGoogle", bundle: .module)
                    }
                    Button {
                        Task { await plugins.chooseFolder() }
                    } label: {
                        Text("plugins.addFolder", bundle: .module)
                    }
                    Menu {
                        ForEach(plugins.codexFolders, id: \.self) { url in
                            Button {
                                Task { await plugins.open(url) }
                            } label: {
                                let name = url.pathComponents.suffix(3).joined(separator: " / ")
                                Text(verbatim: name)
                            }
                        }
                    } label: {
                        Text("plugins.addCodex", bundle: .module)
                    }
                }.disabled(plugins.working)
            }.controlSize(.small)
            Section {
                if plugins.plugins.isEmpty {
                    Text("plugins.noInstalled", bundle: .module).foregroundStyle(.secondary)
                }
                ForEach(plugins.plugins) { p in
                    DisclosureGroup(
                        isExpanded: Binding(
                            get: { expanded == p.id },
                            set: { open in
                                expanded = open ? p.id : nil
                                if open { Task { await plugins.rediscover(p) } }
                            })
                    ) {
                        PluginDetailView(plugin: p, plugins: plugins, chatGPT: chatGPT)
                        Button(role: .destructive) {
                            removing = p
                        } label: {
                            Text("plugins.remove", bundle: .module)
                        }.disabled(plugins.working)
                    } label: {
                        Toggle(
                            isOn: Binding(
                                get: { p.enabled },
                                set: { on in
                                    Task {
                                        await plugins.setEnabled(p, enabled: on, chatGPT: chatGPT)
                                    }
                                })
                        ) {
                            Text(verbatim: p.version.isEmpty ? p.name : "\(p.name) \(p.version)")
                        }.disabled(plugins.working || p.tools.isEmpty && !p.enabled)
                    }
                }
            } header: {
                Text("plugins.installed", bundle: .module)
            }
            Section {
                if let failure = plugins.failureKey {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure), bundle: .module))
                }
                PluginCallLogView(calls: plugins.calls)
            }
        }
        .task { await plugins.load() }
        .sheet(
            isPresented: Binding(
                get: { plugins.draft != nil }, set: { if !$0 { plugins.draft = nil } })
        ) {
            PluginReviewView(plugins: plugins)
        }
        .confirmationDialog(
            Text("plugins.confirmRemove", bundle: .module),
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } })
        ) {
            Button(role: .destructive) {
                if let p = removing { Task { await plugins.delete(p, chatGPT: chatGPT) } }
            } label: {
                Text("plugins.remove", bundle: .module)
            }
        } message: {
            Text("plugins.removeNotice", bundle: .module)
        }
    }
}

/// Servers with their sign-in state, and pinned tools marked when the server changed them.
private struct PluginDetailView: View {
    let plugin: PluginRecord
    let plugins: PluginsModel
    let chatGPT: ChatGPTModel
    @State private var tokens: [String: String] = [:]
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(plugin.servers, id: \.key) { s in
                PluginServerHeader(key: s.key, kind: s.kind, downloads: s.downloads)
                if s.kind.isLocal {
                    PluginLocalSettings(
                        plugin: plugin, server: s, plugins: plugins, chatGPT: chatGPT)
                } else if let id = s.kind.managedID {
                    PluginManagedSettings(
                        plugin: plugin, server: s, id: id, plugins: plugins, chatGPT: chatGPT)
                } else if s.needsKey {
                    if plugins.signedIn[plugin.id]?.contains(s.key) == true {
                        HStack {
                            Text("plugins.signedIn", bundle: .module)
                            Button {
                                Task {
                                    await plugins.signOut(plugin, server: s.key, chatGPT: chatGPT)
                                }
                            } label: {
                                Text("plugins.signOut", bundle: .module)
                            }
                        }
                    } else {
                        HStack {
                            SecureField(
                                text: Binding(
                                    get: { tokens[s.key, default: ""] },
                                    set: { tokens[s.key] = $0 })
                            ) {
                                Text("plugins.token", bundle: .module)
                            }
                            Button {
                                let token = tokens[s.key, default: ""]
                                tokens[s.key] = nil
                                Task {
                                    await plugins.saveToken(
                                        plugin, server: s.key, token: token, chatGPT: chatGPT)
                                }
                            } label: {
                                Text("plugins.signIn", bundle: .module)
                            }.disabled(tokens[s.key, default: ""].isEmpty)
                        }
                        Text("plugins.signInRequired", bundle: .module).font(.caption)
                    }
                } else {
                    Text("plugins.noSignIn", bundle: .module).font(.caption)
                }
            }
            if plugin.servers.contains(where: { !$0.kind.startsProcess }) {
                Text("plugins.readOnlyWarning", bundle: .module).font(.caption)
            }
            ForEach(plugin.tools, id: \.name) { pin in
                HStack {
                    Text(verbatim: "\(pin.server) / \(pin.name)")
                    if plugins.changed(plugin, pin) {
                        Text("plugins.changed", bundle: .module).foregroundStyle(.orange)
                        Button {
                            Task { await plugins.repin(plugin, pin, chatGPT: chatGPT) }
                        } label: {
                            Text("plugins.approveAgain", bundle: .module)
                        }
                    }
                    Button {
                        Task { await plugins.unapprove(plugin, pin, chatGPT: chatGPT) }
                    } label: {
                        Text("plugins.unapprove", bundle: .module)
                    }
                }
            }
            // While a program runs for discovery, only its «Cancel» needs to stay usable.
        }.disabled(plugins.working && plugins.running == nil)
    }
}

/// The review before a plugin is used: instructions, servers, tools and the read-only notice.
private struct PluginReviewView: View {
    let plugins: PluginsModel
    var body: some View {
        if let d = plugins.draft {
            VStack(alignment: .leading, spacing: 0) {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "shippingbox").font(.title2)
                    VStack(alignment: .leading, spacing: 6) {
                        let title =
                            d.review.version.isEmpty
                            ? d.review.name : "\(d.review.name) \(d.review.version)"
                        Text(verbatim: title).font(.title3.bold())
                        Text("plugins.reviewHelp", bundle: .module).font(.caption)
                    }
                    Spacer(minLength: 0)
                }.padding(20)
                ScrollView {
                    HStack(alignment: .top, spacing: 20) {
                        VStack(alignment: .leading, spacing: 12) {
                            ForEach(d.review.warnings, id: \.self) {
                                Text(verbatim: $0).font(.caption).foregroundStyle(.orange)
                            }
                            Text("plugins.skills", bundle: .module).font(.headline)
                            if d.review.skills.isEmpty { Text("plugins.noSkills", bundle: .module) }
                            ForEach(d.review.skills, id: \.name) { skill in
                                DisclosureGroup {
                                    Text(verbatim: skill.text).font(.caption.monospaced())
                                        .textSelection(.enabled)
                                } label: {
                                    let label =
                                        skill.description.isEmpty
                                        ? skill.name : "\(skill.name): \(skill.description)"
                                    Text(verbatim: label)
                                }
                            }
                            Text("plugins.servers", bundle: .module).font(.headline)
                            ForEach(d.review.servers, id: \.key) { s in
                                PluginServerHeader(key: s.key, kind: s.kind, downloads: s.downloads)
                                    .padding(12).ambientInset()
                            }
                            if d.review.servers.contains(where: { !$0.kind.startsProcess }) {
                                Text("plugins.readOnlyWarning", bundle: .module).font(.callout)
                            }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                        VStack(alignment: .leading, spacing: 12) {
                            Text("plugins.access", bundle: .module).font(.headline)
                            ForEach(d.review.servers, id: \.key) { s in
                                Text(verbatim: s.key).font(.headline)
                                if d.plugin == nil && s.needsKey && !s.kind.startsProcess {
                                    SecureField(
                                        text: Binding(
                                            get: {
                                                plugins.draft?.tokens[s.key, default: ""] ?? ""
                                            },
                                            set: { plugins.draft?.tokens[s.key] = $0 })
                                    ) {
                                        Text("plugins.token", bundle: .module)
                                    }
                                }
                                if d.plugin == nil {
                                    ForEach(s.kind.envNames, id: \.self) { name in
                                        SecureField(
                                            text: Binding(
                                                get: { plugins.draft?.values[s.key]?[name] ?? "" },
                                                set: {
                                                    plugins.draft?.values[s.key, default: [:]][
                                                        name] = $0
                                                })
                                        ) { Text(verbatim: name) }
                                    }
                                } else if let id = s.kind.managedID, let p = d.plugin,
                                    let server = p.servers.first(where: { $0.key == s.key })
                                {
                                    PluginManagedControls(
                                        plugin: p, server: server, id: id, plugins: plugins,
                                        chatGPT: nil
                                    ) {
                                        await plugins.runDraft(server: s.key)
                                    }
                                } else if s.kind.isLocal {
                                    PluginRunButton(
                                        running: plugins.running?.server == s.key, plugins: plugins
                                    ) {
                                        await plugins.runDraft(server: s.key)
                                    }
                                }
                                PluginDiscoveryStatus(found: d.discoveries[s.key])
                                Text("plugins.tools", bundle: .module).font(.headline)
                                ForEach(d.discoveries[s.key]?.tools ?? []) { t in
                                    Toggle(
                                        isOn: Binding(
                                            get: {
                                                let id = "\(s.key)/\(t.name)"
                                                return plugins.draft?.selected.contains(id) == true
                                            },
                                            set: { on in
                                                if on {
                                                    plugins.draft?.selected.insert(
                                                        "\(s.key)/\(t.name)")
                                                } else {
                                                    plugins.draft?.selected.remove(
                                                        "\(s.key)/\(t.name)")
                                                }
                                            })
                                    ) {
                                        VStack(alignment: .leading) {
                                            Text(verbatim: t.name)
                                            if let description = t.description {
                                                Text(verbatim: description).font(.caption)
                                            }
                                            let mark =
                                                t.readOnly
                                                ? "plugins.declaredReadOnly" : "plugins.notReadOnly"
                                            Text(LocalizedStringKey(mark), bundle: .module)
                                                .font(.caption).foregroundStyle(.secondary)
                                        }
                                    }.disabled(!t.approvable)
                                }
                            }
                            DisclosureGroup {
                                Text("plugins.dataNotice", bundle: .module).font(.caption)
                            } label: {
                                Text("settings.details", bundle: .module)
                            }
                            if let failure = plugins.failureKey {
                                Callout(
                                    tone: Theme.error, systemImage: "exclamationmark.circle",
                                    message: Text(LocalizedStringKey(failure), bundle: .module))
                            }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }.padding(.horizontal, 20).padding(.bottom, 20)
                }
                Divider()
                HStack {
                    Spacer()
                    Button(role: .cancel) {
                        plugins.draft = nil
                    } label: {
                        Text("plugins.close", bundle: .module)
                    }
                    if d.plugin == nil {
                        Button {
                            Task { await plugins.installDraft() }
                        } label: {
                            Text("plugins.install", bundle: .module)
                        }.ambientPrimary()
                    } else {
                        Button {
                            Task { await plugins.enableDraft() }
                        } label: {
                            Text("plugins.enable", bundle: .module)
                        }.ambientPrimary().disabled(d.selected.isEmpty)
                    }
                }.disabled(plugins.working)
                    .padding(16)
            }
            .background(Color(nsColor: .textBackgroundColor))
            .frame(width: 892, height: 756)
        }
    }
}

/// The call log: what was sent where, when, and the outcome (the control instead of approvals).
private struct PluginCallLogView: View {
    let calls: [PluginCall]
    var body: some View {
        DisclosureGroup {
            if calls.isEmpty { Text("plugins.noCalls", bundle: .module).font(.caption) }
            ForEach(calls) { call in
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: "\(call.pluginName) · \(call.server) / \(call.tool)")
                        .font(.caption.bold())
                    Text(Date(timeIntervalSince1970: TimeInterval(call.atUTC)), format: .dateTime)
                        .font(.caption)
                    Text(verbatim: call.arguments).font(.caption.monospaced())
                        .textSelection(.enabled)
                    Text(
                        LocalizedStringKey("plugins.outcome." + String(call.outcome)),
                        bundle: .module
                    )
                    .font(.caption).foregroundStyle(.secondary)
                }
            }
        } label: {
            Text("plugins.callLog", bundle: .module)
        }
    }
}

/// Provenance of a cited saved plugin result.
struct PluginCitationView: View {
    let citation: PluginCitation
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(
                "plugins.citation \(citation.plugin) \(citation.server) \(citation.tool)",
                bundle: .module
            ).font(.caption)
            Text(Date(timeIntervalSince1970: TimeInterval(citation.calledAtUTC)), format: .dateTime)
                .font(.caption)
            if !citation.complete {
                Text("plugins.partial", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// A server line: the URL of a remote server, or exactly what a local server will run and the
/// honest notice about a program on this Mac (ADR-022 «Что показывает просмотр»).
private struct PluginServerHeader: View {
    let key: String
    let kind: PluginServerKind
    let downloads: Bool
    var body: some View {
        switch kind {
        case .remote(let url):
            Text(verbatim: "\(key) — \(url)").font(.caption.monospaced())
        case .local(let command, let args, let env):
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: key).font(.caption.bold())
                Text("plugins.local.title", bundle: .module).font(.caption)
                LabeledContent {
                    Text(verbatim: command).font(.caption.monospaced()).textSelection(.enabled)
                } label: {
                    Text("plugins.local.command", bundle: .module).font(.caption)
                }
                if !args.isEmpty {
                    LabeledContent {
                        VStack(alignment: .leading) {
                            ForEach(Array(args.enumerated()), id: \.offset) {
                                Text(verbatim: $0.element).font(.caption.monospaced())
                                    .textSelection(.enabled)
                            }
                        }
                    } label: {
                        Text("plugins.local.args", bundle: .module).font(.caption)
                    }
                }
                LabeledContent {
                    Text(verbatim: env.isEmpty ? "—" : env.joined(separator: ", "))
                        .font(.caption.monospaced())
                } label: {
                    Text("plugins.local.env", bundle: .module).font(.caption)
                }
                Group {
                    Text("plugins.local.warning", bundle: .module).foregroundStyle(.orange)
                    if downloads {
                        Text("plugins.local.downloads", bundle: .module).foregroundStyle(.orange)
                    }
                    DisclosureGroup {
                        Text("plugins.local.readOnlyClaim", bundle: .module)
                        Text("plugins.local.whenRuns", bundle: .module)
                        Text("plugins.local.arguments", bundle: .module)
                        if !env.isEmpty { Text("plugins.local.secrets", bundle: .module) }
                        Text("plugins.local.orphans", bundle: .module)
                        Text("plugins.local.path", bundle: .module)
                        Text("plugins.local.packageFiles", bundle: .module)
                    } label: {
                        Text("settings.details", bundle: .module)
                    }
                }.font(.caption)
            }
        case .managed:
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: key).font(.caption.bold())
                Text("plugins.google.title", bundle: .module).font(.caption)
                Group {
                    Text("plugins.local.warning", bundle: .module).foregroundStyle(.orange)
                    DisclosureGroup {
                        Text("plugins.google.downloads", bundle: .module)
                        Text("plugins.google.protection", bundle: .module)
                        Text("plugins.google.network", bundle: .module)
                        Text("plugins.google.scopes", bundle: .module)
                        Text("plugins.google.data", bundle: .module)
                        Text("plugins.google.injection", bundle: .module)
                        Text("plugins.google.testingMode", bundle: .module)
                        Text("plugins.local.whenRuns", bundle: .module)
                        Text("plugins.local.arguments", bundle: .module)
                        Text("plugins.local.orphans", bundle: .module)
                        Text("plugins.google.updates", bundle: .module)
                    } label: {
                        Text("plugins.important", bundle: .module)
                    }
                }.font(.caption)
            }
        }
    }
}
/// The managed Google server in the plugin settings: install, sign-in and «Run and show tools».
private struct PluginManagedSettings: View {
    let plugin: PluginRecord
    let server: PluginServer
    let id: String
    let plugins: PluginsModel
    let chatGPT: ChatGPTModel
    var body: some View {
        PluginManagedControls(
            plugin: plugin, server: server, id: id, plugins: plugins, chatGPT: chatGPT
        ) {
            await plugins.runInstalled(plugin, server: server.key)
        }
        PluginDiscoveryStatus(found: plugins.live[plugin.id]?[server.key])
    }
}
/// Install state with «Install», «Update», «Cancel» and «Remove server»; then «Sign in with
/// Google» and «Run and show tools» once the server is installed (ADR-022).
private struct PluginManagedControls: View {
    let plugin: PluginRecord
    let server: PluginServer
    let id: String
    let plugins: PluginsModel
    /// Nil in the review sheet, where removing the server is not offered.
    let chatGPT: ChatGPTModel?
    let run: () async -> Void
    /// The official uv install command, shown for copying; Ambient never runs it (D69).
    private static let uvCommand = "curl -LsSf https://astral.sh/uv/install.sh | sh"
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            let status = plugins.managedStatus[id]
            if plugins.installing?.id == id {
                HStack {
                    ProgressView().controlSize(.small)
                    Text("plugins.google.installing", bundle: .module)
                    Button(role: .cancel) {
                        plugins.cancelInstall()
                    } label: {
                        Text("plugins.cancelRun", bundle: .module)
                    }
                }
            } else if let status {
                let key = "plugins.google.status." + status.rawValue
                Text(LocalizedStringKey(key), bundle: .module)
                switch status {
                case .uvMissing:
                    Text(verbatim: Self.uvCommand).font(.caption.monospaced())
                        .textSelection(.enabled)
                case .notInstalled, .outdated:
                    Button {
                        Task { await plugins.installManaged(id) }
                    } label: {
                        let label =
                            status == .outdated ? "plugins.google.update" : "plugins.google.install"
                        Text(LocalizedStringKey(label), bundle: .module)
                    }.disabled(plugins.working)
                case .installed:
                    if let chatGPT {
                        Button(role: .destructive) {
                            Task { await plugins.removeManaged(id, chatGPT: chatGPT) }
                        } label: {
                            Text("plugins.google.remove", bundle: .module)
                        }.disabled(plugins.working)
                    }
                }
            }
            if let result = plugins.installResult, result.status != .installed {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    message: Text(
                        LocalizedStringKey("plugins.google.result." + result.status.rawValue),
                        bundle: .module))
                if let tail = result.stderrTail {
                    Text("plugins.stderr", bundle: .module).font(.caption)
                    Text(verbatim: tail).font(.caption2.monospaced()).textSelection(.enabled)
                }
            }
            PluginGoogleSignIn(plugin: plugin, server: server, plugins: plugins, chatGPT: chatGPT)
            if status == .installed {
                PluginRunButton(
                    running: plugins.running?.plugin == plugin.id
                        && plugins.running?.server == server.key, plugins: plugins, action: run)
            }
        }
    }
}
/// «Sign in with Google» through the browser, the signed-in state with the testing-mode date,
/// and «Sign in again» after Google ended the grant.
private struct PluginGoogleSignIn: View {
    let plugin: PluginRecord
    let server: PluginServer
    let plugins: PluginsModel
    let chatGPT: ChatGPTModel?
    var body: some View {
        let expired = plugins.expired.contains(
            PluginEnvelope.account(plugin: plugin.id, server: server.key))
        if plugins.signedIn[plugin.id]?.contains(server.key) == true {
            HStack {
                Text("plugins.google.signedIn", bundle: .module)
                if let chatGPT {
                    Button {
                        Task { await plugins.signOut(plugin, server: server.key, chatGPT: chatGPT) }
                    } label: {
                        Text("plugins.signOut", bundle: .module)
                    }
                }
            }
            if let issued = plugins.signedInAt[plugin.id]?[server.key] {
                let until = Date(timeIntervalSince1970: TimeInterval(issued + 7 * 24 * 3600))
                let date = until.formatted(date: .abbreviated, time: .shortened)
                Text("plugins.google.signInUntil \(date)", bundle: .module).font(.caption)
            }
        } else {
            if expired {
                Text("plugins.google.expired", bundle: .module).foregroundStyle(.orange)
            }
            HStack {
                Button {
                    Task { await plugins.signInWithGoogle(plugin, server: server.key) }
                } label: {
                    Text("plugins.google.signIn", bundle: .module)
                }.disabled(plugins.working)
                if plugins.signingIn {
                    ProgressView().controlSize(.small)
                    Text("plugins.google.waiting", bundle: .module).font(.caption)
                    Button(role: .cancel) {
                        plugins.cancelSignIn()
                    } label: {
                        Text("plugins.cancelRun", bundle: .module)
                    }
                }
            }
        }
    }
}
/// «Run and show tools»: starts the program for one discovery; «Cancel» stops it.
private struct PluginRunButton: View {
    let running: Bool
    let plugins: PluginsModel
    let action: () async -> Void
    var body: some View {
        HStack {
            Button {
                Task { await action() }
            } label: {
                Text("plugins.run", bundle: .module)
            }.disabled(plugins.working)
            if running {
                ProgressView().controlSize(.small)
                Button(role: .cancel) {
                    plugins.cancelRun()
                } label: {
                    Text("plugins.cancelRun", bundle: .module)
                }
            }
        }
    }
}
/// A failed discovery and, for a local program, its error output and an unconfirmed stop.
private struct PluginDiscoveryStatus: View {
    let found: Discovery?
    var body: some View {
        if let found, found.status != .ok {
            let key = "plugins.discovery." + found.status.rawValue
            Callout(
                tone: Theme.error, systemImage: "exclamationmark.circle",
                message: Text(LocalizedStringKey(key), bundle: .module))
            if let stderr = found.stderr {
                Text("plugins.stderr", bundle: .module).font(.caption)
                Text(verbatim: stderr).font(.caption2.monospaced()).textSelection(.enabled)
            }
        }
        if found?.stopConfirmed == false {
            Text("plugins.stopUnconfirmed", bundle: .module).font(.caption).foregroundStyle(.red)
        }
    }
}
/// A local server's variable values and «Run and show tools» in the plugin settings.
private struct PluginLocalSettings: View {
    let plugin: PluginRecord
    let server: PluginServer
    let plugins: PluginsModel
    let chatGPT: ChatGPTModel
    @State private var values: [String: String] = [:]
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            let names = server.kind.envNames
            if names.isEmpty {
                Text("plugins.local.noValues", bundle: .module).font(.caption)
            } else if plugins.signedIn[plugin.id]?.contains(server.key) == true {
                HStack {
                    Text("plugins.local.valuesSaved", bundle: .module)
                    Button {
                        Task { await plugins.signOut(plugin, server: server.key, chatGPT: chatGPT) }
                    } label: {
                        Text("plugins.local.clearValues", bundle: .module)
                    }
                }
            } else {
                ForEach(names, id: \.self) { name in
                    SecureField(
                        text: Binding(
                            get: { values[name, default: ""] }, set: { values[name] = $0 })
                    ) { Text(verbatim: name) }
                }
                Button {
                    let entered = values
                    values = [:]
                    Task {
                        await plugins.saveValues(
                            plugin, server: server.key, values: entered, chatGPT: chatGPT)
                    }
                } label: {
                    Text("plugins.local.saveValues", bundle: .module)
                }.disabled(names.contains { values[$0, default: ""].isEmpty })
                Text("plugins.local.valuesRequired", bundle: .module).font(.caption)
            }
            PluginRunButton(
                running: plugins.running?.plugin == plugin.id
                    && plugins.running?.server == server.key, plugins: plugins
            ) {
                await plugins.runInstalled(plugin, server: server.key)
            }
            PluginDiscoveryStatus(found: plugins.live[plugin.id]?[server.key])
        }
    }
}
