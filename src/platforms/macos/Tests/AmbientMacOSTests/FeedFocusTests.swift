import Foundation
import Testing

@testable import AmbientMacOS

private func row(_ id: UInt64, stale: Bool = false, revision: UInt64 = 1) throws -> Proposal {
    let json = """
        {"id":\(id),"task":{"task_id":7,"revision":1},"workspace_revision":1,
        "content_revision":\(revision),"kind":"clarification","message":"m","rationale":"r",
        "citations":[],"step_id":null,"visible":true,"stale":\(stale)}
        """
    return try JSONDecoder().decode(Proposal.self, from: Data(json.utf8))
}

/// ADR-023 #20: the reply target is expanded, otherwise the clicked row, otherwise the first
/// row that is not stale.
@Test func feedExpandsReplyTargetThenClickThenFirstFreshRow() throws {
    let rows = [try row(1, stale: true), try row(2), try row(3)]
    #expect(rows.expandedID(reply: nil, clicked: nil) == 2)
    #expect(rows.expandedID(reply: nil, clicked: 3) == 3)
    #expect(rows.expandedID(reply: nil, clicked: 1) == 1)
    #expect(rows.expandedID(reply: rows[2].target, clicked: 2) == 3)
    // A click on a row that left the list and a reply target from an older revision fall through.
    #expect(rows.expandedID(reply: nil, clicked: 9) == 2)
    #expect(rows.expandedID(reply: try row(3, revision: 2).target, clicked: nil) == 2)
    #expect([try row(1, stale: true)].expandedID(reply: nil, clicked: nil) == nil)
    #expect([Proposal]().expandedID(reply: nil, clicked: nil) == nil)
}
