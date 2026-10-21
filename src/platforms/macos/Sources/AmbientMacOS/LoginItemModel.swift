import Observation
import ServiceManagement

@MainActor @Observable final class LoginItemModel {
    private(set) var status: SMAppService.Status?
    private(set) var failureKey: String?
    private let readStatus: () -> SMAppService.Status
    private let register: () throws -> Void
    private let unregister: () throws -> Void
    private let openSettings: () -> Void

    init(
        readStatus: @escaping () -> SMAppService.Status = { SMAppService.mainApp.status },
        register: @escaping () throws -> Void = { try SMAppService.mainApp.register() },
        unregister: @escaping () throws -> Void = { try SMAppService.mainApp.unregister() },
        openSettings: @escaping () -> Void = { SMAppService.openSystemSettingsLoginItems() }
    ) {
        self.readStatus = readStatus
        self.register = register
        self.unregister = unregister
        self.openSettings = openSettings
    }

    var statusKey: String {
        guard let status else { return "loginItem.unchecked" }
        switch status {
        case .notRegistered: return "loginItem.notRegistered"
        case .enabled: return "loginItem.enabled"
        case .requiresApproval: return "loginItem.requiresApproval"
        case .notFound: return "loginItem.unavailable"
        @unknown default: return "loginItem.unavailable"
        }
    }

    func refresh() {
        status = readStatus()
    }

    func refreshByUser() {
        failureKey = nil
        refresh()
    }

    func enable() {
        failureKey = nil
        do {
            try register()
        } catch {
            // OS errors stay local; raw errors can contain paths and are not UI copy.
            failureKey = "loginItem.enableFailed"
        }
        refresh()
    }

    func disable() {
        failureKey = nil
        do {
            try unregister()
        } catch {
            failureKey = "loginItem.disableFailed"
        }
        refresh()
    }

    func openSystemSettings() {
        failureKey = nil
        openSettings()
    }
}
