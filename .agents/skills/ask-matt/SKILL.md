---
name: ask-matt
description: Ask which skill or flow fits your situation. A router over the skills in this repo.
disable-model-invocation: true
---

# Ask Matt

You don't remember every skill, so ask.

A **flow** is a path through the skills. Most work runs along one **main flow**; a small, clear change can skip planning; everything else is standalone.

## The main flow: idea → ship

The route most work travels. You have an idea and want it built.

1. **`/grill-with-docs`** sharpens the idea by interview. It runs the `/grilling` primitive, calls the Codebase Scout and the Pattern Librarian before the approach is chosen, puts the simplest sufficient approach to you, and records the agreed decisions in `docs/features/<feature-slug>/decisions.md`. (No working directory? Use `/grill-me` instead, covered under Standalone.)
2. **`/to-spec`** turns the thread into `docs/features/<feature-slug>/spec.md`, then has the `plan-simplicity-reviewer` and the Evidence Auditor check the finished spec in parallel before anything is split.
3. **`/to-tickets`** splits the spec into tracer-bullet tickets under `docs/features/<feature-slug>/issues/`, each declaring its **blocking edges**, and walks you through the breakdown before writing the files.
4. **Implementation** runs in a separate implementer session, one ticket at a time; you assign the role. The implementer builds with the Superpowers `test-driven-development` skill, uses `systematic-debugging` for bugs and `verification-before-completion` before claiming the work is done, and gets the code simplicity review once the ticket's logic is ready. There is no automatic general code review, and nothing is committed until you ask.

Work the tickets along the **frontier**: any ticket whose blockers are all done can start. Independent tickets can go to separate implementer sessions; each implementer owns only its own ticket.

### Small, clear changes

A small bug, a copy change or a local fix doesn't need the planning flow. Give it straight to an implementer: it still looks for ready-made tools and the simplest approach, says so in two or three sentences, calls the required specialists and gets the code simplicity review.

### Context hygiene

Keep steps 1–3 in **one unbroken context window** (don't compact or clear until after `/to-tickets`) so the grilling, the spec and the tickets all build on the same thinking. Each implementer session then starts fresh, working from its ticket, the spec and the decisions file.

The limit on this is the **[smart zone](https://www.aihero.dev/ai-coding-dictionary/smart-zone)**: the window (~150k tokens on state-of-the-art models) within which the model still reasons sharply. If a session approaches it before `/to-tickets`, don't push on degraded; `/compact` at the nearest phase boundary and carry on (see Phase boundaries).

## Phase boundaries

A **phase** is a chunk of work inside a session: the grilling, the spec, the tickets. At the **boundary** between two of them you have five options, and picking between them is the fuzziest decision in this whole map:

- **Continue**: stay put. Costs nothing, loses nothing.
- **`/clear`**: empty the window, when nothing here matters to what's next.
- **`/handoff`** writes a portable markdown file. Narrow: only for a **new harness**, a **new directory**, a **colleague**, or forking a side task **mid-phase**. What it buys is portability.
- **Subagent**: send a tightly-scoped task to its own window and get a report back.
- **`/compact`** compresses this context and seeds a fresh session with it. The **default**, at the bottom of the tree rather than the first reach.

Read [PHASE-BOUNDARIES.md](PHASE-BOUNDARIES.md) for the ordered tree: the five questions, the reasoning behind each branch, and why the primary-source cost makes **Continue** the one to rule out first. Make the decision **at** a boundary; mid-phase, continue or split the rest into subagents.

## Standalone

Off the main flow entirely.

- **`/grill-me`**: the same relentless interview as `/grill-with-docs`, but **stateless**: it saves nothing locally and writes no decisions file. Reach for it when you are **not working in a working directory** (sharpening a plan, a design, a piece of writing, anything with no repo under it). If you are in a working directory, use `/grill-with-docs` instead: it runs the same interview and leaves a paper trail.
- **`/grilling`** is the interview primitive itself: rounds, the frontier, facts are the agent's job and decisions are yours. `/grill-me` and `/grill-with-docs` are the two named ways in. Reach for it directly only when you want the interview with no wrapper around it.
- **`/to-questionnaire`** comes in when the thing blocking you isn't in your head or the codebase but in **someone else's**: a client, a designer, a colleague. It writes them a questionnaire to fill in, interviewing you about the **send** (who it's going to, what you need back) and aiming the questions at the gap. Use it only when you directly ask for questions for someone else; many open questions, a complex feature or a mention of a client are not enough. It prepares the document and does not send it. What comes back is material for `/grill-with-docs` or `/to-spec`.
- **`/wait-what`** is the corrective for a message that didn't land. Use it mid-conversation, inside any other skill, and the agent re-pitches what it just said with the context you were missing, in plain English, using the terms already agreed in the conversation.
