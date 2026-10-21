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
            AssistantSettingsView(model: delegate.model, chatGPT: delegate.chatGPT)
                .ambientTheme()
        }
        MenuBarExtra {
            AssistantMenu(model: delegate.model)
        } label: {
            AssistantMenuBarLabel(model: delegate.model)
        }
        .menuBarExtraStyle(.menu)
    }
}

@MainActor final class AmbientAppDelegate: NSObject, NSApplicationDelegate {
    let model: AssistantModel
    let chatGPT: ChatGPTModel
    private var edgeCard: EdgeCardController?
    override init() {
        AmbientProcess.launch()
        #if AMBIENT_VISUAL_FIXTURE
            let root = visualFixtureRoot()
            let session = MemorySession(databaseURL: root.appending(path: "memory.sqlite3"))
            chatGPT = ChatGPTModel(
                session: session, chatGPTKeychainService: "dev.ambient.foundation.visual.chatgpt",
                codexHome: root)
        #else
            let session = MemorySession()
            chatGPT = ChatGPTModel(session: session)
        #endif
        model = AssistantModel(session: session)
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        edgeCard = EdgeCardController(model: model, chatGPT: chatGPT)
        edgeCard?.start()
        Task { @MainActor in await model.loadManualPause() }
        Task { @MainActor in await chatGPT.cleanupLocalAudioAfterLaunch() }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // Refuse input before the awaited auth shutdown.
        model.beginClosing()
        edgeCard?.stop()
        Task { @MainActor in
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
