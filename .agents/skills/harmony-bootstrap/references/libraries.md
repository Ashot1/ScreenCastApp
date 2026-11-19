<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/skills/harmony-bootstrap/references/libraries.md
Edit the source and run yarn agents:generate.
-->

# Library and platform lookup

Inspect installed versions and native APIs first. This is local lookup knowledge, not a dependency mandate.

- Core: Rust standard library, Cargo.lock and existing crates. The memory implementation already uses rusqlite and sha2; inspect the owning module and locked versions for the current dependencies.
- Providers: existing openidconnect, reqwest and Tokio authentication transport; inspect Cargo.lock and the module README before adding another protocol or HTTP mechanism.
- macOS: Swift standard library, Foundation, Swift Concurrency, SwiftUI/AppKit, ScreenCaptureKit, Accessibility, Keychain.
- Windows: .NET base libraries, Windows SDK, Windows Graphics Capture, UI Automation, Credential Manager; shell project is not created yet.
- Bridge: Rust C ABI, Swift C imports, .NET LibraryImport. Do not expose managed objects across it.
- Tooling only: Node built-ins, yaml 2.9.1, smol-toml 1.9.0, Vitest 4.0.18.
- Formatting: bundled `swift format` with `.swift-format`, rustfmt with `rustfmt.toml`; semantic concurrency checks in Swift compiler and Rust Clippy.
- Local inference: separate bundled helper, not a new in-process dependency. Select build/model only after validation.

Primary technical sources and their limits are in `docs/technical-references.md`.
