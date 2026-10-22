import SwiftUI

/// The design's inset block: a quiet nested fill instead of the default group box chrome.
struct AmbientInsetGroupBoxStyle: GroupBoxStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            configuration.label.font(.headline)
            configuration.content
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.fill.quaternary, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
}

extension View {
    /// Applies the design layer on a real root (the memory window, Settings, the edge panel);
    /// separate hosting roots do not inherit environment styles. No root tint: glass buttons stay
    /// untinted and system controls keep the system accent (ADR-024).
    public func ambientTheme() -> some View {
        groupBoxStyle(AmbientInsetGroupBoxStyle())
    }

    /// The design's primary action: a black capsule with its own tint.
    func ambientPrimary() -> some View {
        buttonStyle(.borderedProminent).buttonBorderShape(.capsule).tint(Theme.ink)
    }
}
