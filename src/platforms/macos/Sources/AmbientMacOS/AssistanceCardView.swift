import SwiftUI

/// Each window owns its scroll position and its clicked row; both surfaces share Core rows and one
/// independent draft.
struct AssistanceCardView: View {
    let model: AssistantModel
    @Bindable var chatGPT: ChatGPTModel
    var edge = false
    var contentHeightChanged: ((CGFloat) -> Void)?
    @State private var scrollAnchor: UInt64?
    @State private var clickedProposal: UInt64?
    @State private var detailsProposal: UInt64?
    @FocusState private var editingReply: Bool
    private var rows: [Proposal] {
        if edge {
            return chatGPT.assistanceReady && model.manualPause == false
                ? chatGPT.feed.filter(\.visible) : []
        }
        return chatGPT.feed
    }
    var body: some View {
        // The one scroll of the feed: the hosts do not wrap it, so the position stays per window.
        ScrollView {
            content.padding(.horizontal, edge ? 16 : 36).padding(.vertical, edge ? 16 : 32)
        }
        .scrollPosition(id: $scrollAnchor, anchor: .top)
        .onScrollGeometryChange(for: CGFloat.self) { geometry in
            geometry.contentSize.height + geometry.contentInsets.top + geometry.contentInsets.bottom
        } action: { _, height in
            contentHeightChanged?(height)
        }
        .safeAreaInset(edge: .bottom, alignment: .leading) {
            if !edge {
                Label {
                    Text("feed.storage", bundle: .module)
                } icon: {
                    Image(systemName: "lock")
                }
                .font(.caption).foregroundStyle(.secondary)
                .padding(.horizontal, 36).padding(.vertical, 16)
            }
        }
    }
    private var content: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            if !edge {
                Text("feed.saved", bundle: .module).foregroundStyle(.secondary)
                    .padding(.bottom, 12)
            }
            if !chatGPT.assistanceReady || model.manualPause != false {
                Label {
                    Text("feed.unavailable", bundle: .module)
                } icon: {
                    Image(systemName: "info.circle")
                }
                .font(.caption).foregroundStyle(.secondary)
            }
            let expanded = rows.expandedID(reply: chatGPT.replyParent, clicked: clickedProposal)
            LazyVStack(alignment: .leading, spacing: 8) {
                ForEach(rows) { proposal in
                    Group {
                        if proposal.id == expanded { card(proposal) } else { row(proposal) }
                    }.id(proposal.id)
                }
            }.scrollTargetLayout()
            if rows.isEmpty && (!edge || chatGPT.feed.isEmpty) {
                ContentUnavailableView {
                    Label {
                        Text("feed.empty", bundle: .module)
                    } icon: {
                        Image(systemName: "tray")
                    }
                } description: {
                    Text("feed.emptyHelp", bundle: .module)
                }
            }
            // A draft whose parent is no longer shown remains editable and can still be discarded.
            if (chatGPT.replyParent != nil || !chatGPT.replyDraft.isEmpty)
                && !rows.contains(where: { $0.target == chatGPT.replyParent })
            {
                replyEditor.padding(16)
                    .background(fill(0.9), in: RoundedRectangle(cornerRadius: 22))
            }
            if let preview = chatGPT.assistancePreview {
                if preview.mode == .newContext {
                    Callout(
                        tone: Theme.stale, systemImage: "exclamationmark.triangle",
                        message: Text("feed.replace", bundle: .module))
                }
                DisclosureGroup {
                    ScrollView {
                        Text(verbatim: preview.context).font(.system(.caption, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }.frame(maxHeight: 180)
                } label: {
                    Text("assistance.preview", bundle: .module)
                }
                Button {
                    Task { await chatGPT.sendPreparedAssistance() }
                } label: {
                    Text("assistance.send", bundle: .module)
                }
                .ambientPrimary()
                .disabled(chatGPT.busy || model.manualPause != false)
            }
            if chatGPT.assisting {
                WorkRow(
                    title: Text("reply.sending", bundle: .module),
                    cancel: { Task { await chatGPT.cancel() } },
                    disabled: chatGPT.state == .cancelling
                )
                .background(fill(0.62), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
            if let failure = chatGPT.replyFailure ?? chatGPT.assistanceFailure {
                Callout(
                    tone: Theme.error, systemImage: "exclamationmark.circle",
                    title: Text("error.title", bundle: .module),
                    message: Text(
                        LocalizedStringKey(failure == .limit ? "feed.limit" : failure.key),
                        bundle: .module))
            }
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            if edge {
                Text("feed.title", bundle: .module).font(.headline)
                    .accessibilityAddTraits(.isHeader)
                // ADR-023 #21: the edge header counts the rows it shows.
                Text(rows.count, format: .number).font(.headline).foregroundStyle(.secondary)
                Spacer()
                Button {
                    Task { await chatGPT.hideEdge() }
                } label: {
                    Text("edge.hide", bundle: .module)
                }.buttonStyle(.plain)
            } else {
                Text("feed.title", bundle: .module).font(.system(size: 26, weight: .semibold))
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                Button {
                    Task { await chatGPT.refreshCurrentResult() }
                } label: {
                    Label {
                        Text("feed.reload", bundle: .module)
                    } icon: {
                        Image(systemName: "arrow.clockwise")
                    }
                }.buttonStyle(.bordered).buttonBorderShape(.capsule)
            }
        }
    }
    /// The case text when the case is on the loaded page, otherwise the previous number.
    private func caseTitle(_ proposal: Proposal) -> String {
        model.snapshot?.tasks.items.first { $0.id == proposal.task.taskID }?.text
            ?? caseNumber(proposal)
    }
    private func caseNumber(_ proposal: Proposal) -> String {
        "\(String(localized: "edge.task", bundle: .module)) \(proposal.task.taskID.formatted())"
    }
    private func metadata(_ proposal: Proposal) -> some View {
        HStack(spacing: 4) {
            Text(
                LocalizedStringKey(proposal.stale ? "feed.outdated" : proposal.kind.key),
                bundle: .module
            )
            .fontWeight(.semibold)
            .foregroundStyle(
                proposal.stale
                    ? Theme.stale.text
                    : proposal.kind == .clarification ? Theme.clarification.text : .primary)
            Text(verbatim: "·").accessibilityHidden(true)
            Text(verbatim: caseTitle(proposal))
                .lineLimit(1).truncationMode(.tail).help(caseNumber(proposal))
            if !proposal.visible { Pill(text: Text("feed.closed", bundle: .module)) }
        }.font(.caption).foregroundStyle(.secondary)
    }
    /// The design's cards are opaque on the edge glass and quiet on the window's white sheet.
    private func fill(_ opacity: Double) -> AnyShapeStyle {
        edge ? AnyShapeStyle(.background.opacity(opacity)) : AnyShapeStyle(.fill.quaternary)
    }
    private func actionsDisabled(_ proposal: Proposal) -> Bool {
        chatGPT.busy || chatGPT.preparingReply || !chatGPT.assistanceReady || proposal.stale
            || model.manualPause != false
    }
    private var newDisabled: Bool {
        chatGPT.busy || !chatGPT.assistanceReady || model.manualPause != false
    }
    /// A collapsed row: a click expands it in this window; its one action is the card's main one.
    private func row(_ proposal: Proposal) -> some View {
        HStack(spacing: 12) {
            Button {
                clickedProposal = proposal.id
            } label: {
                VStack(alignment: .leading, spacing: 5) {
                    metadata(proposal)
                    Text(verbatim: proposal.message).lineLimit(1).truncationMode(.tail)
                }.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
            }
            .buttonStyle(.plain).help(Text(verbatim: proposal.message))
            Group {
                if proposal.stale {
                    newButton(proposal, short: true).disabled(newDisabled)
                } else if proposal.kind == .clarification {
                    replyButton(proposal, short: true).disabled(actionsDisabled(proposal))
                } else {
                    continueButton(proposal).disabled(actionsDisabled(proposal))
                }
            }.buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .background(fill(0.62), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay {
            if proposal.stale && !edge {
                RoundedRectangle(cornerRadius: 20).stroke(.quaternary)
            }
        }
    }
    private func card(_ proposal: Proposal) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                metadata(proposal)
                Spacer(minLength: 4)
                menu(proposal)
            }
            Text(verbatim: proposal.message).font(.title3)
                .fixedSize(horizontal: false, vertical: true)
            if proposal.stale {
                Callout(
                    tone: Theme.stale, systemImage: "clock.arrow.circlepath",
                    title: Text("feed.outdated", bundle: .module),
                    message: Text("feed.stale", bundle: .module)
                ) {
                    newButton(proposal).disabled(newDisabled)
                }
            }
            DisclosureGroup(
                isExpanded: Binding(
                    get: { detailsProposal == proposal.id },
                    set: { expanded in
                        detailsProposal = expanded ? proposal.id : nil
                        if expanded { Task { await chatGPT.loadDetails(proposal) } }
                    })
            ) {
                Text(verbatim: proposal.rationale)
                if let details = chatGPT.details, details.proposal.target == proposal.target {
                    ForEach(details.citations) { citation in
                        DisclosureGroup {
                            if let question = citation.modelQuestion {
                                Text("reply.question", bundle: .module).font(.caption.bold())
                                Text(verbatim: question)
                                Text("reply.user", bundle: .module).font(.caption.bold())
                            }
                            switch citation.integration {
                            case .github(let metadata): GitHubMetadataView(metadata: metadata)
                            case .jira(let metadata): JiraMetadataView(metadata: metadata)
                            case .todoist(let metadata): TodoistMetadataView(metadata: metadata)
                            case nil: EmptyView()
                            }
                            if citation.integration != nil {
                                IntegrationSourceTimeView(time: citation.sourceObservedAtUTC)
                            }
                            if let plugin = citation.plugin { PluginCitationView(citation: plugin) }
                            Text(verbatim: citation.text)
                        } label: {
                            Text(LocalizedStringKey(citation.key), bundle: .module)
                        }
                    }
                }
            } label: {
                Text("feed.details", bundle: .module)
            }
            // Long labels wrap to a column instead of clipping in the 420 pt edge panel.
            ViewThatFits(in: .horizontal) {
                HStack { actions(proposal) }
                VStack(alignment: .leading) { actions(proposal) }
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule)
            .disabled(actionsDisabled(proposal))
            if chatGPT.replyParent == proposal.target {
                replyEditor
            }
            if !edge {
                HStack {
                    Spacer()
                    if proposal.visible {
                        Text("edge.shown", bundle: .module).foregroundStyle(.secondary)
                    } else {
                        visibilityButton(proposal)
                    }
                }.font(.caption)
            }
        }
        .padding(16)
        .background(fill(0.9), in: RoundedRectangle(cornerRadius: 22, style: .continuous))
    }
    @ViewBuilder private func actions(_ proposal: Proposal) -> some View {
        if proposal.kind == .clarification {
            replyButton(proposal).ambientPrimary()
        } else {
            copyButton(proposal).ambientPrimary()
        }
        continueButton(proposal)
    }
    private func copyButton(_ proposal: Proposal) -> some View {
        Button {
            Task { await chatGPT.copy(proposal, using: model) }
        } label: {
            Text("assistance.copy", bundle: .module)
        }
    }
    private func replyButton(_ proposal: Proposal, short: Bool = false) -> some View {
        Button {
            chatGPT.chooseReply(proposal)
            editingReply = chatGPT.replyParent == proposal.target
        } label: {
            Text(short ? "feed.replyShort" : "feed.reply", bundle: .module)
        }
    }
    private func continueButton(_ proposal: Proposal) -> some View {
        Button {
            Task { await chatGPT.prepareContinuation(proposal) }
        } label: {
            Text("feed.continue", bundle: .module)
        }
    }
    private func newButton(_ proposal: Proposal, short: Bool = false) -> some View {
        Button {
            Task { await chatGPT.prepareNewContext(proposal) }
        } label: {
            Text(short ? "feed.restart" : "feed.new", bundle: .module)
        }
    }
    private func visibilityButton(_ proposal: Proposal) -> some View {
        Button {
            Task { await chatGPT.setVisibility(proposal, visible: !proposal.visible) }
        } label: {
            Text(proposal.visible ? "edge.hide" : "edge.show", bundle: .module)
        }
        .buttonStyle(.borderless).controlSize(.small)
        .disabled(
            !proposal.visible && (!chatGPT.assistanceReady || proposal.stale || chatGPT.busy))
    }
    private func menu(_ proposal: Proposal) -> some View {
        Menu {
            if proposal.kind == .clarification {
                copyButton(proposal).disabled(actionsDisabled(proposal))
            }
            newButton(proposal).disabled(newDisabled)
            Button {
                Task {
                    await chatGPT.suppress(proposal)
                    await model.reload()
                }
            } label: {
                Text("edge.suppress", bundle: .module)
            }.disabled(chatGPT.busy || proposal.stale)
            visibilityButton(proposal)
            Divider()
            Button(role: .destructive) {
                Task { await chatGPT.clearDiscussion(proposal) }
            } label: {
                Text("feed.clear", bundle: .module)
            }
        } label: {
            Image(systemName: "ellipsis")
        }
        .menuStyle(.button).menuIndicator(.hidden)
        .buttonStyle(.plain).buttonBorderShape(.circle).controlSize(.small)
        .accessibilityLabel(Text("feed.actions", bundle: .module))
    }
    private var replyEditor: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("reply.title", bundle: .module).font(.caption).foregroundStyle(.secondary)
            if let proposal = chatGPT.replyProposal,
                !rows.contains(where: { $0.target == proposal.target })
            {
                Text(verbatim: proposal.message).font(.caption).foregroundStyle(.secondary)
            }
            if chatGPT.replyConflict {
                Callout(
                    tone: Theme.stale, systemImage: "exclamationmark.triangle",
                    message: Text("feed.draftConflict", bundle: .module))
            }
            if chatGPT.replyIsStale {
                Callout(
                    tone: Theme.stale, systemImage: "exclamationmark.triangle",
                    message: Text("reply.stale", bundle: .module))
            }
            // This editor has no content-revision identity and remains when its target disappears.
            TextEditor(text: Binding(get: { chatGPT.replyDraft }, set: { chatGPT.updateReply($0) }))
                .frame(height: 100).focused($editingReply)
                .scrollContentBackground(.hidden).padding(8)
                .background(.fill.quaternary, in: RoundedRectangle(cornerRadius: 12))
                .accessibilityLabel(Text("reply.placeholder", bundle: .module))
                .disabled(chatGPT.busy || chatGPT.voice.busy)
                .onTapGesture { editingReply = true }
            HStack {
                Button {
                    chatGPT.updateReply(String(localized: "reply.yes", bundle: .module))
                } label: {
                    Text("reply.yes", bundle: .module)
                }
                Button {
                    chatGPT.updateReply(String(localized: "reply.no", bundle: .module))
                } label: {
                    Text("reply.no", bundle: .module)
                }
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
            .disabled(chatGPT.busy || chatGPT.voice.busy)
            VoiceReplyView(chatGPT: chatGPT)
            if chatGPT.preparingReply {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("reply.preparing", bundle: .module).font(.caption)
                }
            }
            if let preview = chatGPT.replyPreview {
                DisclosureGroup(isExpanded: $chatGPT.previewExpanded) {
                    ScrollView {
                        Text(verbatim: preview.context).font(.system(.caption, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }.frame(maxHeight: 180)
                } label: {
                    Text("reply.preview", bundle: .module)
                }
            }
            Button {
                Task { await chatGPT.sendReply() }
            } label: {
                Text("reply.send", bundle: .module)
            }
            .ambientPrimary()
            .disabled(!chatGPT.canSendReply || model.manualPause != false)
            Button {
                Task { await chatGPT.discardReply() }
            } label: {
                Text("reply.discard", bundle: .module)
            }
            .buttonStyle(.plain).font(.caption)
            .disabled(chatGPT.busy || chatGPT.voice.busy)
            DetailsFootnote(text: Text("reply.persistence", bundle: .module)) {
                Text("reply.persistenceDetails", bundle: .module)
            }
        }
    }
}
