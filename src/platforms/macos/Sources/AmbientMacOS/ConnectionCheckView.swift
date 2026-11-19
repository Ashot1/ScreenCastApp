import SwiftUI

struct ConnectionCheckView: View {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    var body: some View {
        Form {
            Section {
                Text("check.summary", bundle: .module)
                DisclosureGroup {
                    Text("check.help", bundle: .module).foregroundStyle(.secondary)
                    if let url = URL(string: "https://chatgpt.com") {
                        Link(destination: url) { Text("check.limits", bundle: .module) }
                    }
                } label: {
                    Text("settings.details", bundle: .module)
                }
            }
            Section {
                Toggle(
                    isOn: Binding(
                        get: { chatGPT.creditReported },
                        set: { value in
                            Task { await chatGPT.reportCreditSetting(value) }
                        })
                ) {
                    Text("check.report", bundle: .module)
                }.disabled(chatGPT.state != .saved || chatGPT.busy)
                Text("check.reportHelp", bundle: .module).font(.caption)
                Picker(
                    selection: Binding(
                        get: { chatGPT.checkProfile }, set: { chatGPT.checkProfile = $0 })
                ) {
                    Text(verbatim: "Low").tag(ModelProfile.low)
                    Text(verbatim: "High").tag(ModelProfile.high)
                } label: {
                    Text("check.profile", bundle: .module)
                }.disabled(chatGPT.busy)
            } header: {
                Text("check.before", bundle: .module)
            }
            Section {
                if chatGPT.checking {
                    Text(
                        chatGPT.state == .cancelling ? "check.cancelling" : "check.working",
                        bundle: .module)
                    Button {
                        Task { await chatGPT.cancel() }
                    } label: {
                        Text("check.cancel", bundle: .module)
                    }.disabled(chatGPT.state == .cancelling)
                } else {
                    Button {
                        Task { await chatGPT.checkConnection() }
                    } label: {
                        Text("check.run", bundle: .module)
                    }.disabled(
                        chatGPT.state != .saved || !chatGPT.creditReported
                            || chatGPT.checkSummary == nil || model.manualPause != false
                            || !model.canToggleManualPause)
                }
                if let failure = chatGPT.journalFailure {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(LocalizedStringKey(failure.key), bundle: .module))
                }
                if let failure = chatGPT.checkFailure {
                    Callout(
                        tone: Theme.error, systemImage: "exclamationmark.circle",
                        message: Text(
                            LocalizedStringKey(
                                failure == .storage ? "check.error.recording" : failure.key),
                            bundle: .module))
                }
            } header: {
                Text("check.title", bundle: .module)
            }
            if let summary = chatGPT.checkSummary {
                Section {
                    LabeledContent {
                        Text(summary.attempts, format: .number)
                    } label: {
                        Text("check.attempts", bundle: .module)
                    }
                    LabeledContent {
                        Text(summary.input, format: .number)
                    } label: {
                        Text("check.input", bundle: .module)
                    }
                    LabeledContent {
                        Text(summary.output, format: .number)
                    } label: {
                        Text("check.output", bundle: .module)
                    }
                    LabeledContent {
                        Text(summary.total, format: .number)
                    } label: {
                        Text("check.total", bundle: .module)
                    }
                    LabeledContent {
                        Text(summary.unknown, format: .number)
                    } label: {
                        Text("check.unknown", bundle: .module)
                    }
                } header: {
                    Text("check.journal", bundle: .module)
                }
                Section {
                    if let latest = summary.latest {
                        Text(LocalizedStringKey(latest.outcome.key), bundle: .module)
                        Text(
                            Date(timeIntervalSince1970: TimeInterval(latest.atUTC)),
                            format: .dateTime)
                        if let key = latest.purpose.key {
                            Text(LocalizedStringKey(key), bundle: .module)
                        }
                        Text("check.requested", bundle: .module)
                        Text(verbatim: "gpt-6-luna · \(latest.profile.effort) · priority")
                        Text("check.actual", bundle: .module)
                        HStack {
                            field(latest.model, expected: "gpt-6-luna")
                            switch latest.effort {
                            case .missing: Text("check.absent", bundle: .module)
                            case .low: Text(verbatim: "low")
                            case .high: Text(verbatim: "high")
                            case .other: Text("check.other", bundle: .module)
                            }
                            switch latest.tier {
                            case .missing: Text("check.absent", bundle: .module)
                            case .priority: Text(verbatim: "priority")
                            case .fast: Text(verbatim: "fast")
                            case .default: Text(verbatim: "default")
                            case .other: Text("check.other", bundle: .module)
                            }
                        }
                        if let usage = latest.usage {
                            HStack {
                                Text("check.total", bundle: .module)
                                Text(usage.total, format: .number)
                            }
                        } else {
                            Text("check.usageUnknown", bundle: .module)
                        }
                    } else {
                        Text("check.noHistory", bundle: .module)
                    }
                } header: {
                    Text("check.last", bundle: .module)
                }
            }
            Section {
                Text("check.historyHelp", bundle: .module).foregroundStyle(.secondary)
            }
        }
    }
    private func field(_ value: ModelCheckField, expected: String) -> Text {
        switch value {
        case .missing: Text("check.absent", bundle: .module)
        case .expected: Text(verbatim: expected)
        case .other: Text("check.other", bundle: .module)
        }
    }
}
