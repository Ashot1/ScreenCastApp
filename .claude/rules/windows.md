---
paths:
  - src/platforms/windows/**
---

<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/rules/windows.md; agent-sources/manifest.json
Edit the source and run yarn agents:generate.
-->

# Windows / C#

- This folder owns the future native C# shell, Windows Graphics Capture, UI Automation, permissions, credentials and OS coordinate transforms.
- Share the exact Rust core; never port its logic into C#.
- Use nullable reference types and .NET analyzers when creating the first project. Establish a supported .NET/Windows SDK baseline in an ADR and CI before implementation.
- Keep capture and provider work off the UI thread. Check capture support, frame lifetime, window identity, resize and device loss.
- Use native credential storage; do not copy macOS permission assumptions.
- Use `LibraryImport` against a reviewed C ABI. Native handle/buffer lifetime is explicit; do not marshal managed objects or allow callbacks after disposal.
- Do not claim a Windows shell build or test before a Windows project and runner actually exist. Rust-on-Windows CI proves the core only.
