use ambient_core::*;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-plugin-lifecycle-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
const MANIFEST: &str = r#"{"name":"Docs","version":"1.0.0"}"#;
const MCP: &str = r#"{"mcpServers":{"docs":{"url":"https://mcp.example.com/mcp","headers":{"Authorization":"x"}},"open":{"url":"https://open.example.com/mcp"}}}"#;
fn install(s: &mut Store) -> Result<u64, StoreError> {
    let files = [
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
    ];
    let hash = review_plugin(&files)?.hash;
    s.install_plugin(&files, &hash, 1_700_000_000)
}
fn listed(s: &Store) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str::<Value>(&s.plugins_json()?)?["plugins"][0].clone())
}
fn revision(s: &Store) -> Result<u64, Box<dyn std::error::Error>> {
    listed(s)?["revision"]
        .as_u64()
        .ok_or_else(|| "revision".into())
}
fn tool(name: &str, read_only: Option<bool>, description: &str) -> Value {
    let mut t = json!({"inputSchema":{"type":"object","properties":{"q":{"type":"string"}}},"name":name,"description":description,"title":"ignored","outputSchema":{"type":"object"}});
    if let Some(r) = read_only {
        t["annotations"] = json!({"readOnlyHint": r, "destructiveHint": false});
    }
    t
}
fn discovery(listed: Result<Vec<Value>, PluginCallOutcome>) -> Discovery {
    Discovery {
        listed,
        stderr: None,
        stop_confirmed: true,
    }
}
fn discovered(raw: &[Value]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let found: Value =
        serde_json::from_str(&discovery_json(&discovery(Ok(raw.to_vec())), &remote())?)?;
    assert_eq!(found["status"], "ok");
    Ok(serde_json::from_value(found["tools"].clone())?)
}

/// Discovery of an ordinary remote server: no managed allowlist applies.
fn remote() -> Target {
    Target::Remote {
        url: "https://mcp.example.com/mcp".into(),
    }
}

#[test]
fn discovery_pins_the_canonical_definition_and_only_declared_read_only_tools_are_approvable()
-> Result<(), Box<dyn std::error::Error>> {
    let reordered: Value = serde_json::from_str(
        r#"{"description":"Search","annotations":{"readOnlyHint":true},"name":"search","inputSchema":{"properties":{"q":{"type":"string"}},"type":"object"}}"#,
    )?;
    let tools = discovered(&[
        tool("search", Some(true), "Search"),
        reordered,
        tool("write", Some(false), "Write"),
        tool("plain", None, "No hint"),
        tool("huge", Some(true), &"d".repeat(MAX_TOOL_DEFINITION)),
    ])?;
    assert_eq!(tools[0]["approvable"], true);
    assert_eq!(tools[0]["hash"], tools[1]["hash"]);
    assert_eq!(
        tools[0]["definition"],
        r#"{"description":"Search","inputSchema":{"properties":{"q":{"type":"string"}},"type":"object"},"name":"search","readOnlyHint":true}"#
    );
    for (i, read_only) in [(2, false), (3, false), (4, true)] {
        assert_eq!(tools[i]["approvable"], false, "{i}");
        assert_eq!(tools[i]["read_only"], read_only, "{i}");
        assert!(tools[i].get("definition").is_none(), "{i}");
    }
    let no_hint = discovered(&[tool("plain", None, "No hint")])?;
    assert_ne!(no_hint[0]["hash"], Value::Null);
    let many = (0..=MAX_LISTED_TOOLS)
        .map(|i| tool(&format!("t{i}"), Some(true), "x"))
        .collect::<Vec<_>>();
    assert_eq!(
        discovery_json(&discovery(Ok(many)), &remote()).err(),
        Some(StoreError::ResourceLimit)
    );
    Ok(())
}

#[test]
fn a_failed_local_discovery_names_the_status_and_carries_its_tail_and_an_unconfirmed_stop()
-> Result<(), Box<dyn std::error::Error>> {
    for (outcome, status) in [
        (PluginCallOutcome::NotConfigured, "not_configured"),
        (PluginCallOutcome::Cancelled, "cancelled"),
        (PluginCallOutcome::Timeout, "timeout"),
        (PluginCallOutcome::Unavailable, "unavailable"),
        (PluginCallOutcome::Invalid, "invalid"),
    ] {
        let d = Discovery {
            listed: Err(outcome),
            stderr: Some("npm ERR! [redacted]".into()),
            stop_confirmed: false,
        };
        let v: Value = serde_json::from_str(&discovery_json(&d, &remote())?)?;
        assert_eq!(v["status"], status);
        assert_eq!(v["stderr"], "npm ERR! [redacted]");
        assert_eq!(v["stop_confirmed"], false);
    }
    let ok: Value = serde_json::from_str(&discovery_json(&discovery(Ok(vec![])), &remote())?)?;
    assert!(ok.get("stderr").is_none() && ok.get("stop_confirmed").is_none());
    Ok(())
}

#[test]
fn approval_recomputes_the_hash_and_pin_changes_bump_the_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let id = install(&mut s)?;
    let r0 = revision(&s)?;
    let tools = discovered(&[
        tool("search", Some(true), "Search"),
        tool("write", Some(false), "Write"),
    ])?;
    let def = tools[0]["definition"].as_str().ok_or("definition")?;
    let hash = tools[0]["hash"].as_str().ok_or("hash")?;
    let wrong = format!("{}0", &hash[..63]);
    assert_eq!(
        s.approve_plugin_tool(
            id,
            r0,
            "docs",
            def,
            if wrong == hash { "f" } else { &wrong }
        )
        .err(),
        Some(StoreError::InvalidInput)
    );
    let write = r#"{"description":"Write","name":"write","readOnlyHint":false}"#;
    let write_hash = discovered(&[
        json!({"name":"write","description":"Write","annotations":{"readOnlyHint":false}}),
    ])?[0]["hash"]
        .as_str()
        .ok_or("hash")?
        .to_owned();
    assert_eq!(
        s.approve_plugin_tool(id, r0, "docs", write, &write_hash)
            .err(),
        Some(StoreError::InvalidInput)
    );
    let spaced = def.replace(":", ": ");
    assert_eq!(
        s.approve_plugin_tool(id, r0, "docs", &spaced, hash).err(),
        Some(StoreError::InvalidInput)
    );
    assert_eq!(
        s.approve_plugin_tool(id, r0, "missing", def, hash).err(),
        Some(StoreError::NotFound)
    );
    assert_eq!(
        s.approve_plugin_tool(id, r0 + 1, "docs", def, hash).err(),
        Some(StoreError::StaleRevision)
    );
    // A new pin keeps the revision; the identical pin is a no-op.
    assert_eq!(s.approve_plugin_tool(id, r0, "docs", def, hash)?, r0);
    assert_eq!(s.approve_plugin_tool(id, r0, "docs", def, hash)?, r0);
    assert_eq!(
        listed(&s)?["tools"],
        json!([{"server":"docs","name":"search","hash":hash}])
    );
    // Re-pinning a changed definition and removing an approval bump the revision.
    let changed = discovered(&[tool("search", Some(true), "Search v2")])?;
    let r1 = s.approve_plugin_tool(
        id,
        r0,
        "docs",
        changed[0]["definition"].as_str().ok_or("definition")?,
        changed[0]["hash"].as_str().ok_or("hash")?,
    )?;
    assert!(r1 > r0);
    assert_eq!(
        s.unapprove_plugin_tool(id, r0, "docs", "search").err(),
        Some(StoreError::StaleRevision)
    );
    let r2 = s.unapprove_plugin_tool(id, r1, "docs", "search")?;
    assert!(r2 > r1);
    assert_eq!(listed(&s)?["tools"], json!([]));
    assert_eq!(
        s.unapprove_plugin_tool(id, r2, "docs", "search").err(),
        Some(StoreError::NotFound)
    );
    Ok(())
}

#[test]
fn enable_revoke_and_removal_follow_the_revision_and_generation_rules()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let id = install(&mut s)?;
    let r0 = revision(&s)?;
    s.set_plugin_enabled(id, false)?;
    assert_eq!(revision(&s)?, r0, "same value changes nothing");
    s.set_plugin_enabled(id, true)?;
    assert_eq!(listed(&s)?["enabled"], true);
    assert_eq!(revision(&s)?, r0, "enabling keeps the revision");
    s.set_plugin_enabled(id, false)?;
    let r1 = revision(&s)?;
    assert!(r1 > r0, "disabling bumps the revision");
    s.set_plugin_enabled(id, true)?;
    assert_eq!(
        s.revoke_plugin_credential(id, r0, "docs").err(),
        Some(StoreError::StaleRevision)
    );
    let r2 = s.revoke_plugin_credential(id, r1, "docs")?;
    let p = listed(&s)?;
    assert_eq!(p["revision"], r2);
    let docs = p["servers"]
        .as_array()
        .and_then(|v| v.iter().find(|s| s["key"] == "docs"))
        .ok_or("docs")?;
    assert_eq!(docs["generation"], r2);
    let r3 = s.begin_plugin_removal(id, r2)?;
    let p = listed(&s)?;
    assert_eq!(p["enabled"], false);
    assert!(
        p["servers"]
            .as_array()
            .ok_or("servers")?
            .iter()
            .all(|s| s["generation"] == r3)
    );
    // Re-enabling after native removed the keys keeps the plugin: finish refuses.
    s.set_plugin_enabled(id, true)?;
    assert_eq!(
        s.finish_plugin_removal(id, r3).err(),
        Some(StoreError::StaleRevision)
    );
    s.set_plugin_enabled(id, false)?;
    let r4 = s.begin_plugin_removal(id, revision(&s)?)?;
    s.finish_plugin_removal(id, r4)?;
    assert_eq!(
        serde_json::from_str::<Value>(&s.plugins_json()?)?["plugins"],
        json!([])
    );
    assert_eq!(
        s.finish_plugin_removal(id, r4).err(),
        Some(StoreError::NotFound)
    );
    // Plugin IDs come from the shared clock and are never reused.
    assert!(install(&mut s)? > id);
    Ok(())
}

#[test]
fn the_plugin_namespace_is_reserved_for_saved_tool_results() -> Result<(), StoreError> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let task = s.create_task("task")?;
    let key = SourceKey {
        namespace: PLUGIN_NAMESPACE.into(),
        scope: "1".into(),
        object_id: "docs/search:00".into(),
    };
    let input = SourceEvidence {
        text: "text".into(),
        observed_at_utc: None,
        completeness: Completeness::Complete,
        kind: AssertionKind::SourceFact,
    };
    assert_eq!(
        s.bind_source(task.id, task.revision, &key, None, &input, 1)
            .err(),
        Some(StoreError::InvalidInput)
    );
    Ok(())
}

#[test]
fn schemas_thirteen_through_fifteen_are_rejected_and_tampered_pins_or_interrupted_calls_are_handled_at_open()
-> Result<(), Box<dyn std::error::Error>> {
    for (fixture, version) in [
        (include_str!("fixtures/schema-thirteen.sql"), 13),
        (include_str!("fixtures/schema-fourteen.sql"), 14),
        (include_str!("fixtures/schema-fifteen.sql"), 15),
    ] {
        let old = Database::new();
        let c = Connection::open(&old.0)?;
        c.execute_batch(fixture)?;
        c.execute_batch(&format!("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); INSERT INTO store_identity VALUES(1,randomblob(16)); INSERT INTO integration_switches VALUES('local-projects',1),('codex',1),('github',1),('jira',1); PRAGMA application_id=1095582257; PRAGMA user_version={version};"))?;
        drop(c);
        let before = std::fs::read(&old.0)?;
        assert_eq!(
            Store::open(&old.0).err(),
            Some(StoreError::UnsupportedSchema),
            "{version}"
        );
        assert_eq!(std::fs::read(&old.0)?, before, "{version}");
    }

    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let id = install(&mut s)?;
    let tools = discovered(&[tool("search", Some(true), "Search")])?;
    let r = revision(&s)?;
    s.approve_plugin_tool(
        id,
        r,
        "docs",
        tools[0]["definition"].as_str().ok_or("definition")?,
        tools[0]["hash"].as_str().ok_or("hash")?,
    )?;
    drop(s);
    let c = Connection::open(&db.0)?;
    c.execute(
        "INSERT INTO plugin_calls VALUES(1,5,?1,'Docs','docs','search','{}',1,0,0)",
        [id as i64],
    )?;
    drop(c);
    let s = Store::open(&db.0)?;
    let calls: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(calls[0]["stage"], 2);
    assert_eq!(calls[0]["outcome"], PluginCallOutcome::Interrupted as u32);
    drop(s);
    let c = Connection::open(&db.0)?;
    c.execute(
        "UPDATE plugin_tools SET definition=replace(definition,'Search','Other')",
        [],
    )?;
    drop(c);
    assert_eq!(
        Store::open(&db.0).err(),
        Some(StoreError::UnsupportedSchema)
    );
    Ok(())
}

#[test]
fn a_stored_package_without_the_kind_tag_or_with_an_invalid_local_server_is_unsupported()
-> Result<(), Box<dyn std::error::Error>> {
    let open = r#"{"key":"open","kind":{"kind":"remote","url":"https://open.example.com/mcp","auth_required":false}}"#;
    for (docs, opens) in [
        (
            r#"{"key":"docs","kind":{"kind":"local","command":"npx","args":["-y","x"],"env":["A"]}}"#,
            true,
        ),
        // The 24A flat shape: no compatibility layer.
        (
            r#"{"key":"docs","url":"https://mcp.example.com/mcp","auth_required":true}"#,
            false,
        ),
        (
            r#"{"key":"docs","kind":{"kind":"local","command":"./server.js","args":[],"env":[]}}"#,
            false,
        ),
        (
            r#"{"key":"docs","kind":{"kind":"local","command":"node","args":[],"env":["PATH"]}}"#,
            false,
        ),
        (
            r#"{"key":"docs","kind":{"kind":"local","command":"node","args":[],"env":["B","A"]}}"#,
            false,
        ),
        (
            r#"{"key":"docs","kind":{"kind":"local","command":"node","args":[],"env":[],"cwd":"."}}"#,
            false,
        ),
    ] {
        let db = Database::new();
        let mut s = Store::open(&db.0)?;
        install(&mut s)?;
        drop(s);
        let c = Connection::open(&db.0)?;
        let stored = format!(r#"{{"servers":[{docs},{open}],"skills":[]}}"#);
        c.execute("UPDATE plugins SET package=?1", [&stored])?;
        drop(c);
        let result = Store::open(&db.0).map(|_| ()).err();
        assert_eq!(
            result,
            if opens {
                None
            } else {
                Some(StoreError::UnsupportedSchema)
            },
            "{docs}"
        );
    }
    Ok(())
}
