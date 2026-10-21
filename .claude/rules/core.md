---
paths:
  - src/core/**
---

<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/rules/core.md; agent-sources/manifest.json
Edit the source and run yarn agents:generate.
-->

# Shared Rust core

- This is the single owner of domain contracts, task memory, scheduling, routing, budget accounting and action validation for both platforms.
- Do not import OS UI/capture libraries, Swift/C# APIs or native window handles. Normalize platform data at the boundary.
- Keep `unsafe_code = "forbid"` in this crate. C ABI implementation belongs in the future bridge crate.
- Use Rust's native types and errors; no panics/unwraps/expect on reachable external input paths.
- Inject time and external effects at the boundary when implementing replayable logic. Persist wall time for history and use monotonic time for in-session deadlines.
- Verify `cargo check --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`.
- Every core feature has the same behavioral corpus on macOS and Windows. A Windows CI build is required before claiming platform portability is verified.
