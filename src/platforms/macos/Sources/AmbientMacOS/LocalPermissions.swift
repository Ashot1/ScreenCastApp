import ApplicationServices
import CLocalPermissions
import CoreGraphics
import SwiftUI

@MainActor struct LocalPermissions {
    private(set) var screenCapture: Bool?
    private(set) var accessibility: Bool?

    mutating func check(
        screenCapture: () -> Bool = { CGPreflightScreenCaptureAccess() },
        accessibility: () -> Bool = {
            // This preflight must never request access or read another app's content.
            AXIsProcessTrustedWithOptions(
                [ambient_accessibility_prompt_key(): false] as CFDictionary)
        }
    ) {
        self.screenCapture = screenCapture()
        self.accessibility = accessibility()
    }
}

struct LocalPermissionsView: View {
    @State private var permissions = LocalPermissions()
    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text("permissions.help", bundle: .module)
                LabeledContent {
                    result(permissions.screenCapture)
                } label: {
                    Text("permissions.screenCapture", bundle: .module)
                }
                LabeledContent {
                    result(permissions.accessibility)
                } label: {
                    Text("permissions.accessibility", bundle: .module)
                }
                Button {
                    permissions.check()
                } label: {
                    Text("permissions.check", bundle: .module)
                }
            }
        } label: {
            Text("permissions.title", bundle: .module)
        }
    }
    private func result(_ value: Bool?) -> Text {
        switch value {
        case nil: Text("permissions.unchecked", bundle: .module)
        case true: Text("permissions.granted", bundle: .module)
        case false: Text("permissions.notGranted", bundle: .module)
        }
    }
}
