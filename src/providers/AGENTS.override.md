<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/rules/providers.md
Edit the source and run yarn agents:generate.
-->

# Model providers

- Provider endpoints/model IDs are owned here when implementations are added. Verify official contracts before selecting an API, version or price.
- Use one completion/error/usage contract from the core; never let provider SDK types become the domain model.
- All attempts, SDK retries, fallbacks and shadow requests pass budget and deadline admission. Disable hidden automatic paid retries.
- Treat provider payloads as untrusted input. Unknown usage cannot become zero cost.
- Jev vision/Box is experimental. A box is not proof that a target exists; require a presence check, versioned preprocessing and current native geometry before proposing an action.
- Local helper is independently supervised and optional. Cloud dispatch must not share its blocking lifetime.
- Keys stay in native secret stores and enter only the authorized transport. No credentials or captured content in fixtures/logs.
