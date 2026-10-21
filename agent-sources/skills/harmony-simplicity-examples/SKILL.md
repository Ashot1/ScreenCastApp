---
name: harmony-simplicity-examples
description: Use when deciding on state, cancellation, validation, fallback or abstractions in Ambient Assistant.
---

# Native Simplicity Examples

These examples replace frontend-local React examples; the common simplicity workflow stays unchanged.

| Too complex | Sufficient | Keep complexity when |
| --- | --- | --- |
| Copy goal/memory into each Swift/C# screen and sync on timers | One Rust owner, native UI displays snapshots | UI has a real independent unsaved draft |
| Put capture, model inference and UI on one actor/queue | Separate native capture, core dispatch and helper lifetime | Ownership still needs an explicit serialized state transition |
| Cancel a request and immediately mark local worker free | Keep it occupied until completion/termination | Cloud fallback must run independently without awaiting local cancellation |
| Validate every owned value repeatedly | Validate external provider/FFI inputs at ingress | Permission, target identity or geometry can change before an action |
| Call local, Jev and a reasoning LLM for every event | Route directly to the capability needed | Measured multi-stage work preserves quality and saves full-episode cost |
| Create a crate/protocol for every future module | Add a module when it gains implemented responsibility | A separate process or platform is a real boundary now |
| Shared core imports AppKit and conditionally ignores it on Windows | Core contracts contain normalized data; native adapter owns OS handles | Actual platform behavior differs, and is represented explicitly |

Do not remove a mechanism merely to reduce line count. Name the preserved scenario and the responsibility that disappears.
