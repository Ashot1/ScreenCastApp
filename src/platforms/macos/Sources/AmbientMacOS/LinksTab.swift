import SwiftUI

struct LinksTab: View {
    let model: AssistantModel
    let snapshot: MemorySnapshot
    @Binding var linkTarget: UInt64?
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("links.title", bundle: .module).font(.title3.bold())
            if snapshot.links.items.isEmpty {
                Text("links.empty", bundle: .module).foregroundStyle(.secondary)
            }
            ForEach(snapshot.links.items) { link in
                HStack(alignment: .center, spacing: 12) {
                    Button {
                        Task { await model.requestSelection(.task(link.otherTaskID)) }
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(verbatim: link.otherTask.text).fontWeight(.medium)
                                    .lineLimit(2)
                                LinkStatus(link: link)
                            }
                            Spacer()
                            Image(systemName: "arrow.up.forward.square")
                                .foregroundStyle(.tertiary).accessibilityHidden(true)
                        }
                        .padding(.vertical, 6)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    if link.related {
                        Button {
                            Task { await model.removeLink(link) }
                        } label: {
                            Text("links.remove", bundle: .module)
                        }
                        .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
                        .disabled(model.hasUnsavedChanges)
                    } else {
                        Pill(text: Text("links.rejected", bundle: .module))
                    }
                }
                Divider()
            }
            PageControls(
                first: model.linksAfter == 0 ? nil : { Task { await model.pageLinks(after: 0) } },
                next: snapshot.links.nextAfter.map { next in
                    { Task { await model.pageLinks(after: next) } }
                })
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
                Text("links.target", bundle: .module)
            }
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
    }
}

/// The target case's status and, for an accepted link, the date of that decision
/// (`MemoryLink.actionAtUTC`); a rejected link carries its own label instead.
private struct LinkStatus: View {
    let link: MemoryLink
    var body: some View {
        HStack(spacing: 4) {
            Text(
                link.otherTask.lifecycle == .completed ? "lifecycle.completed" : "lifecycle.open",
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
