---
name: grill-with-docs
description: A relentless interview to sharpen a plan or design, which also chooses the simplest sufficient implementation approach and records the agreed decisions in the feature's decisions file as we go.
disable-model-invocation: true
---

<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: .agents/skills/grill-with-docs/SKILL.md
Edit the source and run yarn agents:generate.
-->

Call the Skill tool for "grilling" and run its interview with the additions below. They are extra branches of the same design tree, not a second interview.

## Feature folder

Work in `docs/features/<feature-slug>/`; agree the slug with the user if it is not obvious. The folder is tracked in git.

Keep `docs/features/<feature-slug>/decisions.md` as the running record of the interview:

- Record each decision as soon as the user settles it, with its reason. Number decisions (`D01`, `D02`, …) and keep the numbers stable.
- When the user corrects or replaces a decision, record the correction next to it; never rewrite history silently.
- Keep the questions you asked with the answers you got, and keep open questions in their own list.
- Keep settled decisions apart from your proposals and from verified facts.

## Facts before the approach

Once the behaviour and the user scenarios are settled, and before choosing how to build it, call the Codebase Scout and the Pattern Librarian:

- the Codebase Scout finds the related project code: existing helpers, hooks, types, components, practices and tests, including ones nobody has named yet;
- the Pattern Librarian checks what the installed libraries already provide for this behaviour, starting from the places the Scout found.

Verify the conclusions that drive the decision against the places they cite.

## Choose the approach

Then put the implementation approach to the user as its own branch of the design tree. Show:

1. the behaviour and the scenarios that must not break;
2. where the data lives and which ready-made components, hooks or libraries fit, with links to the code;
3. the simplest sufficient approach, and which mechanisms it makes unnecessary;
4. if the simplest approach does not fit, the concrete requirement that rules it out;
5. what must not change, and the rejected options with their reasons.

For an obvious change two or three sentences are enough. Do not move on to the spec until the user agrees with the approach.

By default the simpler option wins if it keeps the behaviour, the scenarios and the contracts. A more complex option wins only when you name a concrete requirement that the simple one makes worse. "For the future" or "more robust" without a scenario does not count.

If a small change to the product requirements would make the solution much simpler, propose it and ask the user. Never change the requirements on your own.

## Big forks

At a big fork — a change of the data owner or of its lifecycle, a new shared module, or a custom mechanism instead of a ready one — call the `plan-simplicity-reviewer` for one independent alternative. Give it the requirements, the scenarios, the verified facts and the source code, but not your own candidate. Compare its alternative with yours and bring the comparison to the user.

When the design tree is done, continue with `to-spec`.
