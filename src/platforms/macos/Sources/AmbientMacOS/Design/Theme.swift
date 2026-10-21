import AppKit
import SwiftUI

/// Design colors of ADR-023 that system semantic colors do not cover. Light values come from the
/// design; dark values are system colors, so dark mode follows macOS (D74).
enum Theme {
    /// Accent of prominent buttons and switches: the design's ink in light mode, white in dark.
    static let ink = Color(light: rgb(0x15171C), dark: .white)
    static let ready = Tone(fill: 0xE8F4EC, text: 0x17663A, dark: .systemGreen)
    static let stale = Tone(fill: 0xFCEEDB, text: 0x8A470C, dark: .systemOrange)
    static let error = Tone(fill: 0xFDEDEB, text: 0xA1231A, dark: .systemRed)
    static let clarification = Tone(fill: 0xE6EEFB, text: 0x1F4F9E, dark: .systemBlue)
}

/// One meaning of the design palette: a quiet fill and its text color.
struct Tone {
    let fill: Color
    let text: Color

    init(fill: UInt32, text: UInt32, dark: NSColor) {
        self.fill = Color(light: rgb(fill), dark: dark.withAlphaComponent(0.18))
        self.text = Color(light: rgb(text), dark: dark)
    }
}

extension Color {
    /// AppKit resolves the provider for every appearance, including Increase Contrast variants.
    init(light: NSColor, dark: NSColor) {
        self.init(
            nsColor: NSColor(name: nil) { appearance in
                appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
            })
    }
}

private func rgb(_ hex: UInt32) -> NSColor {
    NSColor(
        srgbRed: CGFloat(hex >> 16 & 0xFF) / 255, green: CGFloat(hex >> 8 & 0xFF) / 255,
        blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
}
