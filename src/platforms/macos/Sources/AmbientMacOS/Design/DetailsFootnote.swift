import SwiftUI

/// The design's quiet footnote: a short visible line (what is read, sent or saved stays visible) and
/// the long explanation in a system «Подробнее» disclosure (D83).
struct DetailsFootnote<Details: View>: View {
    let text: Text
    @ViewBuilder let details: Details
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            text
            DisclosureGroup {
                VStack(alignment: .leading, spacing: 6) { details }
                    .frame(maxWidth: .infinity, alignment: .leading)
            } label: {
                Text("sources.details", bundle: .module)
            }
        }
        .font(.caption).foregroundStyle(.secondary)
    }
}
