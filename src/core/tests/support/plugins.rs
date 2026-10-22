use ambient_core::*;
use serde_json::{Value, json};

pub const MANIFEST: &str = r#"{"name":"Docs","version":"1.0.0"}"#;
pub const MCP: &str = r#"{"mcpServers":{"docs":{"url":"https://mcp.example.com/mcp","headers":{"Authorization":"x"}},"open":{"url":"https://open.example.com/mcp"}}}"#;
pub const SKILL: &str = "---\nname: search-docs\ndescription: Find documents\n---\nSearch first.\n";
pub fn files() -> Vec<PluginFile<'static>> {
    vec![
        PluginFile {
            kind: PluginFileKind::Manifest,
            path: "plugin.json",
            bytes: MANIFEST.as_bytes(),
        },
        PluginFile {
            kind: PluginFileKind::Mcp,
            path: "mcp.json",
            bytes: MCP.as_bytes(),
        },
        PluginFile {
            kind: PluginFileKind::Skill,
            path: "skills/search/SKILL.md",
            bytes: SKILL.as_bytes(),
        },
    ]
}
pub fn raw_tool(name: &str, description: &str) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{"q":{"type":"string"}}},"annotations":{"readOnlyHint":true}})
}
pub fn hash(raw: &Value) -> Result<[u8; 32], StoreError> {
    Ok(canonical_tool(raw)?.hash)
}
pub fn plugin(s: &Store, id: u64) -> Result<Value, Box<dyn std::error::Error>> {
    let all: Value = serde_json::from_str(&s.plugins_json()?)?;
    all["plugins"]
        .as_array()
        .and_then(|v| v.iter().find(|p| p["id"] == id))
        .cloned()
        .ok_or_else(|| "plugin".into())
}
pub fn revision(s: &Store, id: u64) -> Result<u64, Box<dyn std::error::Error>> {
    plugin(s, id)?["revision"]
        .as_u64()
        .ok_or_else(|| "revision".into())
}
pub fn generation(s: &Store, id: u64, server: &str) -> Result<u64, Box<dyn std::error::Error>> {
    plugin(s, id)?["servers"]
        .as_array()
        .and_then(|v| v.iter().find(|x| x["key"] == server))
        .and_then(|x| x["generation"].as_u64())
        .ok_or_else(|| "generation".into())
}
/// Installs the Docs plugin, approves `tools` on `server` and enables it.
pub fn installed(
    s: &mut Store,
    server: &str,
    tools: &[Value],
) -> Result<u64, Box<dyn std::error::Error>> {
    let f = files();
    let id = s.install_plugin(&f, &review_plugin(&f)?.hash, 1_700_000_000)?;
    for raw in tools {
        let t = canonical_tool(raw)?;
        let hex = t
            .hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let r = revision(s, id)?;
        s.approve_plugin_tool(id, r, server, &t.definition, &hex)?;
    }
    s.set_plugin_enabled(id, true)?;
    Ok(id)
}
pub fn key(s: &Store, id: u64, server: &str) -> Result<PluginKeyRef, Box<dyn std::error::Error>> {
    Ok(PluginKeyRef {
        plugin: id,
        server: server.into(),
        generation: generation(s, id, server)?,
    })
}
pub fn high() -> ActualConfig {
    ActualConfig {
        model: ActualModel::Luna,
        effort: ActualEffort::High,
        tier: ActualTier::Fast,
    }
}
pub fn calls(names: &[(&str, Value)]) -> ModelResponse {
    ModelResponse {
        check: ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: high(),
            usage: CheckUsage::new(10, 2, 12).ok(),
        },
        answer: None,
        observation: None,
        calls: names
            .iter()
            .enumerate()
            .map(|(i, (n, args))| ToolCall {
                call_id: format!("call_{i}"),
                name: (*n).into(),
                arguments: args.as_object().cloned().unwrap_or_default(),
            })
            .collect(),
        forwarded: vec![
            json!({"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque"}),
        ],
    }
}
pub fn answered(task: TaskId, evidence: &[&str]) -> ModelResponse {
    ModelResponse {
        check: ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: high(),
            usage: None,
        },
        answer: Some(AssistanceAnswer {
            proposals: vec![ProposalAnswer {
                proposal_id: None,
                kind: AssistanceKind::Clarification,
                message: "Which document?".into(),
                rationale: "Search result".into(),
                evidence_ids: std::iter::once(format!("task:{}", task.0))
                    .chain(evidence.iter().map(|e| (*e).to_owned()))
                    .collect(),
                step_id: None,
            }],
            rationale: "Full set".into(),
        }),
        observation: None,
        calls: Vec::new(),
        forwarded: Vec::new(),
    }
}
pub fn listed(tools: &[Value]) -> Result<std::collections::BTreeMap<String, [u8; 32]>, StoreError> {
    tools
        .iter()
        .map(|t| Ok((t["name"].as_str().unwrap_or_default().to_owned(), hash(t)?)))
        .collect()
}
pub fn completed(text: &str, list: Option<&[Value]>) -> Result<ToolCallResult, StoreError> {
    Ok(ToolCallResult {
        outcome: PluginCallOutcome::Completed,
        text: text.into(),
        listed: list.map(listed).transpose()?,
    })
}
/// Installs a plugin whose one server `local` is a command with the given variable names,
/// approves `tools` and enables it.
pub fn installed_local(
    s: &mut Store,
    env: &[&str],
    tools: &[Value],
) -> Result<u64, Box<dyn std::error::Error>> {
    let env = env
        .iter()
        .map(|n| (n.to_string(), json!("package value is ignored")))
        .collect::<serde_json::Map<_, _>>();
    let mcp = json!({"mcpServers":{"local":{"command":"/usr/bin/python3","args":["/opt/server.py"],"env":env}}}).to_string();
    let mcp: &'static str = Box::leak(mcp.into_boxed_str());
    let f = vec![
        PluginFile {
            kind: PluginFileKind::Manifest,
            path: "plugin.json",
            bytes: br#"{"name":"Local"}"#,
        },
        PluginFile {
            kind: PluginFileKind::Mcp,
            path: "mcp.json",
            bytes: mcp.as_bytes(),
        },
    ];
    let id = s.install_plugin(&f, &review_plugin(&f)?.hash, 1_700_000_000)?;
    for raw in tools {
        let t = canonical_tool(raw)?;
        let hex = t
            .hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let r = revision(s, id)?;
        s.approve_plugin_tool(id, r, "local", &t.definition, &hex)?;
    }
    s.set_plugin_enabled(id, true)?;
    Ok(id)
}
