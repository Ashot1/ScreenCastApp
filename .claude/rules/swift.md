---
paths:
  - src/platforms/macos/**
---

<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/rules/swift.md; agent-sources/manifest.json
Edit the source and run yarn agents:generate.
-->

# macOS / Swift

- Use Swift 6 language mode, native frameworks and the project `.swift-format`; no JavaScript/TypeScript lint rules apply.
- Run `yarn check:swift` and `yarn lint:swift`. Format only assigned source files with `swift format format --in-place --configuration .swift-format <files>`.
- Strict format rules reject force unwrap, force try and implicitly unwrapped optionals. Do not suppress diagnostics to avoid fixing ownership.
- UI mutation stays on `@MainActor`; capture callbacks must not perform inference, synchronous network calls or image encoding on the UI thread.
- Actors own mutable asynchronous state. Do not add `@unchecked Sendable`, `nonisolated(unsafe)` or detached tasks without a documented boundary reason and targeted validation.
- ScreenCaptureKit and accessibility permissions are separate. Permission revocation stops the affected stream and invalidates pending actions.
- Release system frame surfaces promptly after required GPU use. Native code owns display scale, orientation, coordinate conversion and current target identity.
- Credentials use Keychain. UI strings use native localizable resources when UI is introduced.
- The shell does not own a second memory store, router or cost ledger.
