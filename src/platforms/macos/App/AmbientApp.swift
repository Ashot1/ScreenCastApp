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
        let session = MemorySession()
        model = AssistantModel(session: session)
        chatGPT = ChatGPTModel(session: session)
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
