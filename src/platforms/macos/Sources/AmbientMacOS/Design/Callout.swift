import SwiftUI

/// The design's state block (error, warning, information, unsaved): a quiet tone fill with a
/// titled message and the block's own actions.
struct Callout<Actions: View>: View {
    let tone: Tone
    let systemImage: String
    let title: Text
    let message: Text
    @ViewBuilder let actions: Actions
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label {
                title.font(.headline)
            } icon: {
                Image(systemName: systemImage)
            }
            .foregroundStyle(tone.text)
            message
            actions.buttonStyle(.bordered).buttonBorderShape(.capsule)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tone.fill, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .accessibilityElement(children: .contain)
    }
}
