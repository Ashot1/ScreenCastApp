import SwiftUI

/// The design's status mark inside windows (D75: the menu bar uses an SF Symbol). Color is never
/// the only signal: each state draws a symbol or arc, and callers place the status text next to
/// it, so the ring itself is hidden from accessibility.
struct StatusRing: View {
    enum Kind { case assistance, paused, unknown, closing }
    let kind: Kind
    var body: some View {
        ZStack {
            switch kind {
            case .assistance:
                track
                arc(1.0 / 3, .orange)
            case .paused:
                track
                Image(systemName: "pause.fill").font(.system(size: 7, weight: .bold))
            case .unknown:
                Circle().stroke(.secondary, style: StrokeStyle(lineWidth: 2, dash: [2.5, 2.5]))
                Image(systemName: "questionmark").font(.system(size: 7, weight: .bold))
            case .closing:
                track
                arc(0.25, .primary)
            }
        }
        .frame(width: 14, height: 14)
        .padding(3)
        .accessibilityHidden(true)
    }
    private var track: some View { Circle().stroke(.quaternary, lineWidth: 2) }
    private func arc(_ length: Double, _ color: some ShapeStyle) -> some View {
        Circle().trim(from: 0, to: length)
            .stroke(color, style: StrokeStyle(lineWidth: 2, lineCap: .round))
            .rotationEffect(.degrees(-90))
    }
}
