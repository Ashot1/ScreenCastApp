import AppKit
import SwiftUI

struct VoiceReplyView: View {
    let chatGPT: ChatGPTModel
    private var voice: VoiceReplyModel { chatGPT.voice }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                if voice.phase == .idle {
                    Button {
                        chatGPT.startVoice(replacing: !chatGPT.replyDraft.isEmpty)
                    } label: {
                        Label {
                            Text(
                                chatGPT.replyDraft.isEmpty ? "voice.record" : "voice.replace",
                                bundle: .module)
                        } icon: {
                            Image(systemName: "mic")
                        }
                    }.disabled(!chatGPT.canStartVoice)
                } else {
                    if voice.phase == .recording || voice.phase == .limit {
                        Button {
                            voice.finish()
                        } label: {
                            Label {
                                Text("voice.finish", bundle: .module)
                            } icon: {
                                Image(systemName: "checkmark")
                            }
                        }
                    }
                    Button {
                        voice.cancel()
                    } label: {
                        Label {
                            Text("voice.cancel", bundle: .module)
                        } icon: {
                            Image(systemName: "xmark")
                        }
                    }.disabled(voice.phase == .stopping)
                }
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
            if voice.phase == .recording || voice.phase == .limit {
                recording
            } else if let key = phaseKey {
                Text(LocalizedStringKey(key), bundle: .module).font(.caption)
            }
            if voice.modelDirectory == nil {
                Text("voice.models.none", bundle: .module).font(.caption)
                Button {
                    voice.chooseModels()
                } label: {
                    Text("voice.models.choose", bundle: .module)
                }.disabled(voice.busy)
            }
            if !chatGPT.replyDraft.isEmpty && !voice.busy {
                Text("voice.replace.help", bundle: .module).font(.caption).foregroundStyle(
                    .secondary)
            }
            if voice.silence { Text("voice.silence", bundle: .module).font(.caption) }
            if let failure = voice.failure {
                Callout(
                    tone: Theme.error, systemImage: "mic.slash",
                    message: Text(LocalizedStringKey(failure.key), bundle: .module)
                ) {
                    if failure == .cleanup {
                        Button {
                            voice.retryCleanup()
                        } label: {
                            Text("voice.cleanup.retry", bundle: .module)
                        }
                        .disabled(voice.busy)
                    }
                }
            }
            Text("voice.local", bundle: .module).font(.caption).foregroundStyle(.secondary)
        }
    }
    /// Recording state in the voice color (the error token, ADR-023 #3), with time toward the limit.
    private var recording: some View {
        HStack(spacing: 8) {
            Circle().fill(Theme.error.text).frame(width: 8, height: 8).accessibilityHidden(true)
            // The limit phase can report slightly more than the limit; the bar stays full.
            ProgressView(
                value: min(voice.elapsed, VoiceReplyModel.recordingLimit),
                total: VoiceReplyModel.recordingLimit
            ) {
                if let key = phaseKey { Text(LocalizedStringKey(key), bundle: .module) }
            } currentValueLabel: {
                Text(Duration.seconds(voice.elapsed).formatted(.time(pattern: .minuteSecond)))
                    .monospacedDigit()
            }
            .tint(Theme.error.text)
        }.font(.caption)
    }
    private var phaseKey: String? {
        switch voice.phase {
        case .idle: nil
        case .permission: "voice.permission"
        case .recording: "voice.recording"
        case .limit: "voice.limit"
        case .transcribing: "voice.working"
        case .stopping: "voice.stopping"
        }
    }
}

struct VoiceSettingsView: View {
    let voice: VoiceReplyModel
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("voice.privacy", bundle: .module)
                if let directory = voice.modelDirectory {
                    Text(verbatim: directory.path).font(.caption).textSelection(.enabled)
                    Text("voice.models.selected", bundle: .module).font(.caption)
                } else {
                    Text("voice.models.none", bundle: .module)
                }
                HStack {
                    Button {
                        voice.chooseModels()
                    } label: {
                        Text("voice.models.choose", bundle: .module)
                    }
                    if voice.modelDirectory != nil {
                        Button {
                            voice.selectModels(nil)
                        } label: {
                            Text("voice.models.clear", bundle: .module)
                        }
                    }
                }.disabled(voice.busy)
            }.frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            Text("voice.settings.title", bundle: .module)
        }
    }
}

extension VoiceReplyModel {
    func chooseModels() {
        guard !busy else { return }
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.resolvesAliases = false
        if panel.runModal() == .OK, let url = panel.url { selectModels(url) }
    }
}
