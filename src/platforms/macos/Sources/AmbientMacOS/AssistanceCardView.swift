import SwiftUI

/// Each window owns its scroll position; both surfaces share Core rows and one independent draft.
struct AssistanceCardView: View {
    let model: AssistantModel
    @Bindable var chatGPT: ChatGPTModel
    var edge = false
    @State private var scrollAnchor: UInt64?
    @State private var expandedProposal: UInt64?
    @FocusState private var editingReply: Bool
    private var rows: [Proposal] {
        if edge {
            return chatGPT.assistanceReady && model.manualPause == false
                ? chatGPT.feed.filter(\.visible) : []
        }
        return chatGPT.feed
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("feed.title", bundle: .module).font(.headline)
                Spacer()
                if edge {
                    Button {
                        Task { await chatGPT.hideEdge() }
                    } label: {
                        Text("edge.hide", bundle: .module)
                    }
                } else {
                    Button {
                        Task { await chatGPT.refreshCurrentResult() }
                    } label: {
                        Text("feed.reload", bundle: .module)
                    }
                }
            }
            if !chatGPT.assistanceReady || model.manualPause != false {
                Text("feed.unavailable", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(rows) { proposal in
                        proposalRow(proposal).id(proposal.id)
                    }
                }.scrollTargetLayout()
            }
            .scrollPosition(id: $scrollAnchor, anchor: .top)
            .frame(minHeight: 80, maxHeight: edge ? 320 : 440)
            if rows.isEmpty { Text("feed.empty", bundle: .module).foregroundStyle(.secondary) }
            if chatGPT.replyParent != nil || !chatGPT.replyDraft.isEmpty {
                replyEditor
            }
            if let preview = chatGPT.assistancePreview {
                if preview.mode == .newContext {
                    Text("feed.replace", bundle: .module).font(.caption)
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
                }.disabled(chatGPT.busy || model.manualPause != false)
            }
            if chatGPT.assisting {
                Text("reply.sending", bundle: .module)
                Button {
                    Task { await chatGPT.cancel() }
                } label: {
                    Text("check.cancel", bundle: .module)
                }.disabled(chatGPT.state == .cancelling)
            }
            if let failure = chatGPT.replyFailure ?? chatGPT.assistanceFailure {
                Text(
                    LocalizedStringKey(failure == .limit ? "feed.limit" : failure.key),
                    bundle: .module
                ).foregroundStyle(.red)
            }
            if model.copied { Text("plan.copied", bundle: .module).font(.caption) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func proposalRow(_ proposal: Proposal) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("edge.task", bundle: .module)
                Text(proposal.task.taskID, format: .number)
            }.font(.caption)
            HStack {
                Text(LocalizedStringKey(proposal.kind.key), bundle: .module).font(
                    .subheadline.bold())
                if !proposal.visible { Text("feed.closed", bundle: .module).font(.caption) }
                Spacer()
                Button {
                    Task { await chatGPT.setVisibility(proposal, visible: !proposal.visible) }
                } label: {
                    Text(proposal.visible ? "edge.hide" : "edge.show", bundle: .module)
                }
                .disabled(
                    !proposal.visible
                        && (!chatGPT.assistanceReady || proposal.stale || chatGPT.busy))
            }
            Text(verbatim: proposal.message).fixedSize(horizontal: false, vertical: true)
            if proposal.stale {
                Text("feed.stale", bundle: .module).font(.caption).foregroundStyle(.secondary)
            }
            DisclosureGroup(
                isExpanded: Binding(
                    get: { expandedProposal == proposal.id },
                    set: { expanded in
                        expandedProposal = expanded ? proposal.id : nil
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
            VStack(alignment: .leading) {
                Button {
                    Task { await chatGPT.copy(proposal, using: model) }
                } label: {
                    Text("assistance.copy", bundle: .module)
                }
                if proposal.kind == .clarification {
                    Button {
                        chatGPT.chooseReply(proposal)
                        editingReply = chatGPT.replyParent == proposal.target
                    } label: {
                        Text("feed.reply", bundle: .module)
                    }
                }
                Button {
                    Task { await chatGPT.prepareContinuation(proposal) }
                } label: {
                    Text("feed.continue", bundle: .module)
                }
            }.disabled(
                chatGPT.busy || chatGPT.preparingReply || !chatGPT.assistanceReady || proposal.stale
                    || model.manualPause != false)
            VStack(alignment: .leading) {
                Button {
                    Task { await chatGPT.prepareNewContext(proposal) }
                } label: {
                    Text("feed.new", bundle: .module)
                }.disabled(chatGPT.busy || !chatGPT.assistanceReady || model.manualPause != false)
                Button {
                    Task {
                        await chatGPT.suppress(proposal)
                        await model.reload()
                    }
                } label: {
                    Text("edge.suppress", bundle: .module)
                }.disabled(chatGPT.busy || proposal.stale)
                Button {
                    Task { await chatGPT.clearDiscussion(proposal) }
                } label: {
                    Text("feed.clear", bundle: .module)
                }
            }
        }
        .padding(12).background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 10))
    }
    private var replyEditor: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("reply.title", bundle: .module).font(.subheadline.bold())
            if let proposal = chatGPT.replyProposal {
                Text(verbatim: proposal.message).font(.caption)
            }
            if chatGPT.replyConflict {
                Text("feed.draftConflict", bundle: .module).foregroundStyle(.secondary)
            }
            if chatGPT.replyIsStale {
                Text("reply.stale", bundle: .module).foregroundStyle(.secondary)
            }
            VoiceReplyView(chatGPT: chatGPT)
            // This editor has no content-revision identity and remains when its target disappears.
            TextEditor(text: Binding(get: { chatGPT.replyDraft }, set: { chatGPT.updateReply($0) }))
                .frame(height: 100).focused($editingReply)
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
                Button {
                    Task { await chatGPT.discardReply() }
                } label: {
                    Text("reply.discard", bundle: .module)
                }
            }.disabled(chatGPT.busy || chatGPT.voice.busy)
            if chatGPT.preparingReply { Text("reply.preparing", bundle: .module).font(.caption) }
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
            }.disabled(!chatGPT.canSendReply || model.manualPause != false)
            Text("reply.persistence", bundle: .module).font(.caption).foregroundStyle(.secondary)
        }
    }
}
