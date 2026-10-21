---
name: harmony-bootstrap
description: Use when starting Ambient Assistant work in Cursor or before repository edits, delegation, or user-requested UI verification.
---

<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: agent-sources/skills/harmony-bootstrap/SKILL.md
Edit the source and run yarn agents:generate.
-->

# Harmony Bootstrap

Use this Skill before repository work. Universal rules are already supplied by
native entrypoints; do not reread bodies present in session context.

## Native Scope

- Universal and directory rules use root and nested `AGENTS.md`.
- Ignore all `AGENTS.override.md` files.
- `.cursor/rules/repository-workflow.mdc` is the native workflow adapter.
- Native agents live in `.cursor/agents/`; keep their configured models.

## Startup

Before edits, identify touched paths, read their nearest README files up to root,
and inspect existing code and the owning framework API. Repeat when scope changes.
Use root-to-path `AGENTS.md` rules. Read a matching scope not yet loaded before edits.

For library lookup hints, read [references/libraries.md](references/libraries.md)
when relevant. Native-first ordering and hard behavior rules come from universal
rules, rather than duplicated copies in this Skill.

## Required Specialists

Use the following triggers for planners and implementers, including a narrow
change with a known edit location. A named specialist's configured model comes
from its native definition, not manual-worker routing.

| Specialist | Mandatory trigger | Input / ownership |
| --- | --- | --- |
| `codebase-scout` | Any code change: planner before choosing an approach; implementer before dependent edits | Project code, ownership, existing helpers/components/types and affected tests; read only |
| `pattern-librarian` | The same code task, after Scout supplies places | Installed library/framework capabilities at those places; read only |
| `external-researcher` | A decision depends on facts outside the repository | Current primary sources and versions; read only; no messages to people |
| `localization-mechanic` | Any translation key/text addition, edit or deletion | Approved English copy and UI context; all supported locales; exclusive write scope |
| `endpoint-registry-mechanic` | Any change in provider endpoint or model registries | Verified provider URL/model/capability and configuration intent; exclusive registry scope |
| `type-scaffolder` | Any transport/domain/FFI type work: finding, deriving, adding, changing or deleting a declaration, including a local alias replaced by schema inference | Contract source, fields/optionality/nullability and type scope; never UI view state |
| `visual-inspector` | User-requested UI/API check with more than about five cases or a case/viewport matrix | Parent-supplied mechanical cases; read only |
| `plan-simplicity-reviewer` | A major interview/design fork; a finished specification | One independent alternative at the fork, or finished-spec review; read only |
| `evidence-auditor` | A finished specification, parallel with plan simplicity review, before tickets | Verify its data/APIs/components; planning only; read only |
| `code-simplicity-reviewer` | Task logic is ready, before reporting completion | Current task diff and directly affected code; one pass; read only |

The owner has authorized these calls. When a trigger applies, delegation is
mandatory. Known files, small size, doing it faster yourself or lower cost do not
justify skipping a specialist. The manual-worker cost gate does not apply here.

Only three exceptions permit skipping a triggered call:

- a result already exists and is applicable to the current code/state;
- the human user forbade delegation;
- the required tool is unavailable or rejects an attempted call.

State the concrete evidence for the exception. A missing trigger means the role
is not needed. If a tool is blocked, report the actual blocker and continue only
within your authorized role; do not bypass a user/system restriction.

Before dependent edits, match the proposed changes against this matrix using the
Scout's findings. Update the startup list when a new trigger becomes clear.
Check additions, changes and deletions; “no new types” does not exempt deleting
an existing payload/domain/FFI type. If a schema replaces that alias, call
`type-scaffolder` before the replacement. For a non-UI declaration
inside a UI file, the mechanic verifies the contract read only and recommends
reuse/inference/removal; the UI file owner applies the adopted result.

## Visible Preflight And Adoption

At the start, briefly state the required specialists and when they run. Before
each call, show: “→ Calling <role>: <assignment>. Expected: <result>.” After its
answer: “← <role> returned: … Adopted: … Rejected: … because …”. Use the user's
language and keep it concise. Report actual calls and skipped triggers at the end
so the user can tell which requirements were followed.

Give each specialist a bounded question/write scope and a non-overlapping parent
scope. Scout passes places to Pattern Librarian; do not repeat their whole search.
Do not paste Skill bodies into prompts. Pass only relevant constraints, adopted
findings, evidence, write boundaries and verification expectations.

Every specialist returns conclusions separately from assumptions, concrete
anchors (file/symbol/line/condition or primary source/version), what affects the
conclusion and what was not checked. Verify the key conclusions that determine
your decision against those anchors: data ownership, lifecycle, contracts,
errors/access and the consequences of simplification. Inspect a mechanic's diff
before adopting it or editing dependent code. Never edit its owned files in parallel.

Named specialists and routine mechanics complete their role themselves. They
must never create, delegate to or coordinate subagents.

## Simplicity Review

At a major plan fork, give `plan-simplicity-reviewer` requirements,
scenarios and verified evidence without the planner's proposed solution. It
builds one independent alternative; the planner then compares the two. For a
finished spec, this reviewer and `evidence-auditor` independently review
it before tickets. An ordinary small fix needs no planning interview/spec.

Call `code-simplicity-reviewer` once when the task's logic is ready.
Include the actual specialist calls and skipped triggers with their reasons in
its assignment. The reviewer checks changed translation keys, provider
registries and non-UI transport/domain/FFI type declarations against that
list, including removals. Resolve a missed mandatory mechanic before completion.
After corrections, repeat only for the corrected part. It proposes simplifications,
including directly adjacent old code, and does not apply them. The part's owner
adopts findings after verification. A finding that changes an agreed plan,
requirements, public contract, lifecycle, data owner or task boundaries goes to
the user before implementation; ordinary choices left open by the plan do not.

## Visual Inspection

Check visually only when the user requested it. Walk the key flows yourself.
For more than about five cases or a case/viewport matrix, hand every case to
`visual-inspector`. Supply goal, target state/setup, viewports, numbered
actions, expected results and evidence. It reports PASS/FAIL/BLOCKED per case and
viewport plus incidental findings. Surface those findings; an incomplete brief
is blocked, and the inspector does not invent cases or product decisions.

## Manual Workers And Ownership

Use `harmony-orchestration` only before creating a manual/generic implementation
worker for a concrete autonomy, parallelism or context-isolation benefit.
Each agent owns decisions within its role, integration, verification and the
answer for its part. Independent implementers have no lead implementer. Keep
shared contracts and dependencies explicit; different filenames alone do not
make packages independent. The parent owns diagnosis/evaluation and synthesis
of its own task, without duplicating specialist investigations.

## Verification

Follow universal finishing rules: `yarn verify` on macOS or `yarn verify:portable`
for common core/tooling on Windows. Use project Swift format rules, rustfmt and
Clippy; fix diagnostics without weakening rules. Report actual specialist calls,
checks and limits; never claim a skipped check passed. Commit only when asked.
