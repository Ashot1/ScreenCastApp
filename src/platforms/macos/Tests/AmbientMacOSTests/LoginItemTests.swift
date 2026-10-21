import Foundation
import ServiceManagement
import Testing

@testable import AmbientMacOS

@MainActor private final class LoginItemDouble {
    var status: SMAppService.Status = .notRegistered
    var events: [String] = []
    var registrationResult: SMAppService.Status = .requiresApproval
    var removalResult: SMAppService.Status = .notRegistered
    var registerFails = false
    var unregisterFails = false

    func model() -> LoginItemModel {
        LoginItemModel(
            readStatus: {
                self.events.append("status")
                return self.status
            },
            register: {
                self.events.append("register")
                self.status = self.registrationResult
                if self.registerFails { throw CocoaError(.fileReadNoPermission) }
            },
            unregister: {
                self.events.append("unregister")
                self.status = self.removalResult
                if self.unregisterFails { throw CocoaError(.fileReadNoPermission) }
            },
            openSettings: { self.events.append("settings") })
    }
}

@Test @MainActor func loginItemRefreshOnlyReadsAndDetectsExternalRevocation() {
    let service = LoginItemDouble()
    service.status = .enabled
    let model = service.model()
    #expect(service.events.isEmpty)
    #expect(model.status == nil)
    model.refresh()
    #expect(model.status == .enabled)
    service.status = .requiresApproval
    model.refresh()
    #expect(model.status == .requiresApproval)
    #expect(model.statusKey == "loginItem.requiresApproval")
    #expect(service.events == ["status", "status"])
}

@Test @MainActor func loginItemEnableReadsActualApprovalInsteadOfAssumingEnabled() {
    let service = LoginItemDouble()
    let model = service.model()
    model.enable()
    #expect(service.events == ["register", "status"])
    #expect(model.status == .requiresApproval)
    #expect(model.failureKey == nil)
}

@Test @MainActor func loginItemDisableReadsActualStatusAfterSingleRemoval() {
    let service = LoginItemDouble()
    service.status = .enabled
    service.removalResult = .requiresApproval
    let model = service.model()
    model.disable()
    #expect(service.events == ["unregister", "status"])
    #expect(model.status == .requiresApproval)
}

@Test @MainActor func loginItemRegistrationErrorRereadsStatusAndExplicitRetryClearsError() {
    let service = LoginItemDouble()
    service.registerFails = true
    let model = service.model()
    model.enable()
    #expect(model.failureKey == "loginItem.enableFailed")
    #expect(model.status == .requiresApproval)
    #expect(service.events == ["register", "status"])
    service.registerFails = false
    service.registrationResult = .enabled
    model.enable()
    #expect(model.failureKey == nil)
    #expect(model.status == .enabled)
    #expect(service.events == ["register", "status", "register", "status"])
}

@Test @MainActor func loginItemRemovalErrorPreservesSystemTruthAndSettingsNeverRegisters() {
    let service = LoginItemDouble()
    service.unregisterFails = true
    service.removalResult = .enabled
    let model = service.model()
    model.disable()
    #expect(model.failureKey == "loginItem.disableFailed")
    #expect(model.status == .enabled)
    #expect(service.events == ["unregister", "status"])
    model.openSystemSettings()
    #expect(model.failureKey == nil)
    #expect(model.status == .enabled)
    #expect(service.events == ["unregister", "status", "settings"])
}

@Test @MainActor func loginItemUnknownAndMissingServiceRemainUnavailable() throws {
    let service = LoginItemDouble()
    let model = service.model()
    service.status = .notFound
    model.refresh()
    #expect(model.statusKey == "loginItem.unavailable")
    service.status = try #require(SMAppService.Status(rawValue: 9876))
    model.refresh()
    #expect(model.status == service.status)
    #expect(model.statusKey == "loginItem.unavailable")
    #expect(service.events == ["status", "status"])
}

@Test @MainActor func loginItemExplicitRefreshClearsErrorButAutomaticRefreshPreservesIt() {
    let service = LoginItemDouble()
    service.registerFails = true
    let model = service.model()
    model.enable()
    service.status = .notRegistered
    model.refresh()
    #expect(model.failureKey == "loginItem.enableFailed")
    #expect(model.status == .notRegistered)
    model.refreshByUser()
    #expect(model.failureKey == nil)
    #expect(model.status == .notRegistered)
    #expect(service.events == ["register", "status", "status", "status"])
}

@Test @MainActor func loginItemActionsLeavePersistentPauseUntouched() async throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let memory = MemorySession(databaseURL: url)
    try await memory.setManualPause(true)
    let service = LoginItemDouble()
    let model = service.model()
    model.refresh()
    model.enable()
    model.openSystemSettings()
    model.disable()
    try await memory.close()
    let reopened = MemorySession(databaseURL: url)
    #expect(try await reopened.manualPause())
    #expect(service.events == ["status", "register", "status", "settings", "unregister", "status"])
    try await reopened.close()
}
