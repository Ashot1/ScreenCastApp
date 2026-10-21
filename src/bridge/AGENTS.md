<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/rules/bridge.md
Edit the source and run yarn agents:generate.
-->

# Native bridge

- One versioned narrow C ABI connects the Rust owner with both native shells. The foundation declares no public ABI yet.
- Keep fixed-width values, explicit byte lengths, error codes and documented ownership. Never expose Rust layout, Swift objects or C# managed references.
- Every allocation has an owner and exactly one release path. Contain panics/errors before the foreign boundary.
- Specify call thread, callback delivery, cancellation, shutdown and no-callback-after-disposal semantics before writing bindings.
- Validate input size/schema at ingress. Action approval and semantic freshness remain core policy; native adapters revalidate live OS facts immediately before execution.
- Run the same round-trip, error, cancellation and disposal integration cases through Swift and C# before claiming bridge parity.
