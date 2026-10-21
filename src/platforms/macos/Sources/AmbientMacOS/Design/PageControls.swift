import SwiftUI

/// The design's paging row. A missing action hides its button, and the row disappears when there
/// is nowhere to page (ADR-023 #6).
struct PageControls: View {
    var first: (() -> Void)? = nil
    var next: (() -> Void)? = nil
    var body: some View {
        if first != nil || next != nil {
            HStack {
                if let first {
                    Button(action: first) { Text("tasks.first", bundle: .module) }
                }
                if let next {
                    Button(action: next) { Text("tasks.next", bundle: .module) }
                }
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
        }
    }
}
