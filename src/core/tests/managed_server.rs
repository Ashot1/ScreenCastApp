//! The managed Google server (ADR-022 «Управляемый сервер Google», 24B): the built-in package, its
//! origin rule, the Core allowlist at approval, permit and discovery, and the start margin.
#[path = "support/assistance.rs"]
pub mod assistance_support;
#[path = "support/plugins.rs"]
pub mod plugin_support;
use ambient_core::*;
use assistance_support::*;
use plugin_support::*;
use rusqlite::{Connection, params};
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
            "ambient-managed-server-{}-{}.sqlite",
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
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const GOOGLE: ManagedServerId = ManagedServerId::GoogleWorkspace;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn install_google(s: &mut Store) -> Result<u64> {
    let files = GOOGLE.builtin_package();
    let review = review_plugin(files)?;
    Ok(s.install_plugin(files, &review.hash, 1_700_000_000)?)
}
fn approve(s: &mut Store, id: u64, raw: &Value) -> std::result::Result<u64, StoreError> {
    let t = canonical_tool(raw)?;
    let r = plugin(s, id).map_err(|_| StoreError::NotFound)?["revision"]
        .as_u64()
        .ok_or(StoreError::NotFound)?;
    s.approve_plugin_tool(id, r, "google", &t.definition, &hex(&t.hash))
}

#[test]
fn the_id_round_trips_and_the_built_in_bytes_match_their_hash() -> Result {
    assert_eq!(GOOGLE.as_str(), "google-workspace");
    assert_eq!(
        serde_json::to_string(&GOOGLE)?,
        format!("\"{}\"", GOOGLE.as_str())
    );
    assert_eq!(ManagedServerId::parse("google-workspace"), Some(GOOGLE));
    for other in ["", "Google-Workspace", "google-workspace ", "google"] {
        assert_eq!(ManagedServerId::parse(other), None, "{other:?}");
    }
    // A release whose built-in bytes and constant disagree fails loudly here.
    let review = review_plugin(GOOGLE.builtin_package())?;
    assert_eq!(review.hash, hex(&GOOGLE.builtin_package_hash()));
    assert_eq!(
        review.servers,
        vec![PluginServerSpec {
            key: "google".into(),
            kind: ServerKind::Managed { id: GOOGLE },
        }]
    );
    assert!(review.warnings.is_empty(), "{:?}", review.warnings);
    let kind = &review.servers[0].kind;
    assert!(kind.needs_key() && !kind.downloads_code());
    assert_eq!(kind.target(), Target::Managed(GOOGLE));
    let v: Value = serde_json::from_str(&review.json()?)?;
    assert_eq!(
        v["servers"][0]["kind"],
        json!({"kind":"managed","id":"google-workspace"})
    );
    assert_eq!(v["servers"][0]["needs_key"], true);
    Ok(())
}

#[test]
fn a_managed_entry_outside_the_built_in_package_is_skipped_with_a_warning() -> Result {
    let manifest = r#"{"name":"Not Google"}"#;
    for mcp in [
        r#"{"mcpServers":{"google":{"ambient_managed":"google-workspace"},"docs":{"url":"https://mcp.example.com/mcp"}}}"#,
        r#"{"mcpServers":{"google":{"ambient_managed":"unknown-server"},"docs":{"url":"https://mcp.example.com/mcp"}}}"#,
    ] {
        let files = [
            PluginFile {
                kind: PluginFileKind::Manifest,
                path: "plugin.json",
                bytes: manifest.as_bytes(),
            },
            PluginFile {
                kind: PluginFileKind::Mcp,
                path: "mcp.json",
                bytes: mcp.as_bytes(),
            },
        ];
        let review = review_plugin(&files)?;
        assert_eq!(review.servers.len(), 1, "{mcp}");
        assert_eq!(review.servers[0].key, "docs");
        assert!(
            review.warnings.iter().any(|w| w.contains("google")),
            "{:?}",
            review.warnings
        );
    }
    // Mixed with a URL or a command, the entry rejects the package like url+command does.
    for entry in [
        r#"{"ambient_managed":"google-workspace","url":"https://mcp.example.com/mcp"}"#,
        r#"{"ambient_managed":"google-workspace","command":"/usr/bin/true"}"#,
    ] {
        let mcp = format!(r#"{{"mcpServers":{{"google":{entry}}}}}"#);
        let files = [
            PluginFile {
                kind: PluginFileKind::Manifest,
                path: "plugin.json",
                bytes: manifest.as_bytes(),
            },
            PluginFile {
                kind: PluginFileKind::Mcp,
                path: "mcp.json",
                bytes: mcp.as_bytes(),
            },
        ];
        assert_eq!(
            review_plugin(&files).err(),
            Some(StoreError::InvalidInput),
            "{entry}"
        );
    }
    Ok(())
}

#[test]
fn an_installed_managed_plugin_still_opens_after_the_built_in_package_changes() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let id = install_google(&mut s)?;
    drop(s);
    // A later release ships other built-in bytes: the stored row keeps the old package hash, and
    // open-time validation checks the stored server structurally only.
    let c = Connection::open(&db.0)?;
    c.execute(
        "UPDATE plugins SET package_hash=?1 WHERE id=?2",
        params![vec![7u8; 32], id as i64],
    )?;
    drop(c);
    let s = Store::open(&db.0)?;
    assert_eq!(plugin(&s, id)?["servers"][0]["kind"]["kind"], "managed");
    Ok(())
}

#[test]
fn only_allowlisted_tools_are_approvable_and_discovery_marks_the_rest() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let id = install_google(&mut s)?;
    // Declared read-only but outside the allowlist (it writes a local file): never approved.
    let download = raw_tool("get_drive_file_download_url", "Download");
    assert_eq!(
        approve(&mut s, id, &download).err(),
        Some(StoreError::InvalidInput)
    );
    for name in [
        "search_drive_files",
        "get_drive_file_content",
        "search_docs",
        "get_doc_as_markdown",
    ] {
        approve(&mut s, id, &raw_tool(name, name))?;
    }
    let found = Discovery {
        listed: Ok(vec![
            raw_tool("search_docs", "Search"),
            download,
            json!({"name":"create_doc","annotations":{"readOnlyHint":false}}),
        ]),
        stderr: None,
        stop_confirmed: true,
    };
    let v: Value = serde_json::from_str(&discovery_json(&found, &Target::Managed(GOOGLE))?)?;
    let approvable = v["tools"]
        .as_array()
        .ok_or("tools")?
        .iter()
        .map(|t| (t["name"].clone(), t["approvable"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        approvable,
        vec![
            (json!("search_docs"), json!(true)),
            (json!("get_drive_file_download_url"), json!(false)),
            (json!("create_doc"), json!(false)),
        ]
    );
    assert!(v["tools"][1].get("definition").is_none());
    // A remote server is not limited by the managed allowlist.
    let remote = Target::Remote {
        url: "https://mcp.example.com/mcp".into(),
    };
    let v: Value = serde_json::from_str(&discovery_json(&found, &remote)?)?;
    assert_eq!(v["tools"][1]["approvable"], true);
    Ok(())
}

#[test]
fn a_pinned_name_outside_the_allowlist_is_never_sent_and_the_first_call_needs_start_time() -> Result
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let id = install_google(&mut s)?;
    let search = raw_tool("search_docs", "Search");
    approve(&mut s, id, &search)?;
    // An older release allowed this name and it was pinned then; the current list no longer does.
    let old = canonical_tool(&raw_tool("list_spreadsheets", "Old"))?;
    drop(s);
    let c = Connection::open(&db.0)?;
    c.execute(
        "INSERT INTO plugin_tools VALUES(?1,'google',?2,?3,?4)",
        params![id as i64, old.name, old.definition, old.hash.to_vec()],
    )?;
    drop(c);
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    s.set_plugin_enabled(id, true)?;
    // Without a Google token: «sign in», no network and no call-log row.
    let task = s.create_task("Find the launch plan")?;
    let revision = s.task(task.id)?.revision;
    let preview = prepare_new(&mut s, task.id, revision, &[])?;
    let a = s.begin_assistance(preview.request_id)?;
    let mut l = s.begin_model_loop(a, &[])?;
    let step = s.dispatch_model_step(&mut l, 10)?;
    let step_id = step.id;
    drop(step);
    s.finish_model_step(
        &mut l,
        step_id,
        calls(&[
            ("search_docs", json!({"query":"plan"})),
            ("list_spreadsheets", json!({})),
        ]),
    )?;
    assert!(s.permit_tool_call(&mut l, 20)?.is_none());
    let step = s.dispatch_model_step(&mut l, 30)?;
    let errors = step
        .history
        .iter()
        .filter_map(|h| match h {
            HistoryItem::Output { output, .. } => serde_json::from_str::<Value>(output).ok(),
            HistoryItem::Forwarded(_) => None,
        })
        .map(|v| v["error"].clone())
        .collect::<Vec<_>>();
    assert_eq!(errors, vec![json!("login_required"), json!("not_allowed")]);
    let step_id = step.id;
    drop(step);
    s.finish_model_step(&mut l, step_id, answered(task.id, &[]))?;
    s.release_check(&l.cancellation());
    assert_eq!(
        serde_json::from_str::<Value>(&s.plugin_calls_json()?)?,
        json!([])
    );
    // With a token the allowed tool gets a permit to the managed target; it starts the server.
    let k = key(&s, id, "google")?;
    if !s.assistance_available()? {
        ready(&mut s)?;
    }
    let revision = s.task(task.id)?.revision;
    let preview = prepare_new(&mut s, task.id, revision, &[])?;
    let a = s.begin_assistance(preview.request_id)?;
    let mut l = s.begin_model_loop(a, &[k])?;
    let step = s.dispatch_model_step(&mut l, 40)?;
    let step_id = step.id;
    drop(step);
    s.finish_model_step(
        &mut l,
        step_id,
        calls(&[("search_docs", json!({"query":"plan"}))]),
    )?;
    let permit = s.permit_tool_call(&mut l, 50)?.ok_or("permit")?;
    assert_eq!(permit.target, Target::Managed(GOOGLE));
    assert!(permit.list_first);
    s.accept_tool_result(
        &mut l,
        permit,
        completed("plan.doc", Some(std::slice::from_ref(&search)))?,
    )?;
    s.release_check(&l.cancellation());
    Ok(())
}
