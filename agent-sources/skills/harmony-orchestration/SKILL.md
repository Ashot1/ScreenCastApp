---
name: harmony-orchestration
description: Use when a manual or generic {{environment}} implementation worker will be spawned.
---

# Harmony Orchestration

{{workerPreamble}}

This cost gate applies only to manual/generic implementation workers. Triggered
specialists and routine mechanics are mandatory under `harmony-bootstrap`.

## Manual Worker Gate

Work inline by default. Do not create a worker merely because implementation is
large, multi-file, or multi-step, and do not choose a worker count before
decomposing the work.

Before spawning, define an autonomous work package. It must:

- start without waiting for another agent's result or decision;
- have its product and technical decisions already resolved;
- own exclusive files, modules, resources, or questions;
- avoid changing a contract used by another concurrent package;
- have a clear output, stop condition, and independent verification path;
- integrate without the parent repeating the implementation.

Different files do not make packages autonomous when they share an evolving
contract. Assign at most one owner to each package. Do not create multiple
workers for the same problem or for tightly coupled parts of one implementation.

Delegate only when an autonomous package has a concrete parallelism or context-
isolation benefit that exceeds handoff, coordination, and adoption cost. If the
task has one ordinary package, handle it inline. A single worker for the only
package requires a specific recorded reason showing why a large isolated
implementation is more effective than inline execution.

## Routing Record

Record this before spawning:

```text
HH Worker Routing:
Objective: <already-decided implementation package>
Autonomy: <why the package passes every autonomy condition>
Scope: <exclusive files/modules>
Dependencies: <resolved contracts and non-overlapping parent/worker scopes>
Benefit: <concrete parallelism or context-isolation benefit over inline work>
Nested delegation: <conditions under which this worker may create subagents>
{{workerRecord}}
```

{{workerModels}}

## Worker Contract

Each worker owns decomposition, decisions within its assigned role, integration,
verification and the answer about its own package. Independent implementers have
no lead implementer; dependencies and shared contracts stay explicit.

Every worker prompt must include:

- objective and acceptance criteria;
- why the package is autonomous and worth delegating;
- exclusive write scope and files to avoid;
- relevant repository rules and adopted findings;
- expected checks, evidence, output format, and stop conditions;
- the nested-delegation gate and the worker's responsibility for any subagents;
- instruction not to revert or overwrite other work.

The parent and other workers must not edit the worker-owned files concurrently.
The parent inspects and adopts the result before dependent work.

## Nested Delegation

A generic worker may create subagents only when its assigned package genuinely
decomposes into autonomous subpackages. It applies this same manual-worker gate,
assigns one owner per subpackage, prevents overlapping work, and remains
responsible for synthesis and verification. A subagent with a named specialized
role must complete that role itself and must never create or coordinate further
subagents.

## Boundaries

Do not create generic workers for diagnosis, evaluation, top-level synthesis,
or final decisions. The parent handles those tasks plus integration, final
verification, and the user-facing answer. If implementation exposes an
unexpected failure or unresolved product decision, the worker stops and returns
evidence to the parent.
