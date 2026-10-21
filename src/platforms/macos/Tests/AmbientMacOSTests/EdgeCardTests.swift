import AppKit
import Testing

@testable import AmbientMacOS

@Test @MainActor func edgeGeometryKeepsPointerScreenAndOnlyClampsExistingPosition() {
    let usable = CGRect(x: -1200, y: 40, width: 1200, height: 760)
    let first = EdgeCardController.placement(in: usable, existing: nil)
    #expect(usable.contains(first))
    #expect(first.maxX == -16)
    let moved = CGRect(x: -600, y: 100, width: 420, height: 500)
    #expect(EdgeCardController.placement(in: usable, existing: moved) == moved)
    let smaller = CGRect(x: 0, y: 40, width: 320, height: 400)
    let corrected = EdgeCardController.placement(in: smaller, existing: moved)
    #expect(smaller.contains(corrected))
    #expect(corrected.width == 320 && corrected.height == 400)
}
