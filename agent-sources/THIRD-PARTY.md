# Simplicity reviewer attribution

The HarmonyHunter reviewer prompts are modified adaptations, with original
wording and repository-specific constraints, from the following sources:

- Anthropic [code-simplifier](https://github.com/anthropics/claude-plugins-official/blob/d182ca456ca09d31d139f7d3818d1d333b103cce/plugins/code-simplifier/agents/code-simplifier.md): behavior preservation and clarity without excessive compression. [Apache-2.0 license](licenses/anthropic-apache-2.0.txt).
- EveryInc [ce-simplify-code](https://github.com/EveryInc/compound-engineering-plugin/blob/1a9f16c45e659d3bcb572b9e73c50aabccdc8b86/skills/ce-simplify-code/SKILL.md): reuse, safety checks and reachable branches. Copyright 2025 Every. [MIT license](licenses/every-mit.txt).
- Addy Osmani [code-simplification](https://github.com/addyosmani/agent-skills/blob/fea75b16472b/skills/code-simplification/SKILL.md): understand existing behavior before deleting it; changed test expectations can mean changed behavior. Copyright 2025 Addy Osmani. [MIT license](licenses/addy-osmani-mit.txt).
- Matt Pocock [codebase-design](https://github.com/mattpocock/skills/blob/321658273cb1d20b76026717d027d505790106d4/skills/engineering/codebase-design/SKILL.md), DEEPENING, and [Design It Twice](https://github.com/mattpocock/skills/blob/d80fa0f4ebe0/skills/engineering/codebase-design/DESIGN-IT-TWICE.md): deep modules, deletion test, real variation before adapters, comparing independent designs. Copyright 2026 Matt Pocock. [MIT license](licenses/matt-pocock-mit.txt).

Claude Code's documented simplify review lenses informed the four-direction
brief; no proprietary command prompt was copied. The trusted-path idea associated
with deslop is paraphrased in original wording; no unlicensed prompt text is copied.
These agents do not inherit upstream multi-agent execution, automatic fixes,
posting, commits or flexible-architecture objectives.
