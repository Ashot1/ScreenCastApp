import SwiftUI

/// The design's meaning label: a small capsule in a palette tone, or in quiet system colors when
/// the label carries no tone.
struct Pill: View {
    let text: Text
    var tone: Tone? = nil
    var body: some View {
        text.font(.caption.weight(.semibold))
            .foregroundStyle(tone.map { AnyShapeStyle($0.text) } ?? AnyShapeStyle(.secondary))
            .padding(.horizontal, 8).padding(.vertical, 3)
            .background(
                tone.map { AnyShapeStyle($0.fill) } ?? AnyShapeStyle(.quaternary), in: Capsule())
    }
}
