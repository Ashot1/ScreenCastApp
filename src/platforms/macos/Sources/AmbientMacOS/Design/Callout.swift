import SwiftUI

/// The design's state block (error, warning, information, unsaved): a quiet tone fill with a
/// message, an optional title and the block's own actions.
struct Callout<Actions: View>: View {
    let tone: Tone
    let systemImage: String
    var title: Text? = nil
    let message: Text
    @ViewBuilder let actions: Actions
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let title {
                Label {
                    title.font(.headline)
                } icon: {
                    Image(systemName: systemImage)
                }
                .foregroundStyle(tone.text)
                message
            } else {
                Label {
                    message
                } icon: {
                    Image(systemName: systemImage).foregroundStyle(tone.text)
                }
            }
            actions.buttonStyle(.bordered).buttonBorderShape(.capsule)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tone.fill, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .accessibilityElement(children: .contain)
    }
}

extension Callout where Actions == EmptyView {
    init(tone: Tone, systemImage: String, title: Text? = nil, message: Text) {
        self.init(tone: tone, systemImage: systemImage, title: title, message: message) {
            EmptyView()
        }
    }
}
