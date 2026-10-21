# CLAUDE.md

@AGENTS.md

All universal engineering, TDD, product-documentation, mock, and human-communication
rules come from `AGENTS.md`. Follow them without weakening or duplicating them.

Claude Code must use the routed rules in `.claude/rules/*.md`.

Claude Code must invoke/read the `harmony-bootstrap` Skill during repository
startup. Before creating a manual/generic implementation worker, invoke/read
the `harmony-orchestration` Skill. Apply the roles and delegation rules from
`AGENTS.md`; named specialized agents and routine mechanics never create
subagents.

If automatic path-based rule loading is unavailable, manually read the rule files whose `paths` frontmatter matches the files you will edit.

Claude Code must ignore all `AGENTS.override.md` files. The root override is a
Codex entrypoint and scoped overrides are Codex adapters.
