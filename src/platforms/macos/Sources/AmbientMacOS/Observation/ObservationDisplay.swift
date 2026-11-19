import Observation

/// A read-only UI projection; policy and accounting remain in Core.
@MainActor @Observable final class ObservationDisplay {
    private(set) var state: ObservationState = .stopped
    private(set) var failure: ModelCheckFailure?
    private(set) var journal: ObservationJournal?
    private(set) var permission: Bool?
    private(set) var loading = true
    private(set) var working = false
    nonisolated init() {}

    func update(
        state: ObservationState, failure: ModelCheckFailure?, working: Bool,
        journal: ObservationJournal? = nil
    ) {
        self.state = state
        self.failure = failure
        self.working = working
        if let journal { self.journal = journal }
        if journal != nil || failure != nil { loading = false }
    }
    func permissionChecked(_ granted: Bool) { permission = granted }
}

extension ObservationState {
    var key: String {
        switch self {
        case .stopped: "observation.state.stopped"
        case .preparing: "observation.state.preparing"
        case .needsPermission: "observation.state.needsPermission"
        case .observing: "observation.state.observing"
        case .paused: "observation.state.paused"
        case .waiting: "observation.state.waiting"
        case .failed: "observation.state.failed"
        }
    }
    var symbol: String {
        switch self {
        case .stopped: "stop.circle"
        case .preparing: "hourglass"
        case .needsPermission: "lock.circle"
        case .observing: "circle.inset.filled"
        case .paused: "pause.circle"
        case .waiting: "clock"
        case .failed: "exclamationmark.circle"
        }
    }
}
