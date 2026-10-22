import SwiftUI

/// The case's links (page 6): an inset list where each row opens the linked case, «⋯» holds
/// «Открыть дело / Убрать», and a rejected link carries its own label; then paging and the picker
/// of open cases to link.
struct LinksTab: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    @Binding var linkTarget: UInt64?
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if snapshot.links.items.isEmpty {
                Text("links.empty", bundle: .module).foregroundStyle(.secondary)
            } else {
                VStack(spacing: 0) {
                    ForEach(snapshot.links.items) { link in
                        row(link)
                        if link.id != snapshot.links.items.last?.id { Divider() }
                    }
                }
                .padding(.horizontal, 14)
                .ambientInset(cornerRadius: 16)
            }
            PageControls(
                first: model.linksAfter == 0 ? nil : { Task { await model.pageLinks(after: 0) } },
                next: snapshot.links.nextAfter.map { next in
                    { Task { await model.pageLinks(after: next) } }
                })
            VStack(alignment: .leading, spacing: 8) {
                Text("links.linkTo", bundle: .module).font(.callout.weight(.medium))
                HStack {
                    Picker(selection: $linkTarget) {
                        Text("links.target", bundle: .module).tag(Optional<UInt64>.none)
                        ForEach(
                            snapshot.tasks.items.filter {
                                $0.id != snapshot.selectedTask?.id && $0.lifecycle == .open
                            }
                        ) {
                            task in
                            Text(verbatim: String(task.text.prefix(80))).tag(Optional(task.id))
                        }
                    } label: {
                        Text("links.linkTo", bundle: .module)
                    }
                    .labelsHidden()
                    Button {
                        if let target = snapshot.tasks.items.first(where: { $0.id == linkTarget }) {
                            Task { await model.link(to: target, related: true) }
                        }
                    } label: {
                        Text("links.add", bundle: .module)
                    }
                    .buttonStyle(.bordered).buttonBorderShape(.capsule)
                    .disabled(
                        model.hasUnsavedChanges
                            || !snapshot.tasks.items.contains {
                                $0.id == linkTarget && $0.id != snapshot.selectedTask?.id
                            })
                }
                Text("links.openOnly", bundle: .module).font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.top, 8)
        }
    }
    private func row(_ link: MemoryLink) -> some View {
        HStack(spacing: 8) {
            Button {
                open(link)
            } label: {
                HStack {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(verbatim: link.otherTask.text).fontWeight(.medium).lineLimit(2)
                            .foregroundStyle(link.related ? .primary : .secondary)
                        LinkStatus(link: link)
                    }
                    Spacer(minLength: 8)
                    if link.related {
                        Image(systemName: "chevron.right").font(.caption.weight(.semibold))
                            .foregroundStyle(.tertiary).accessibilityHidden(true)
                    }
                }
                .padding(.vertical, 10)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if link.related {
                Menu {
                    Button {
                        open(link)
                    } label: {
                        Text("links.open", bundle: .module)
                    }
                    Button {
                        Task { await model.removeLink(link) }
                    } label: {
                        Text("links.remove", bundle: .module)
                    }
                    .disabled(model.hasUnsavedChanges)
                } label: {
                    Image(systemName: "ellipsis")
                }
                .menuStyle(.button).menuIndicator(.hidden)
                .buttonStyle(.bordered).buttonBorderShape(.circle).controlSize(.small)
                .accessibilityLabel(Text("links.actions", bundle: .module))
            } else {
                Pill(text: Text("links.rejected", bundle: .module))
            }
        }
    }
    private func open(_ link: MemoryLink) {
        Task { await model.requestSelection(.task(link.otherTaskID)) }
    }
}

/// The target case's status and, for an accepted link, the date of that decision
/// (`MemoryLink.actionAtUTC`); a rejected link's time is the rejection, so it shows no date.
private struct LinkStatus: View {
    let link: MemoryLink
    var body: some View {
        HStack(spacing: 4) {
            Text(
                link.otherTask.lifecycle == .completed ? "case.completed" : "case.open",
                bundle: .module)
            if link.related {
                Text(verbatim: "·").accessibilityHidden(true)
                let date = Date(timeIntervalSince1970: TimeInterval(link.actionAtUTC))
                    .formatted(.dateTime.day().month())
                Text("links.since \(date)", bundle: .module)
            }
        }
        .font(.caption).foregroundStyle(.secondary)
    }
}
