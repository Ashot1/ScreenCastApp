import AmbientMacOS
import AppKit
import SwiftUI

@main struct AmbientApp: App {
    @NSApplicationDelegateAdaptor(AmbientAppDelegate.self) private var delegate

    var body: some Scene {
        Window("Ambient", id: "memory") {
            AssistantView(model: delegate.model, chatGPT: delegate.chatGPT)
                .task { await delegate.chatGPT.reload() }
                .ambientTheme()
        }
        .defaultSize(width: 940, height: 800)
        Settings {
            AssistantSettingsView(
                model: delegate.model, chatGPT: delegate.chatGPT, observation: delegate.observation
            )
            .ambientTheme()
        }
        MenuBarExtra {
            AssistantMenu(model: delegate.model, observation: delegate.observation)
        } label: {
            AssistantMenuBarLabel(model: delegate.model, observation: delegate.observation)
        }
        .menuBarExtraStyle(.menu)
    }
}

@MainActor final class AmbientAppDelegate: NSObject, NSApplicationDelegate {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    let observation: ObservationSession
    private var edgeCard: EdgeCardController?
    override init() {
        AmbientProcess.launch()
        #if AMBIENT_VISUAL_FIXTURE
            let root = visualFixtureRoot()
            let session = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
            let auth = ChatGPTModel(
                session: session, chatGPTKeychainService: "dev.ambient.foundation.visual.chatgpt",
                codexHome: root)
        #else
            let session = MemorySession()
            let auth = ChatGPTModel(session: session)
        #endif
        chatGPT = auth
        let assistant = AssistantModel(session: session)
        model = assistant
        observation = auth.makeObservationSession(model: assistant)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        edgeCard = EdgeCardController(model: model, chatGPT: chatGPT)
        edgeCard?.start()
        Task { @MainActor in await model.loadManualPause() }
        Task { @MainActor in await chatGPT.cleanupLocalAudioAfterLaunch() }
        #if AMBIENT_VISUAL_FIXTURE
            // Ordinary Visual remains journal-only even if its test policy was previously started.
            Task { await observation.cleanup() }
        #else
            Task { await observation.restore() }
        #endif
        #if DEBUG && AMBIENT_VISUAL_FIXTURE && AMBIENT_OBSERVATION_DEBUG
            startDebugObservation()
        #endif
    }
    #if DEBUG && AMBIENT_VISUAL_FIXTURE && AMBIENT_OBSERVATION_DEBUG
        private func startDebugObservation() {
            Task {
                do { try await observation.start(creditAcknowledged: true) } catch {
                    FileHandle.standardError.write(Data("Observation debug start failed\n".utf8))
                }
            }
        }
    #endif

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // Refuse input before the awaited auth shutdown.
        model.beginClosing()
        edgeCard?.stop()
        Task { @MainActor in
            await observation.shutdown()
            let authClosed = await chatGPT.shutdown()
            let memoryClosed = await model.close()
            sender.reply(toApplicationShouldTerminate: memoryClosed && authClosed)
        }
        return .terminateLater
    }
}

#if AMBIENT_VISUAL_FIXTURE
    /// «Ambient Visual» (ADR-024) runs only on a test folder under `target/verification/`. There is
    /// no fallback to the user's memory: a missing or foreign folder stops the launch.
    private func visualFixtureRoot() -> URL {
        let path = ProcessInfo.processInfo.environment["AMBIENT_VISUAL_ROOT"] ?? ""
        let root = URL(filePath: path, directoryHint: .isDirectory).standardizedFileURL
            .resolvingSymlinksInPath()
        guard path.hasPrefix("/"), root.path.contains("/target/verification/") else {
            FileHandle.standardError.write(
                Data("AMBIENT_VISUAL_ROOT must be a folder under target/verification/\n".utf8))
            exit(EXIT_FAILURE)
        }
        return root
    }
#endif
