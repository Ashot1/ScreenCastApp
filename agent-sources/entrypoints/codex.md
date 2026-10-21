# AGENTS.override.md

Codex entrypoint for Ambient Assistant.

This file is intentionally separate from `AGENTS.md` because Cursor also reads
`AGENTS.md`. Do not move Codex workflow or model routing back into `AGENTS.md`.

Codex gives `AGENTS.override.md` precedence over `AGENTS.md` in the same
directory, so the generator embeds the complete generated `AGENTS.md` below.
The universal rules have one source; do not duplicate them in this adapter.

## Bootstrap

Before repository work:

1. Follow the universal rules embedded below; do not reread root `AGENTS.md`.
2. Invoke/read the `harmony-bootstrap` Skill from Codex Skills.
3. Read every matching scoped `AGENTS.override.md` not already loaded, including parent scopes.
4. Before creating a manual/generic implementation worker, invoke/read the
   `harmony-orchestration` Skill.

Custom Codex subagents live in `.codex/agents/`.

## Delegation Authorization

The repository owner has authorized Codex to spawn service agents and writable
routine mechanics. When a trigger in `harmony-bootstrap` applies, the call is
mandatory, not a cost decision; the only exceptions are the ones in
`AGENTS.md`. Generic implementation workers follow `harmony-orchestration`.

If a required delegation is blocked because the tool is unavailable or rejects
the spawn, report that concrete tool or policy blocker and continue within your
role.

Do not paste Skill bodies wholesale into subagent prompts. Pass only the relevant
constraints, evidence, write scope, adopted findings, and verification
expectations.
