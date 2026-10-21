import SwiftUI

/// The design's work-in-progress row. The caller owns the cancel action and its guard, so the row
/// never decides whether work can stop; the caller also gives the fill of its surface.
struct WorkRow: View {
    let title: Text
    let cancel: () -> Void
    let disabled: Bool
    var body: some View {
        HStack(spacing: 10) {
            ProgressView().controlSize(.small)
            title.font(.callout)
            Spacer(minLength: 8)
            Button(action: cancel) { Text("check.cancel", bundle: .module) }
                .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
                .disabled(disabled)
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }
}
