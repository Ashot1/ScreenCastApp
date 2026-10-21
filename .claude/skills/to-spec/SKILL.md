---
name: to-spec
description: "Turn the current conversation into a spec in the feature folder: no interview, just synthesis of what you've already discussed, then a check of the finished spec before it is split into tickets."
disable-model-invocation: true
---

<!--
GENERATED AGENT FILE. DO NOT EDIT.
Source: .agents/skills/to-spec/SKILL.md
Edit the source and run yarn agents:generate.
-->

This skill takes the current conversation context and codebase understanding and produces a spec. Do NOT interview the user; just synthesize what you already know.

The spec lives in `docs/features/<feature-slug>/spec.md`, next to the feature's `decisions.md`. The folder is tracked in git.

## Process

1. Explore the repo to understand the current state of the codebase, if you haven't already. Use the vocabulary and respect the decisions recorded in `docs/features/<feature-slug>/decisions.md`.

2. Sketch out the seams at which you're going to test the feature. Existing seams should be preferred to new ones. Use the highest seam possible. If new seams are needed, propose them at the highest point you can. The fewer seams across the codebase, the better - the ideal number is one.

Check with the user that these seams match their expectations.

3. Write the spec using the template below to `docs/features/<feature-slug>/spec.md`.

4. Check the finished spec before it is split into tickets. Call these two in parallel:

   - the `plan-simplicity-reviewer`, to check whether the plan is the simplest sufficient one among the available options;
   - the Evidence Auditor, to check that the APIs, data and components the plan relies on exist and can be used as planned.

   Verify their key findings against the places they cite. Fix the spec where a finding stays inside the agreed decisions. A finding that would change an agreed decision or a product requirement goes to the user: describe it and wait for the answer. Only then continue with `to-tickets`.

<spec-template>

## Problem Statement

The problem that the user is facing, from the user's perspective.

## Solution

The solution to the problem, from the user's perspective.

## User Stories

A LONG, numbered list of user stories. Each user story should be in the format of:

1. As an <actor>, I want a <feature>, so that <benefit>

<user-story-example>
1. As a mobile bank customer, I want to see balance on my accounts, so that I can make better informed decisions about my spending
</user-story-example>

This list of user stories should be extremely extensive and cover all aspects of the feature.

## Implementation Decisions

A list of implementation decisions that were made. This can include:

- The modules that will be built/modified
- The interfaces of those modules that will be modified
- Technical clarifications from the developer
- Architectural decisions
- Schema changes
- API contracts
- Specific interactions

Do NOT include specific file paths or code snippets here. They may end up being outdated very quickly; the paths the plan relies on go to Reference Points.

Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it within the relevant decision and note briefly that it came from a prototype. Trim to the decision-rich parts, not a working demo, just the important bits.

## Testing Decisions

A list of testing decisions that were made. Include:

- A description of what makes a good test (only test external behavior, not implementation details)
- Which modules will be tested
- Prior art for the tests (i.e. similar types of tests in the codebase)

## Reference Points

The files, symbols and conditions the chosen approach relies on, with the code revision (commit) they were read at. Mark them as of planning: the implementer re-checks that they are still current instead of searching blind. This is the only section that lists file paths.

## Out of Scope

A description of the things that are out of scope for this spec.

## Further Notes

Any further notes about the feature.

</spec-template>
