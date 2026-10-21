//! Pure parsing of a plugin package in the portable (`plugin.json`, `mcp.json`) or Codex
//! (`.codex-plugin/plugin.json`, `.mcp.json`) layout. Native only reads bounded files.
use crate::codex_types::codex_hex;
use crate::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const MAX_SKILLS: usize = 8;
const MAX_SKILL: usize = 12 * 1024;
const MAX_SKILLS_TOTAL: usize = 48 * 1024;
pub(crate) const MAX_PACKAGE: usize = 256 * 1024;

pub(crate) struct ParsedPackage {
    pub name: String,
    pub version: String,
    pub hash: [u8; 32],
    pub package: PluginPackage,
    pub warnings: Vec<String>,
}

pub fn review_plugin(files: &[PluginFile<'_>]) -> Result<PluginReview, StoreError> {
    let p = parse_package(files)?;
    Ok(PluginReview {
        name: p.name,
        version: p.version,
        hash: codex_hex(&p.hash),
        servers: p.package.servers,
        skills: p.package.skills,
        warnings: p.warnings,
    })
}

/// Hash over files sorted by path, each as length-prefixed (path, bytes): one value for review
/// and install whatever order native read them in.
fn package_hash(files: &[PluginFile<'_>]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([1]);
    for f in files {
        h.update((f.path.len() as u32).to_be_bytes());
        h.update(f.path.as_bytes());
        h.update((f.bytes.len() as u32).to_be_bytes());
        h.update(f.bytes);
    }
    h.finalize().into()
}

pub(crate) fn parse_package(input: &[PluginFile<'_>]) -> Result<ParsedPackage, StoreError> {
    if input.is_empty() {
        return Err(StoreError::InvalidInput);
    }
    if input.len() > MAX_PLUGIN_FILES {
        return Err(StoreError::ResourceLimit);
    }
    let mut files = input.to_vec();
    files.sort_by(|a, b| a.path.cmp(b.path));
    let mut total = 0usize;
    for (i, f) in files.iter().enumerate() {
        if i > 0 && files[i - 1].path == f.path {
            return Err(StoreError::InvalidInput);
        }
        total += f.bytes.len();
        if f.bytes.len() > MAX_PLUGIN_FILE || total > MAX_PLUGIN_FILES_TOTAL {
            return Err(StoreError::ResourceLimit);
        }
    }
    let hash = package_hash(&files);
    let mut manifest = None;
    let mut mcp = None;
    let mut skills = Vec::new();
    for f in &files {
        let slot = match (f.kind, f.path) {
            (PluginFileKind::Manifest, "plugin.json" | ".codex-plugin/plugin.json") => {
                &mut manifest
            }
            (PluginFileKind::Mcp, "mcp.json" | ".mcp.json") => &mut mcp,
            (PluginFileKind::Skill, path) => {
                skills.push((skill_dir(path)?, utf8(f.bytes)?));
                continue;
            }
            _ => return Err(StoreError::InvalidInput),
        };
        if slot.replace(f.bytes).is_some() {
            return Err(StoreError::InvalidInput);
        }
    }
    let mut warnings = Vec::new();
    let manifest: Value = serde_json::from_slice(manifest.ok_or(StoreError::InvalidInput)?)
        .map_err(|_| StoreError::InvalidInput)?;
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| bounded(s, 1, 256))
        .ok_or(StoreError::InvalidInput)?
        .to_owned();
    let version = match manifest.get("version") {
        None => String::new(),
        Some(v) => v
            .as_str()
            .filter(|s| bounded(s, 0, 64))
            .ok_or(StoreError::InvalidInput)?
            .to_owned(),
    };
    if ["apps", "interface", "extensions"]
        .iter()
        .any(|k| manifest.get(k).is_some())
    {
        warnings.push("OpenAI app and interface extensions are ignored".into());
    }
    let servers = match mcp {
        Some(bytes) => servers(bytes, &hash, &mut warnings)?,
        None => Vec::new(),
    };
    if servers.is_empty() {
        return Err(StoreError::InvalidInput);
    }
    if skills.len() > MAX_SKILLS {
        return Err(StoreError::ResourceLimit);
    }
    let mut skill_bytes = 0usize;
    let skills = skills
        .into_iter()
        .map(|(dir, text)| {
            if text.len() > MAX_SKILL {
                return Err(StoreError::TextTooLong);
            }
            skill_bytes += text.len();
            if skill_bytes > MAX_SKILLS_TOTAL {
                return Err(StoreError::ResourceLimit);
            }
            Ok(skill(dir, text))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let package = PluginPackage { servers, skills };
    let json = serde_json::to_string(&package).map_err(|_| StoreError::InvalidInput)?;
    if json.len() > MAX_PACKAGE {
        return Err(StoreError::ResourceLimit);
    }
    Ok(ParsedPackage {
        name,
        version,
        hash,
        package,
        warnings,
    })
}

fn bounded(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && !s.contains('\0')
}
fn utf8(bytes: &[u8]) -> Result<&str, StoreError> {
    std::str::from_utf8(bytes).map_err(|_| StoreError::InvalidInput)
}
/// `skills/<dir>/SKILL.md` with a plain directory name only.
fn skill_dir(path: &str) -> Result<&str, StoreError> {
    path.strip_prefix("skills/")
        .and_then(|p| p.strip_suffix("/SKILL.md"))
        .filter(|d| {
            bounded(d, 1, 64)
                && *d != "."
                && *d != ".."
                && d.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        .ok_or(StoreError::InvalidInput)
}
/// Name and description come from optional `---` front matter; the full text is kept.
fn skill(dir: &str, text: &str) -> PluginSkill {
    let mut name = dir.to_owned();
    let mut description = String::new();
    if let Some(rest) = text.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---")
    {
        for line in rest[..end].lines() {
            if let Some((k, v)) = line.split_once(':') {
                let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
                match k.trim() {
                    "name" if bounded(v, 1, 128) => name = v.to_owned(),
                    "description" if bounded(v, 0, 1024) => description = v.to_owned(),
                    _ => {}
                }
            }
        }
    }
    PluginSkill {
        name,
        description,
        text: text.to_owned(),
    }
}
pub(crate) fn valid_server_key(key: &str) -> bool {
    (1..=32).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn servers(
    bytes: &[u8],
    hash: &[u8; 32],
    warnings: &mut Vec<String>,
) -> Result<Vec<PluginServerSpec>, StoreError> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| StoreError::InvalidInput)?;
    let entries = v
        .get("mcpServers")
        .and_then(Value::as_object)
        .ok_or(StoreError::InvalidInput)?;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, entry) in entries {
        if !valid_server_key(key) || !seen.insert(key) {
            return Err(StoreError::InvalidInput);
        }
        if entry.get("url").is_some() && entry.get("command").is_some() {
            return Err(StoreError::InvalidInput);
        }
        if let Some(id) = entry.get("ambient_managed") {
            if entry.as_object().is_some_and(|o| o.len() > 1) {
                return Err(StoreError::InvalidInput);
            }
            // Only the exact built-in package may name a managed server (D71); review and install
            // both reach this through `parse_package`. Open-time checks are structural only.
            let id = id
                .as_str()
                .and_then(ManagedServerId::parse)
                .filter(|id| id.builtin_package_hash() == *hash);
            match id {
                Some(id) => out.push(PluginServerSpec {
                    key: key.clone(),
                    kind: ServerKind::Managed { id },
                }),
                None => warnings.push(format!(
                    "Server {key} is skipped: only Ambient's built-in package can use a server that Ambient installs"
                )),
            }
            continue;
        }
        let kind = match entry.get("type").map(Value::as_str) {
            None | Some(Some("stdio")) if entry.get("command").is_some() => {
                local_entry(key, entry, warnings)?
            }
            None | Some(Some("http" | "streamable-http")) => remote_entry(key, entry, warnings),
            _ => None,
        };
        let Some(kind) = kind else {
            warnings.push(format!(
                "Server {key} is skipped: only remote HTTPS servers and commands on this Mac are supported"
            ));
            continue;
        };
        out.push(PluginServerSpec {
            key: key.clone(),
            kind,
        });
    }
    if out.len() > MAX_PLUGIN_SERVERS {
        return Err(StoreError::ResourceLimit);
    }
    Ok(out)
}
fn remote_entry(key: &str, entry: &Value, warnings: &mut Vec<String>) -> Option<ServerKind> {
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .and_then(remote_url)?;
    // Only the URL and the need to sign in are read; package headers and secrets never are.
    let auth_required = [
        "headers",
        "bearer_token_env_var",
        "env_http_headers",
        "auth",
        "oauth",
    ]
    .iter()
    .any(|k| entry.get(k).is_some());
    if entry
        .as_object()
        .is_some_and(|o| o.keys().any(|k| !matches!(k.as_str(), "type" | "url")))
    {
        warnings.push(format!(
            "Server {key}: headers, environment variables and secrets in the package are ignored"
        ));
    }
    Some(ServerKind::Remote { url, auth_required })
}
/// A command server: limits and forbidden variable names reject the package; a command inside
/// the package folder is skipped. Variable values in the package are never read.
fn local_entry(
    key: &str,
    entry: &Value,
    warnings: &mut Vec<String>,
) -> Result<Option<ServerKind>, StoreError> {
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .ok_or(StoreError::InvalidInput)?;
    let args = match entry.get("args") {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .ok_or(StoreError::InvalidInput)?
            .iter()
            .map(|a| {
                a.as_str()
                    .map(str::to_owned)
                    .ok_or(StoreError::InvalidInput)
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    let env = match entry.get("env") {
        None => Vec::new(),
        Some(v) => v
            .as_object()
            .ok_or(StoreError::InvalidInput)?
            .keys()
            .cloned()
            .collect(),
    };
    let Some(kind) = local_kind(command, &args, env)? else {
        warnings.push(format!(
            "Server {key} is skipped: commands that run files from the plugin folder are not supported"
        ));
        return Ok(None);
    };
    if entry.get("cwd").is_some() {
        warnings.push(format!("Server {key}: the working folder (cwd) is ignored"));
    }
    if args.iter().any(|a| a.contains("${")) {
        warnings.push(format!(
            "Server {key}: ${{VAR}} placeholders in arguments are passed literally"
        ));
    }
    if args.iter().any(|a| relative_path(a)) {
        warnings.push(format!(
            "Server {key}: an argument looks like a relative path; the program starts in an empty private folder"
        ));
    }
    if entry.as_object().is_some_and(|o| {
        o.keys()
            .any(|k| !matches!(k.as_str(), "type" | "command" | "args" | "env" | "cwd"))
    }) {
        warnings.push(format!("Server {key}: other package settings are ignored"));
    }
    Ok(Some(kind))
}
/// A path with a directory part that is not absolute: `./x`, `../x`, `scripts/x`.
fn relative_path(text: &str) -> bool {
    text.contains('/') && !text.starts_with(['/', '@', '-']) && !text.contains("://")
}
/// The one validating constructor of a local server, used by install and by open-time checks.
/// `None` is a command inside the package folder (skipped with a warning).
pub(crate) fn local_kind(
    command: &str,
    args: &[String],
    mut env: Vec<String>,
) -> Result<Option<ServerKind>, StoreError> {
    if !bounded(command, 1, 1024)
        || args.len() > 32
        || args.iter().any(|a| !bounded(a, 0, 1024))
        || env.len() > 16
    {
        return Err(StoreError::InvalidInput);
    }
    env.sort();
    for (i, name) in env.iter().enumerate() {
        let b = name.as_bytes();
        let valid = (1..=64).contains(&b.len())
            && (b[0].is_ascii_uppercase() || b[0] == b'_')
            && b.iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_');
        // Package names are untrusted: they must never override the environment Ambient sets.
        let forbidden = matches!(name.as_str(), "PATH" | "HOME" | "TMPDIR")
            || name.starts_with("DYLD_")
            || name.starts_with("LD_");
        if !valid || forbidden || (i > 0 && env[i - 1] == *name) {
            return Err(StoreError::InvalidInput);
        }
    }
    if relative_path(command) || command.starts_with('.') {
        return Ok(None);
    }
    Ok(Some(ServerKind::Local {
        command: command.to_owned(),
        args: args.to_vec(),
        env,
    }))
}
/// The normalised URL of a remote server: HTTPS to a named host only. Loopback HTTP exists only
/// in test-support builds.
pub(crate) fn remote_url(text: &str) -> Option<String> {
    let url = url::Url::parse(text).ok()?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return None;
    }
    #[cfg(feature = "model-test-support")]
    if url.scheme() == "http"
        && url.host() == Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
        && url.port().is_some()
    {
        return Some(url.into());
    }
    match url.host()? {
        url::Host::Domain(host)
            if url.scheme() == "https"
                && !host.trim_end_matches('.').eq_ignore_ascii_case("localhost") =>
        {
            Some(url.into())
        }
        _ => None,
    }
}

pub(crate) fn valid_tool_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
/// Canonicalizes one raw MCP `Tool` object. `serde_json` maps are sorted, so `to_string` is the
/// canonical text; the hash covers `readOnlyHint`, so a lost hint is a changed definition.
pub fn canonical_tool(raw: &Value) -> Result<CanonicalTool, StoreError> {
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| valid_tool_name(n))
        .ok_or(StoreError::InvalidInput)?;
    let mut definition = serde_json::Map::new();
    definition.insert("name".into(), Value::String(name.into()));
    if let Some(d) = raw.get("description") {
        definition.insert("description".into(), d.clone());
    }
    if let Some(s) = raw.get("inputSchema") {
        definition.insert("inputSchema".into(), s.clone());
    }
    let hint = raw.get("annotations").and_then(|a| a.get("readOnlyHint"));
    if let Some(h) = hint {
        definition.insert("readOnlyHint".into(), h.clone());
    }
    let definition = Value::Object(definition).to_string();
    let read_only = hint == Some(&Value::Bool(true));
    Ok(CanonicalTool {
        name: name.into(),
        hash: sha256(definition.as_bytes()),
        approvable: read_only && definition.len() <= MAX_TOOL_DEFINITION,
        read_only,
        definition,
    })
}
/// Re-validates a pinned definition text: it must already be canonical, read-only and bounded.
pub(crate) fn pinned_definition(text: &str) -> Result<CanonicalTool, StoreError> {
    if text.len() > MAX_TOOL_DEFINITION {
        return Err(StoreError::InvalidInput);
    }
    let v: Value = serde_json::from_str(text).map_err(|_| StoreError::InvalidInput)?;
    let o = v.as_object().ok_or(StoreError::InvalidInput)?;
    if o.keys().any(|k| {
        !matches!(
            k.as_str(),
            "name" | "description" | "inputSchema" | "readOnlyHint"
        )
    }) || serde_json::to_string(&v).ok().as_deref() != Some(text)
        || o.get("readOnlyHint") != Some(&Value::Bool(true))
    {
        return Err(StoreError::InvalidInput);
    }
    let name = o
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| valid_tool_name(n))
        .ok_or(StoreError::InvalidInput)?;
    Ok(CanonicalTool {
        name: name.into(),
        definition: text.into(),
        hash: sha256(text.as_bytes()),
        read_only: true,
        approvable: true,
    })
}
/// Stateless discovery result for the review: approvable tools carry their exact definition.
fn discovered_tools(listed: &[Value]) -> Result<Vec<Value>, StoreError> {
    if listed.len() > MAX_LISTED_TOOLS {
        return Err(StoreError::ResourceLimit);
    }
    let mut out = Vec::new();
    for raw in listed {
        let Ok(t) = canonical_tool(raw) else {
            continue;
        };
        let description = raw
            .get("description")
            .and_then(Value::as_str)
            .map(|d| d.get(..d.floor_char_boundary(1024)).unwrap_or_default());
        let mut v = serde_json::json!({"name":t.name,"hash":codex_hex(&t.hash),"read_only":t.read_only,"approvable":t.approvable,"description":description});
        if t.approvable {
            v["definition"] = Value::String(t.definition);
        }
        out.push(v);
    }
    Ok(out)
}
/// One server as native shows it: the stored spec plus Core's derived facts.
pub(crate) fn server_json(s: &PluginServerSpec) -> Result<Value, StoreError> {
    let mut v = serde_json::to_value(s).map_err(|_| StoreError::InvalidInput)?;
    v["needs_key"] = Value::Bool(s.kind.needs_key());
    v["downloads"] = Value::Bool(s.kind.downloads_code());
    Ok(v)
}
impl PluginReview {
    pub fn json(&self) -> Result<String, StoreError> {
        let mut v = serde_json::to_value(self).map_err(|_| StoreError::InvalidInput)?;
        v["servers"] = Value::Array(
            self.servers
                .iter()
                .map(server_json)
                .collect::<Result<_, _>>()?,
        );
        Ok(v.to_string())
    }
}
/// The stateless discovery answer for native: canonical tools, or the named failure. A local
/// server adds its redacted stderr tail on failure and an unconfirmed stop.
pub fn discovery_json(d: &Discovery, target: &Target) -> Result<String, StoreError> {
    let mut v = match &d.listed {
        Ok(tools) => {
            let mut tools = discovered_tools(tools)?;
            // The managed server's tools are approvable only by name in the Core allowlist.
            if let Target::Managed(id) = target {
                for t in &mut tools {
                    if !t["name"].as_str().is_some_and(|n| id.allows(n)) {
                        t["approvable"] = Value::Bool(false);
                        if let Some(o) = t.as_object_mut() {
                            o.remove("definition");
                        }
                    }
                }
            }
            serde_json::json!({"status":"ok","tools":tools})
        }
        Err(outcome) => {
            let status = match outcome {
                PluginCallOutcome::LoginRequired => "login_required",
                PluginCallOutcome::NotConfigured => "not_configured",
                PluginCallOutcome::Timeout => "timeout",
                PluginCallOutcome::Cancelled => "cancelled",
                PluginCallOutcome::Invalid | PluginCallOutcome::ToolError => "invalid",
                _ => "unavailable",
            };
            let mut v = serde_json::json!({ "status": status });
            if let Some(tail) = &d.stderr {
                v["stderr"] = Value::String(tail.clone());
            }
            v
        }
    };
    if !d.stop_confirmed {
        v["stop_confirmed"] = Value::Bool(false);
    }
    Ok(v.to_string())
}
