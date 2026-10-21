import { spawnSync } from "node:child_process"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { parse as parseToml } from "smol-toml"
import { afterEach, describe, expect, it } from "vitest"
import { parse as parseYaml } from "yaml"
import { generateAgentSources } from "./generate-agent-sources.mjs"

const script = fileURLToPath(
   new URL("./generate-agent-sources.mjs", import.meta.url),
)
const roots = []
const put = async (root, file, content) => {
   const target = path.join(root, file)
   await mkdir(path.dirname(target), { recursive: true })
   await writeFile(target, content)
}
const json = (value) => `${JSON.stringify(value, null, 3)}\n`
const frontmatter = (text) => parseYaml(text.split("---")[1])

const fixture = async () => {
   const root = await mkdtemp(path.join(tmpdir(), "hh-agent-sources-"))
   roots.push(root)
   await put(
      root,
      "agent-sources/manifest.json",
      json({
         documents: [
            { output: "AGENTS.md", sources: ["rules/universal.md"] },
            {
               output: "AGENTS.override.md",
               sources: ["entrypoints/codex.md"],
               includeGenerated: ["AGENTS.md"],
            },
         ],
         rules: [
            {
               name: "components",
               source: "rules/components.md",
               claude: { paths: ["src/components/**"] },
               cursor: {
                  alwaysApply: false,
                  globs: "src/components/**",
                  description: "Component rules",
               },
            },
         ],
         scopes: [
            {
               output: "src/components/AGENTS.override.md",
               sources: ["rules/components.md"],
            },
         ],
         environments: {
            codex: { label: "Codex", variables: {} },
            claude: { label: "Claude Code", variables: {} },
            cursor: { label: "Cursor", variables: {} },
            opencode: { label: "OpenCode", variables: {} },
         },
         mattCopies: ["claude"],
      }),
   )
   await put(
      root,
      "agent-sources/models.json",
      json({
         codex: {
            research: { model: "gpt-example", model_reasoning_effort: "high" },
         },
         claude: { research: { model: "inherit" } },
         cursor: { research: { model: "composer-example[fast=false]" } },
         opencode: { research: { model: "example/research" } },
      }),
   )
   await put(
      root,
      "agent-sources/rules/universal.md",
      "# Universal\n\nUse existing libraries.\n",
   )
   await put(root, "agent-sources/entrypoints/codex.md", "# Codex adapter\n")
   await put(
      root,
      "agent-sources/rules/components.md",
      "# Components\n\nKeep complete scoped instructions here.\n",
   )
   await put(
      root,
      "agent-sources/agents/researcher.md",
      'Read the full role. Quotes: "x"; path: C:\\repo.\nNever delegate.\n',
   )
   await put(
      root,
      "agent-sources/agents/researcher.json",
      json({
         description: 'Research: "official" sources',
         environments: {
            codex: {
               tier: "research",
               settings: { name: "researcher", sandbox_mode: "read-only" },
            },
            claude: {
               tier: "research",
               settings: {
                  name: "researcher",
                  tools: ["Read", "Grep", "Glob"],
               },
            },
            cursor: {
               tier: "research",
               settings: { name: "researcher", readonly: true },
            },
            opencode: {
               tier: "research",
               settings: {
                  mode: "subagent",
                  permission: { edit: "deny", task: "deny" },
               },
            },
         },
      }),
   )
   await put(
      root,
      "agent-sources/skills/harmony-bootstrap/SKILL.md",
      "---\nname: harmony-bootstrap\ndescription: Start in {{environment}}.\n---\n\n# Bootstrap\n\nUse native rules.\n",
   )
   await put(
      root,
      ".agents/skills/matt/SKILL.md",
      "---\nname: matt\ndescription: Ask questions.\n---\n\n# Matt\n",
   )
   await put(root, ".agents/skills/matt/reference.md", "# Reference\n")
   await put(
      root,
      ".agents/skills/matt/agents/openai.yaml",
      "interface:\n  display_name: Matt\n",
   )
   return root
}

afterEach(async () => {
   await Promise.all(
      roots.splice(0).map((root) => rm(root, { recursive: true, force: true })),
   )
})

describe("agent sources generator", () => {
   it("embeds the generated AGENTS.md in the root override and updates both from one source", async () => {
      const root = await fixture()
      await generateAgentSources(root)
      const universal = await readFile(path.join(root, "AGENTS.md"), "utf8")
      expect(
         await readFile(path.join(root, "AGENTS.override.md"), "utf8"),
      ).toContain(universal)
      await put(
         root,
         "agent-sources/rules/universal.md",
         "# Universal\n\nUpdated once.\n",
      )
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual(expect.arrayContaining(["AGENTS.md", "AGENTS.override.md"]))
      await generateAgentSources(root)
      const updated = await readFile(path.join(root, "AGENTS.md"), "utf8")
      expect(
         await readFile(path.join(root, "AGENTS.override.md"), "utf8"),
      ).toContain(updated)
      expect(updated).toContain("Updated once.")
   })

   it("removes routed rule and scoped outputs after their source mappings are deleted", async () => {
      const root = await fixture()
      await generateAgentSources(root)
      const manifestFile = path.join(root, "agent-sources/manifest.json")
      const manifest = JSON.parse(await readFile(manifestFile, "utf8"))
      manifest.rules = []
      manifest.scopes = []
      await writeFile(manifestFile, json(manifest))
      const stale = [
         ".claude/rules/components.md",
         ".cursor/rules/components.mdc",
         "src/components/AGENTS.override.md",
      ]
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual(expect.arrayContaining(stale))
      await generateAgentSources(root)
      for (const file of stale)
         await expect(readFile(path.join(root, file))).rejects.toMatchObject({
            code: "ENOENT",
         })
   })

   it("detects removed or renamed generated documents without claiming unrelated files", async () => {
      const root = await fixture()
      const manifestFile = path.join(root, "agent-sources/manifest.json")
      const manifest = JSON.parse(await readFile(manifestFile, "utf8"))
      manifest.documents.push(
         { output: "CLAUDE.md", sources: ["rules/universal.md"] },
         {
            output: ".opencode/opencode.json",
            sources: ["environments/opencode.json"],
         },
      )
      manifest.scopes.push({
         output: "src/components/AGENTS.md",
         sources: ["rules/components.md"],
      })
      await put(root, "agent-sources/environments/opencode.json", "{}\n")
      await writeFile(manifestFile, json(manifest))
      await generateAgentSources(root)
      await put(root, "src/unmanaged/AGENTS.md", "Handwritten instructions.\n")
      await put(
         root,
         ".claude/worktrees/keep/CLAUDE.md",
         "GENERATED AGENT FILE. DO NOT EDIT.\n",
      )
      manifest.documents = manifest.documents.filter(
         ({ output }) =>
            !["CLAUDE.md", ".opencode/opencode.json"].includes(output),
      )
      manifest.scopes = manifest.scopes.filter(
         ({ output }) => output !== "src/components/AGENTS.md",
      )
      manifest.scopes.push({
         output: "src/renamed/AGENTS.md",
         sources: ["rules/components.md"],
      })
      await writeFile(manifestFile, json(manifest))
      const stale = [
         "CLAUDE.md",
         ".opencode/opencode.json",
         "src/components/AGENTS.md",
      ]
      const { differences } = await generateAgentSources(root, { check: true })
      expect(differences).toEqual(expect.arrayContaining(stale))
      expect(differences).toContain("src/renamed/AGENTS.md")
      for (const file of stale)
         expect(await readFile(path.join(root, file))).toBeTruthy()
      await generateAgentSources(root)
      for (const file of stale)
         await expect(readFile(path.join(root, file))).rejects.toMatchObject({
            code: "ENOENT",
         })
      expect(
         await readFile(path.join(root, "src/unmanaged/AGENTS.md"), "utf8"),
      ).toBe("Handwritten instructions.\n")
      expect(
         await readFile(
            path.join(root, ".claude/worktrees/keep/CLAUDE.md"),
            "utf8",
         ),
      ).toContain("GENERATED AGENT FILE")
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual([])
   })

   it("renders parseable native agents, scoped rules and skills in all four environments", async () => {
      const root = await fixture()
      await generateAgentSources(root)
      const role = await readFile(
         path.join(root, "agent-sources/agents/researcher.md"),
         "utf8",
      )
      const codex = parseToml(
         await readFile(
            path.join(root, ".codex/agents/researcher.toml"),
            "utf8",
         ),
      )
      expect(codex).toMatchObject({
         name: "researcher",
         model: "gpt-example",
         model_reasoning_effort: "high",
         sandbox_mode: "read-only",
         developer_instructions: role.trim(),
      })
      for (const environment of ["claude", "cursor", "opencode"]) {
         const text = await readFile(
            path.join(root, `.${environment}/agents/researcher.md`),
            "utf8",
         )
         expect(text.startsWith("---\n")).toBe(true)
         expect(frontmatter(text).description).toBe(
            'Research: "official" sources',
         )
         expect(text).toContain(role.trim())
      }
      expect(
         frontmatter(
            await readFile(
               path.join(root, ".claude/rules/components.md"),
               "utf8",
            ),
         ),
      ).toEqual({ paths: ["src/components/**"] })
      expect(
         frontmatter(
            await readFile(
               path.join(root, ".cursor/rules/components.mdc"),
               "utf8",
            ),
         ),
      ).toMatchObject({ alwaysApply: false, globs: "src/components/**" })
      expect(
         await readFile(
            path.join(root, "src/components/AGENTS.override.md"),
            "utf8",
         ),
      ).toContain("Keep complete scoped instructions here.")
      for (const environment of ["codex", "claude", "cursor", "opencode"]) {
         const skill = await readFile(
            path.join(
               root,
               `.${environment}/skills/harmony-bootstrap/SKILL.md`,
            ),
            "utf8",
         )
         expect(frontmatter(skill).name).toBe("harmony-bootstrap")
         expect(frontmatter(skill).description).not.toContain("{{")
         expect(skill).toContain("DO NOT EDIT")
      }
   })

   it("checks matching output without changing any files and regenerates deterministically", async () => {
      const root = await fixture()
      const first = await generateAgentSources(root)
      const expected = await readFile(
         path.join(root, ".codex/agents/researcher.toml"),
         "utf8",
      )
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual([])
      expect((await generateAgentSources(root)).generated).toBe(first.generated)
      expect(
         await readFile(
            path.join(root, ".codex/agents/researcher.toml"),
            "utf8",
         ),
      ).toBe(expected)
   })

   it("CLI --check exits 1 for changed, missing and extra owned files, without repairing them", async () => {
      const root = await fixture()
      await generateAgentSources(root)
      const cli = () =>
         spawnSync(process.execPath, [script, "--check"], {
            cwd: root,
            encoding: "utf8",
         })
      expect(cli().status).toBe(0)
      await put(root, "AGENTS.md", "Changed\n")
      await rm(path.join(root, ".claude/agents/researcher.md"))
      await put(root, ".cursor/rules/obsolete.mdc", "Extra\n")
      await put(root, ".opencode/agents/nested/obsolete.md", "Extra\n")
      const result = cli()
      expect(result.status).toBe(1)
      for (const file of [
         "AGENTS.md",
         ".claude/agents/researcher.md",
         ".cursor/rules/obsolete.mdc",
         ".opencode/agents/nested/obsolete.md",
      ])
         expect(result.stderr).toContain(file)
      expect(await readFile(path.join(root, "AGENTS.md"), "utf8")).toBe(
         "Changed\n",
      )
      expect(
         await readFile(path.join(root, ".cursor/rules/obsolete.mdc"), "utf8"),
      ).toBe("Extra\n")
      await generateAgentSources(root)
      expect(cli().status).toBe(0)
   })

   it("deletes obsolete agents and harmony skills while preserving unrelated skills, commands and worktrees", async () => {
      const root = await fixture()
      await generateAgentSources(root)
      await rm(path.join(root, "agent-sources/agents/researcher.json"))
      await rm(path.join(root, "agent-sources/skills/harmony-bootstrap"), {
         recursive: true,
      })
      const preserved = [
         ".cursor/skills/improve-animations/SKILL.md",
         ".claude/skills/personal/SKILL.md",
         ".codex/skills/personal/SKILL.md",
         ".cursor/commands/task.md",
         ".claude/worktrees/example/.claude/agents/local.md",
      ]
      for (const file of preserved) await put(root, file, "Unrelated\n")
      await put(
         root,
         ".cursor/skills/harmony-obsolete/SKILL.md",
         "Old harmony skill\n",
      )
      const check = await generateAgentSources(root, { check: true })
      expect(check.differences).toContain(".codex/agents/researcher.toml")
      await generateAgentSources(root)
      for (const file of preserved)
         expect(await readFile(path.join(root, file), "utf8")).toBe(
            "Unrelated\n",
         )
      await expect(
         readFile(path.join(root, ".codex/agents/researcher.toml")),
      ).rejects.toMatchObject({ code: "ENOENT" })
      await expect(
         readFile(path.join(root, ".cursor/skills/harmony-obsolete/SKILL.md")),
      ).rejects.toMatchObject({ code: "ENOENT" })
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual([])
   })

   it("copies Matt supporting files and binary assets, catches drift and deletes removed Matt copies", async () => {
      const root = await fixture()
      const binary = Buffer.from([0, 255, 128, 10])
      await put(root, ".agents/skills/matt/image.bin", binary)
      await generateAgentSources(root)
      expect(
         await readFile(path.join(root, ".claude/skills/matt/image.bin")),
      ).toEqual(binary)
      expect(
         frontmatter(
            await readFile(
               path.join(root, ".claude/skills/matt/SKILL.md"),
               "utf8",
            ),
         ),
      ).toMatchObject({ name: "matt" })
      expect(
         parseYaml(
            await readFile(
               path.join(root, ".claude/skills/matt/agents/openai.yaml"),
               "utf8",
            ),
         ),
      ).toEqual({ interface: { display_name: "Matt" } })
      await put(root, ".claude/skills/matt/reference.md", "Drift\n")
      await put(root, ".claude/skills/matt/extra.md", "Extra\n")
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual(
         expect.arrayContaining([
            ".claude/skills/matt/reference.md",
            ".claude/skills/matt/extra.md",
         ]),
      )
      await rm(path.join(root, ".agents/skills/matt"), { recursive: true })
      await generateAgentSources(root)
      await expect(
         readFile(path.join(root, ".claude/skills/matt/SKILL.md")),
      ).rejects.toMatchObject({ code: "ENOENT" })
      expect(
         (await generateAgentSources(root, { check: true })).differences,
      ).toEqual([])
   })
})
