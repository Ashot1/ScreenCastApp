import SwiftUI

struct ObservationSettingsView: View {
    let model: AssistantModel
    let session: ObservationSession
    @State private var creditAcknowledged = false
    @State private var confirmingDelete = false
    private var display: ObservationDisplay { session.display }

    var body: some View {
        Form {
            Section {
                Text("observation.warning", bundle: .module)
                Text("observation.retention", bundle: .module).foregroundStyle(.secondary)
            }
            Section {
                LabeledContent {
                    Label {
                        Text(LocalizedStringKey(display.state.key), bundle: .module)
                    } icon: {
                        Image(systemName: display.state.symbol)
                    }
                } label: {
                    Text("settings.status", bundle: .module)
                }
                if display.loading {
                    ProgressView { Text("observation.loading", bundle: .module) }
                }
                if display.working {
                    ProgressView { Text("observation.working", bundle: .module) }
                }
                if let failure = display.failure {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure.key), bundle: .module))
                }
                LabeledContent {
                    Text(permissionKey, bundle: .module)
                } label: {
                    Text("permissions.screenCapture", bundle: .module)
                }
                Text("observation.permissionHelp", bundle: .module).foregroundStyle(.secondary)
            }
            if let journal = display.journal {
                Section {
                    if journal.policy.started {
                        Text("observation.creditSaved", bundle: .module)
                    } else {
                        Toggle(isOn: $creditAcknowledged) {
                            Text("check.report", bundle: .module)
                        }
                    }
                    Text("observation.creditHelp", bundle: .module).foregroundStyle(.secondary)
                    HStack {
                        if journal.policy.started {
                            Button {
                                Task { await perform { try await session.stop() } }
                            } label: {
                                Text("observation.stop", bundle: .module)
                            }.buttonStyle(.bordered).buttonBorderShape(.capsule)
                            if display.state == .needsPermission {
                                startButton(journal.policy, retry: true)
                            }
                        } else {
                            startButton(journal.policy, retry: false)
                                .disabled(!creditAcknowledged)
                        }
                    }
                    Text("observation.restoreHelp", bundle: .module).foregroundStyle(.secondary)
                }.disabled(locked)
                Section {
                    Stepper(
                        value: Binding(
                            get: { journal.policy.perHourLimit },
                            set: { limit in
                                Task { await perform { try await session.setHourlyLimit(limit) } }
                            }),
                        in: 1...60
                    ) {
                        HStack {
                            Text("observation.limit", bundle: .module)
                            Spacer()
                            Text(journal.policy.perHourLimit, format: .number)
                        }
                    }.disabled(locked)
                    Text("observation.limitHelp", bundle: .module).foregroundStyle(.secondary)
                }
                Section {
                    Text("observation.journalHelp", bundle: .module).foregroundStyle(.secondary)
                    LabeledContent {
                        Text(journal.logFrames, format: .number)
                    } label: {
                        Text("observation.frames", bundle: .module)
                    }
                    LabeledContent {
                        Text(journal.unsentFrames, format: .number)
                    } label: {
                        Text("observation.unsent", bundle: .module)
                    }
                    LabeledContent {
                        Text(journal.hourAttempts, format: .number)
                    } label: {
                        Text("observation.attempts", bundle: .module)
                    }
                    if journal.sends.isEmpty {
                        Text("observation.empty", bundle: .module).foregroundStyle(.secondary)
                    } else {
                        ForEach(journal.sends, id: \.attempt) { send in
                            ObservationSendView(send: send)
                        }
                    }
                } header: {
                    Text("observation.journal", bundle: .module)
                }
            }
            Section {
                HStack {
                    Button {
                        Task { await session.refreshJournal() }
                    } label: {
                        Text("observation.refresh", bundle: .module)
                    }
                    if display.journal != nil {
                        Button(role: .destructive) {
                            confirmingDelete = true
                        } label: {
                            Text("observation.delete", bundle: .module)
                        }
                    }
                }.buttonStyle(.bordered).buttonBorderShape(.capsule).disabled(locked)
            }
        }
        .confirmationDialog(
            Text("observation.delete", bundle: .module), isPresented: $confirmingDelete,
            titleVisibility: .visible
        ) {
            Button(role: .destructive) {
                Task { await perform { try await session.deleteJournal() } }
            } label: {
                Text("observation.delete", bundle: .module)
            }
            Button(role: .cancel) {
            } label: {
                Text("switch.cancel", bundle: .module)
            }
        } message: {
            Text("observation.deleteNotice", bundle: .module)
        }
    }
    private func perform(_ operation: () async throws -> Void) async {
        do { try await operation() } catch { await session.record(error) }
    }
    private var locked: Bool { model.closing || display.working || display.loading }
    private var permissionKey: LocalizedStringKey {
        switch display.permission {
        case true: "permissions.granted"
        case false: "permissions.notGranted"
        case nil: "permissions.unchecked"
        }
    }
    private func startButton(_ policy: ObservationPolicy, retry: Bool) -> some View {
        Button {
            Task {
                await perform {
                    try await session.start(
                        creditAcknowledged: policy.started
                            ? policy.creditAcknowledged : creditAcknowledged,
                        perHourLimit: policy.perHourLimit)
                }
            }
        } label: {
            Text(retry ? "observation.retryPermission" : "observation.start", bundle: .module)
        }.ambientPrimary()
    }
}

private struct ObservationSendView: View {
    let send: ObservationSend
    var body: some View {
        DisclosureGroup {
            LabeledContent {
                Text(verbatim: send.apps.joined(separator: ", "))
            } label: {
                Text("observation.apps", bundle: .module)
            }
            LabeledContent {
                Text(verbatim: "\(send.firstFrame)–\(send.lastFrame)")
            } label: {
                Text("observation.framesRange", bundle: .module)
            }
            LabeledContent {
                Text(send.images, format: .number)
            } label: {
                Text("observation.images", bundle: .module)
            }
            LabeledContent {
                Text(send.droppedFrames, format: .number)
            } label: {
                Text("observation.lossFrames", bundle: .module)
            }
            LabeledContent {
                Text(send.droppedBytes, format: .number)
            } label: {
                Text("observation.lossBytes", bundle: .module)
            }
            LabeledContent {
                Text(send.droppedImages, format: .number)
            } label: {
                Text("observation.lossImages", bundle: .module)
            }
            LabeledContent {
                Text(
                    LocalizedStringKey(
                        send.outcome == .pending ? "observation.pending" : send.outcome.key),
                    bundle: .module)
            } label: {
                Text("observation.outcome", bundle: .module)
            }
            LabeledContent {
                switch send.tier {
                case .missing: Text("check.absent", bundle: .module)
                case .priority: Text(verbatim: "priority")
                case .fast: Text(verbatim: "fast")
                case .default: Text(verbatim: "default")
                case .other: Text("check.other", bundle: .module)
                }
            } label: {
                Text("observation.tier", bundle: .module)
            }
            LabeledContent {
                Text(send.addedCards, format: .number)
            } label: {
                Text("observation.added", bundle: .module)
            }
            LabeledContent {
                Text(send.rejectedItems, format: .number)
            } label: {
                Text("observation.rejected", bundle: .module)
            }
        } label: {
            HStack {
                Text(Date(timeIntervalSince1970: TimeInterval(send.atUtc)), format: .dateTime)
                Spacer()
                Text(
                    LocalizedStringKey(
                        send.outcome == .pending ? "observation.pending" : send.outcome.key),
                    bundle: .module)
            }
        }
    }
}
