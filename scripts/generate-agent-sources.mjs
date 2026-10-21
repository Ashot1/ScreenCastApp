import { promises as fs } from "node:fs"
import path from "node:path"
import { pathToFileURL } from "node:url"
import { stringify as stringifyToml } from "smol-toml"
import { stringify as stringifyYaml } from "yaml"

const ENVIRONMENTS = ["codex", "claude", "cursor", "opencode"]
const MARKER = "GENERATED AGENT FILE. DO NOT EDIT."
const SKILL_MARKER = ".agent-sources-generated"

// Missing generated files/directories are normal on the first run; other FS errors must surface.
const missingIs = (fallback) => (error) => {
   if (error.code === "ENOENT") return fallback
   throw error
}
const entries = async (directory) =>
   (await fs.readdir(directory, { withFileTypes: true }).catch(missingIs([])))
      .filter((entry) => entry.name !== ".DS_Store")
      .sort((a, b) => a.name.localeCompare(b.name, "en"))

const files = async (root, directory) => {
   const result = []
   for (const entry of await entries(path.join(root, directory))) {
      const relative = `${directory}/${entry.name}`
      if (entry.isDirectory()) result.push(...(await files(root, relative)))
      else result.push(relative)
   }
   return result
}

const render = (content, variables) =>
   content.replace(/\{\{([\w-]+)\}\}/g, (_, key) => {
      if (!Object.hasOwn(variables, key))
         throw new Error(`Unknown agent source variable: ${key}`)
      return variables[key]
   })

const annotate = (content, output, source) => {
   const notice = `${MARKER}\nSource: ${source}\nEdit the source and run yarn agents:generate.\n`
   if (/\.(md|mdc)$/.test(output)) {
      const header = `<!--\n${notice}-->\n\n`
      const frontmatter = content.match(/^---\r?\n[\s\S]*?\r?\n---\r?\n/)
      if (frontmatter)
         return `${frontmatter[0]}\n${header}${content.slice(frontmatter[0].length).trimStart()}`
      return `${header}${content}`
   }
   const prefix = /\.jsonc?$/.test(output) ? "// " : "# "
   return `${notice
      .trimEnd()
      .split("\n")
      .map((line) => `${prefix}${line}`)
      .join("\n")}\n\n${content}`
}

const markdown = (metadata, body) =>
   `---\n${stringifyYaml(metadata, { lineWidth: 0 })}---\n\n${body.trim()}\n`

const collectOutput = async (root) => {
   const sourceRoot = path.join(root, "agent-sources")
   const read = (file) => fs.readFile(path.join(sourceRoot, file), "utf8")
   const manifest = JSON.parse(await read("manifest.json"))
   const models = JSON.parse(await read("models.json"))
   const output = new Map()
   const skillDirectories = new Set()
   const add = (file, body, sources) =>
      output.set(file, Buffer.from(annotate(body, file, sources)))
   const compose = async (sources) =>
      `${(await Promise.all(sources.map(read))).map((part) => part.trim()).join("\n\n")}\n`

   for (const document of [...manifest.documents, ...manifest.scopes]) {
      let body = await compose(document.sources)
      const provenance = document.sources.map(
         (source) => `agent-sources/${source}`,
      )
      // Reuse the generated AGENTS.md content so Codex's override includes the same universal rules.
      for (const generated of document.includeGenerated ?? []) {
         body += `\n${output.get(generated).toString()}`
         provenance.push(generated)
      }
      add(document.output, body, provenance.join("; "))
   }
   for (const rule of manifest.rules) {
      for (const environment of ["claude", "cursor"]) {
         if (!rule[environment]) continue
         const body = render(
            await read(rule.source),
            manifest.environments[environment].variables,
         )
         const extension = environment === "cursor" ? "mdc" : "md"
         add(
            `.${environment}/rules/${rule.name}.${extension}`,
            markdown(rule[environment], body),
            `agent-sources/${rule.source}; agent-sources/manifest.json`,
         )
      }
   }
   for (const entry of await entries(path.join(sourceRoot, "agents"))) {
      if (!entry.name.endsWith(".json")) continue
      const name = entry.name.slice(0, -5)
      const agent = JSON.parse(await read(`agents/${entry.name}`))
      const role = await read(`agents/${name}.md`)
      for (const environment of ENVIRONMENTS) {
         const { tier, settings, prefix = "" } = agent.environments[environment]
         const model = models[environment][tier]
         const variables = {
            ...manifest.environments[environment].variables,
            ...model,
         }
         const body =
            `${render(prefix, variables)}${render(role, variables)}`.trim()
         const config = {
            ...settings,
            description: agent.description,
            ...model,
         }
         const filename =
            environment === "opencode" ? name.replaceAll("-", "_") : name
         const extension = environment === "codex" ? "toml" : "md"
         const content =
            environment === "codex"
               ? stringifyToml({ ...config, developer_instructions: body })
               : markdown(config, body)
         add(
            `.${environment}/agents/${filename}.${extension}`,
            content,
            `agent-sources/agents/${name}.md; agent-sources/agents/${name}.json; agent-sources/models.json`,
         )
      }
   }

   const addSkill = async (directory, sourceDirectory, variables) => {
      skillDirectories.add(directory)
      output.set(
         `${directory}/${SKILL_MARKER}`,
         Buffer.from(`${MARKER}\nSource: ${sourceDirectory}\n`),
      )
      for (const sourceFile of await files(root, sourceDirectory)) {
         const relative = sourceFile.slice(sourceDirectory.length + 1)
         const destination = `${directory}/${relative}`
         const content = await fs.readFile(path.join(root, sourceFile))
         // Supporting binary assets are copied byte-for-byte; the directory marker records ownership.
         if (/\.(md|mdc|yaml|yml|toml|jsonc)$/.test(relative)) {
            const text = variables
               ? render(content.toString(), variables)
               : content.toString()
            add(destination, text, sourceFile)
         } else output.set(destination, content)
      }
   }
   for (const entry of await entries(path.join(sourceRoot, "skills"))) {
      if (!entry.isDirectory()) continue
      for (const environment of ENVIRONMENTS) {
         const { label, variables } = manifest.environments[environment]
         await addSkill(
            `.${environment}/skills/${entry.name}`,
            `agent-sources/skills/${entry.name}`,
            { environment: label, ...variables },
         )
      }
   }
   for (const environment of manifest.mattCopies) {
      for (const entry of await entries(path.join(root, ".agents/skills"))) {
         if (!entry.isDirectory()) continue
         await addSkill(
            `.${environment}/skills/${entry.name}`,
            `.agents/skills/${entry.name}`,
         )
      }
   }
   return { output, skillDirectories }
}

const collectOwnedFiles = async (root, output, skillDirectories) => {
   const owned = new Set(output.keys())
   for (const directory of [
      ".claude/rules",
      ".cursor/rules",
      ...ENVIRONMENTS.map((environment) => `.${environment}/agents`),
   ]) {
      for (const file of await files(root, directory)) owned.add(file)
   }
   for (const environment of ENVIRONMENTS) {
      const base = `.${environment}/skills`
      for (const entry of await entries(path.join(root, base))) {
         if (!entry.isDirectory()) continue
         const directory = `${base}/${entry.name}`
         const marker = await fs
            .readFile(path.join(root, directory, SKILL_MARKER), "utf8")
            .catch(missingIs(""))
         if (
            entry.name.startsWith("harmony-") ||
            skillDirectories.has(directory) ||
            marker.startsWith(MARKER)
         ) {
            for (const file of await files(root, directory)) owned.add(file)
         }
      }
   }
   // Only marked files in native instruction homes remain owned after mapping removal.
   const documents = [
      "AGENTS.md",
      "AGENTS.override.md",
      "CLAUDE.md",
      ".codex/config.toml",
      ".opencode/opencode.json",
      ...(await files(root, "src")).filter((file) =>
         ["AGENTS.md", "AGENTS.override.md"].includes(path.basename(file)),
      ),
   ]
   for (const file of documents) {
      const text = await fs
         .readFile(path.join(root, file), "utf8")
         .catch(missingIs(""))
      if (text.includes(MARKER)) owned.add(file)
   }
   return owned
}

export const generateAgentSources = async (
   root = process.cwd(),
   { check = false } = {},
) => {
   const { output, skillDirectories } = await collectOutput(root)
   const owned = await collectOwnedFiles(root, output, skillDirectories)
   const differences = []
   for (const file of [...owned].sort()) {
      const expected = output.get(file)
      const actual = await fs
         .readFile(path.join(root, file))
         .catch(missingIs(null))
      if (expected && actual?.equals(expected)) continue
      differences.push(file)
      if (check) continue
      if (expected) {
         await fs.mkdir(path.dirname(path.join(root, file)), {
            recursive: true,
         })
         await fs.writeFile(path.join(root, file), expected)
      } else await fs.rm(path.join(root, file), { force: true })
   }
   return { differences, generated: output.size }
}

export const run = async () => {
   const check = process.argv.includes("--check")
   const result = await generateAgentSources(process.cwd(), { check })
   if (check && result.differences.length) {
      console.error(
         `Agent files are stale. Run yarn agents:generate.\n${result.differences.join("\n")}`,
      )
      process.exitCode = 1
      return
   }
   console.log(
      check
         ? "Agent files are up to date."
         : `Generated ${result.generated} agent files.`,
   )
}

if (
   process.argv[1] &&
   import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
)
   await run()
