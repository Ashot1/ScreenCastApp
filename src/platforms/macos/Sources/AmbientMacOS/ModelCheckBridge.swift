import CAmbient
import Dispatch
import Foundation

enum ModelCheckFailure: Error, Equatable, Sendable {
    case invalid, paused, cancelled, busy, storage, limit, closed, report, session, native,
        capability, stale, completed, result
    var key: String {
        switch self {
        case .result: "assistance.error.result"
        case .capability: "assistance.error.capability"
        case .stale: "assistance.error.stale"
        case .completed: "assistance.error.completed"
        case .paused: "check.error.paused"
        case .busy: "check.error.busy"
        case .storage: "check.error.storage"
        case .limit: "check.error.limit"
        case .closed: "check.error.closed"
        case .report: "check.error.report"
        case .session: "check.error.session"
        case .cancelled: "check.outcome.cancelled"
        case .invalid, .native: "check.error.invalid"
        }
    }
}
enum ModelCheckOutcome: UInt32, Sendable {
    case pending = 0
    case confirmed, mismatch, provider, invalid, cancelled, timeout, interrupted, local, quota
    var key: String {
        switch self {
        case .pending, .interrupted: "check.outcome.interrupted"
        case .confirmed: "check.outcome.confirmed"
        case .mismatch: "check.outcome.mismatch"
        case .provider: "check.outcome.provider"
        case .invalid: "check.outcome.invalid"
        case .cancelled: "check.outcome.cancelled"
        case .timeout: "check.outcome.timeout"
        case .local: "check.outcome.local"
        case .quota: "check.outcome.quota"
        }
    }
}
enum ModelCheckField: UInt32, Sendable {
    case missing = 0
    case expected, other
}
enum ModelCheckTier: UInt32, Sendable {
    case missing = 0
    case priority, fast, `default`, other
}
struct ModelCheckUsage: Sendable, Equatable {
    let input: UInt64
    let output: UInt64
    let total: UInt64
}
struct ModelCheckAttempt: Sendable, Equatable {
    let id: UInt64
    let atUTC: Int64
    let stage: UInt32
    let outcome: ModelCheckOutcome
    let model: ModelCheckField
    let effort: ModelEffort
    let tier: ModelCheckTier
    let usage: ModelCheckUsage?
    let purpose: ModelPurpose
    let profile: ModelProfile
    init(
        id: UInt64, atUTC: Int64, stage: UInt32, outcome: ModelCheckOutcome, model: ModelCheckField,
        effort: ModelEffort, tier: ModelCheckTier, usage: ModelCheckUsage?,
        purpose: ModelPurpose = .connectivity, profile: ModelProfile = .low
    ) {
        self.id = id
        self.atUTC = atUTC
        self.stage = stage
        self.outcome = outcome
        self.model = model
        self.effort = effort
        self.tier = tier
        self.usage = usage
        self.purpose = purpose
        self.profile = profile
    }
}
struct ModelCheckSummary: Sendable, Equatable {
    let attempts: UInt64
    let input: UInt64
    let output: UInt64
    let total: UInt64
    let unknown: UInt64
    let latest: ModelCheckAttempt?
}
protocol ModelCheckProtocol: Sendable {
    /// `root` holds the private folders of the local plugin servers this dispatch may start.
    func dispatch(_ handle: UInt64, access: String, keys: [PluginDispatchKey], root: URL)
        async throws -> ModelOperationResult
    func close(_ handle: UInt64) async throws
    func cancel(_ handle: UInt64)
}
/// The accepted Rust request blocks only this queue; credentials and memory remain responsive.
actor ModelCheckBridge: ModelCheckProtocol {
    private nonisolated let queue = DispatchSerialQueue(label: "dev.ambient.model.check")
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
    func dispatch(_ handle: UInt64, access: String, keys: [PluginDispatchKey], root: URL) throws
        -> ModelOperationResult
    {
        let bytes = Array(access.utf8)
        guard !bytes.isEmpty, bytes.count <= 16_384, keys.count <= 64 else {
            throw ModelCheckFailure.invalid
        }
        var result = AmbientModelCompletion()
        // Plugin keys are borrowed for this one call, like the access token.
        let texts = keys.flatMap { [Array($0.server.utf8), Array($0.key.utf8)] }
        try PluginBridge.withTexts(texts + [Array(root.path.utf8)]) { t in
            let raw = keys.indices.map {
                AmbientPluginKey(
                    plugin: keys[$0].plugin, server: t[2 * $0], generation: keys[$0].generation,
                    key: t[2 * $0 + 1])
            }
            try bytes.withUnsafeBufferPointer { token in
                try raw.withUnsafeBufferPointer { k in
                    try Self.check(
                        ambient_model_v6_dispatch(
                            handle, token.baseAddress, UInt32(token.count), k.baseAddress,
                            UInt32(k.count), t[2 * keys.count], &result))
                }
            }
        }
        return try Self.decodeCompletion(result)
    }
    nonisolated func cancel(_ handle: UInt64) {
        // Close reports disposal errors after real return; cancellation can race an already closed handle.
        _ = ambient_model_v6_cancel(handle)
    }
    func close(_ handle: UInt64) throws { try Self.check(ambient_model_v6_close(handle)) }
    nonisolated static func check(_ status: UInt32) throws {
        switch status {
        case 0: return
        case 1: throw ModelCheckFailure.invalid
        case 2: throw ModelCheckFailure.paused
        case 3: throw ModelCheckFailure.cancelled
        case 4: throw ModelCheckFailure.busy
        case 5: throw ModelCheckFailure.storage
        case 6: throw ModelCheckFailure.limit
        case 7: throw ModelCheckFailure.closed
        case 9: throw ModelCheckFailure.report
        case 10: throw ModelCheckFailure.capability
        case 11: throw ModelCheckFailure.stale
        case 12: throw ModelCheckFailure.session
        case 13: throw ModelCheckFailure.completed
        default: throw ModelCheckFailure.native
        }
    }
    nonisolated static func decode(_ r: AmbientModelAttempt) throws -> ModelCheckAttempt {
        guard r.id > 0, r.id <= UInt64(Int64.max), (1...2).contains(r.stage),
            r.expected_model == 1, let profile = ModelProfile(rawValue: r.expected_effort),
            r.expected_tier == 1,
            let purpose = ModelPurpose(rawValue: r.purpose),
            purpose != .assistance || profile == .high,
            let outcome = ModelCheckOutcome(rawValue: r.outcome),
            (r.stage == 1 && outcome == .pending) || (r.stage == 2 && outcome != .pending),
            let model = ModelCheckField(rawValue: r.actual_model),
            let effort = ModelEffort(rawValue: r.actual_effort),
            let tier = ModelCheckTier(rawValue: r.actual_tier), r.has_usage <= 1
        else { throw ModelCheckFailure.native }
        let usage: ModelCheckUsage?
        if r.has_usage == 1 {
            let sum = r.input_tokens.addingReportingOverflow(r.output_tokens)
            guard !sum.overflow, sum.partialValue == r.total_tokens,
                r.total_tokens <= UInt64(Int64.max), r.stage == 2
            else { throw ModelCheckFailure.native }
            usage = ModelCheckUsage(
                input: r.input_tokens, output: r.output_tokens, total: r.total_tokens)
        } else {
            guard r.input_tokens == 0, r.output_tokens == 0, r.total_tokens == 0 else {
                throw ModelCheckFailure.native
            }
            usage = nil
        }
        return ModelCheckAttempt(
            id: r.id, atUTC: r.at_utc, stage: r.stage, outcome: outcome, model: model,
            effort: effort, tier: tier, usage: usage, purpose: purpose, profile: profile)
    }
    nonisolated static func decode(_ r: AmbientModelSummary) throws -> ModelCheckSummary {
        let sum = r.input_tokens.addingReportingOverflow(r.output_tokens)
        // Folded totals make the attempt count unbounded; live rows stay bounded in Core.
        guard r.attempts <= UInt64(Int64.max), r.unknown <= r.attempts, !sum.overflow,
            sum.partialValue == r.total_tokens,
            r.has_latest <= 1, r.reserved == 0, (r.attempts == 0) == (r.has_latest == 0)
        else { throw ModelCheckFailure.native }
        let latest = r.has_latest == 1 ? try decode(r.latest) : nil
        return ModelCheckSummary(
            attempts: r.attempts, input: r.input_tokens, output: r.output_tokens,
            total: r.total_tokens, unknown: r.unknown, latest: latest)
    }
}
