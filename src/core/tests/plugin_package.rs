use ambient_core::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-plugin-package-{}-{}.sqlite",
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

const SKILL: &str = "---\nname: search-docs\ndescription: Find a document by title\n---\nUse `search` first, then `read`.\n";
fn file<'a>(kind: PluginFileKind, path: &'a str, bytes: &'a str) -> PluginFile<'a> {
    PluginFile {
        kind,
        path,
        bytes: bytes.as_bytes(),
    }
}
fn portable(mcp: &str) -> Vec<(PluginFileKind, &'static str, String)> {
    vec![
        (
            PluginFileKind::Manifest,
            "plugin.json",
            r#"{"name":"Docs","version":"1.2.0","description":"x","extensions":{"com.openai":{"apps":"x"}}}"#.into(),
        ),
        (PluginFileKind::Mcp, "mcp.json", mcp.into()),
        (PluginFileKind::Skill, "skills/search/SKILL.md", SKILL.into()),
    ]
}
fn files<'a>(v: &'a [(PluginFileKind, &'static str, String)]) -> Vec<PluginFile<'a>> {
    v.iter().map(|(k, p, b)| file(*k, p, b)).collect()
}
const REMOTE: &str = r#"{"mcpServers":{"docs":{"type":"streamable-http","url":"https://mcp.example.com/mcp","headers":{"X-Key":"secret"}}}}"#;

#[test]
fn both_package_formats_review_to_servers_skills_and_an_order_independent_hash()
-> Result<(), StoreError> {
    let p = portable(REMOTE);
    let review = review_plugin(&files(&p))?;
    assert_eq!(review.name, "Docs");
    assert_eq!(review.version, "1.2.0");
    assert_eq!(review.hash.len(), 64);
    assert_eq!(
        review.servers,
        vec![PluginServerSpec {
            key: "docs".into(),
            kind: ServerKind::Remote {
                url: "https://mcp.example.com/mcp".into(),
                auth_required: true,
            },
        }]
    );
    assert_eq!(review.skills.len(), 1);
    assert_eq!(review.skills[0].name, "search-docs");
    assert_eq!(review.skills[0].description, "Find a document by title");
    assert_eq!(review.skills[0].text, SKILL);
    // Headers and OpenAI extensions are ignored, and the review says so.
    assert!(!review.warnings.is_empty());
    let mut reversed = files(&p);
    reversed.reverse();
    assert_eq!(review_plugin(&reversed)?.hash, review.hash);

    let codex = [
        file(
            PluginFileKind::Manifest,
            ".codex-plugin/plugin.json",
            r#"{"name":"Docs","version":"1.2.0","skills":"./skills/","mcpServers":"./.mcp.json","interface":{"displayName":"Docs"}}"#,
        ),
        file(
            PluginFileKind::Mcp,
            ".mcp.json",
            r#"{"mcpServers":{"docs":{"type":"http","url":"https://mcp.example.com/mcp","bearer_token_env_var":"DOCS_TOKEN"}}}"#,
        ),
        file(PluginFileKind::Skill, "skills/search/SKILL.md", SKILL),
    ];
    let c = review_plugin(&codex)?;
    assert_eq!(c.servers, review.servers);
    assert_ne!(c.hash, review.hash);
    let untyped = [
        file(
            PluginFileKind::Manifest,
            "plugin.json",
            r#"{"name":"Docs"}"#,
        ),
        file(
            PluginFileKind::Mcp,
            "mcp.json",
            r#"{"mcpServers":{"docs":{"url":"https://MCP.example.com:443/mcp"}}}"#,
        ),
    ];
    let u = review_plugin(&untyped)?;
    assert_eq!(u.version, "");
    assert_eq!(
        u.servers[0].kind,
        ServerKind::Remote {
            url: "https://mcp.example.com/mcp".into(),
            auth_required: false
        }
    );
    assert!(u.skills.is_empty());
    Ok(())
}

#[test]
fn only_https_remote_servers_with_valid_keys_are_kept() -> Result<(), StoreError> {
    for entry in [
        r#"{"type":"stdio","command":"./server.js"}"#,
        r#"{"command":"scripts/start"}"#,
        r#"{"type":"stdio","url":"https://mcp.example.com/mcp"}"#,
        r#"{"type":"sse","url":"https://mcp.example.com/sse"}"#,
        r#"{"type":"http"}"#,
        r#"{"url":"http://mcp.example.com/mcp"}"#,
        r#"{"url":"https://127.0.0.1/mcp"}"#,
        r#"{"url":"https://0x7f.1/mcp"}"#,
        r#"{"url":"https://[::1]/mcp"}"#,
        r#"{"url":"https://localhost/mcp"}"#,
        r#"{"url":"https://localhost./mcp"}"#,
        r#"{"url":"https://user:pass@mcp.example.com/mcp"}"#,
    ] {
        let mcp = format!(
            r#"{{"mcpServers":{{"bad":{entry},"docs":{{"url":"https://mcp.example.com/mcp"}}}}}}"#
        );
        let review = review_plugin(&files(&portable(&mcp)))?;
        assert_eq!(review.servers.len(), 1, "{entry}");
        assert_eq!(review.servers[0].key, "docs");
        assert!(review.warnings.iter().any(|w| w.contains("bad")), "{entry}");
        let only = format!(r#"{{"mcpServers":{{"bad":{entry}}}}}"#);
        assert_eq!(
            review_plugin(&files(&portable(&only))).err(),
            Some(StoreError::InvalidInput),
            "{entry}"
        );
    }
    for key in ["Docs", "docs_1", "", "a-very-long-server-key-over-32-bytes"] {
        let mcp =
            format!(r#"{{"mcpServers":{{"{key}":{{"url":"https://mcp.example.com/mcp"}}}}}}"#);
        assert_eq!(
            review_plugin(&files(&portable(&mcp))).err(),
            Some(StoreError::InvalidInput),
            "{key}"
        );
    }
    Ok(())
}

#[test]
fn package_and_skill_limits_reject_the_whole_package() {
    let servers = (0..5)
        .map(|i| format!(r#""s{i}":{{"url":"https://mcp{i}.example.com/mcp"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let five = format!(r#"{{"mcpServers":{{{servers}}}}}"#);
    assert_eq!(
        review_plugin(&files(&portable(&five))).err(),
        Some(StoreError::ResourceLimit)
    );
    let mut nine = portable(REMOTE);
    nine.pop();
    let paths = (0..9)
        .map(|i| Box::leak(format!("skills/s{i}/SKILL.md").into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();
    for p in &paths {
        nine.push((PluginFileKind::Skill, p, SKILL.into()));
    }
    assert_eq!(
        review_plugin(&files(&nine)).err(),
        Some(StoreError::ResourceLimit)
    );
    let mut large = portable(REMOTE);
    large[2].2 = format!("{SKILL}{}", "x".repeat(12 * 1024));
    assert_eq!(
        review_plugin(&files(&large)).err(),
        Some(StoreError::TextTooLong)
    );
    let mut total = portable(REMOTE);
    total.pop();
    for p in &paths[..5] {
        total.push((PluginFileKind::Skill, p, "y".repeat(10 * 1024)));
    }
    assert_eq!(
        review_plugin(&files(&total)).err(),
        Some(StoreError::ResourceLimit)
    );
    for (kind, path) in [
        (PluginFileKind::Skill, "skills/../SKILL.md"),
        (PluginFileKind::Skill, "other/SKILL.md"),
        (PluginFileKind::Mcp, "servers.json"),
        (PluginFileKind::Manifest, "manifest.json"),
    ] {
        let mut bad = portable(REMOTE);
        bad.push((kind, path, "{}".into()));
        assert_eq!(
            review_plugin(&files(&bad)).err(),
            Some(StoreError::InvalidInput),
            "{path}"
        );
    }
    let mut no_manifest = portable(REMOTE);
    no_manifest.remove(0);
    assert_eq!(
        review_plugin(&files(&no_manifest)).err(),
        Some(StoreError::InvalidInput)
    );
    let mut long_name = portable(REMOTE);
    long_name[0].2 = format!(r#"{{"name":"{}"}}"#, "n".repeat(257));
    assert_eq!(
        review_plugin(&files(&long_name)).err(),
        Some(StoreError::InvalidInput)
    );
}

#[test]
fn install_reparses_with_the_expected_hash_and_creates_a_disabled_plugin()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let p = portable(REMOTE);
    let review = review_plugin(&files(&p))?;
    let mut wrong = review.hash.clone();
    wrong.replace_range(0..1, if wrong.starts_with('0') { "1" } else { "0" });
    assert_eq!(
        s.install_plugin(&files(&p), &wrong, 1_700_000_000).err(),
        Some(StoreError::StaleRevision)
    );
    let id = s.install_plugin(&files(&p), &review.hash, 1_700_000_000)?;
    let list: serde_json::Value = serde_json::from_str(&s.plugins_json()?)?;
    assert_eq!(list["instance"].as_str().map(str::len), Some(32));
    let plugin = &list["plugins"][0];
    assert_eq!(plugin["id"], id);
    assert_eq!(plugin["enabled"], false);
    assert_eq!(plugin["name"], "Docs");
    assert_eq!(plugin["hash"], review.hash);
    assert_eq!(plugin["installed_at_utc"], 1_700_000_000);
    assert_eq!(plugin["servers"][0]["key"], "docs");
    assert_eq!(plugin["servers"][0]["kind"]["kind"], "remote");
    assert_eq!(plugin["servers"][0]["kind"]["auth_required"], true);
    assert_eq!(plugin["servers"][0]["needs_key"], true);
    assert!(
        plugin["servers"][0]["generation"]
            .as_u64()
            .is_some_and(|g| g > 0)
    );
    assert_eq!(plugin["skills"][0]["name"], "search-docs");
    assert_eq!(plugin["tools"], serde_json::json!([]));
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&s.plugins_json()?)?["plugins"][0]["id"],
        id
    );
    Ok(())
}

#[test]
fn at_most_sixteen_plugins_are_installed() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let p = portable(REMOTE);
    let hash = review_plugin(&files(&p))?.hash;
    for _ in 0..16 {
        s.install_plugin(&files(&p), &hash, 1)?;
    }
    assert_eq!(
        s.install_plugin(&files(&p), &hash, 1).err(),
        Some(StoreError::ResourceLimit)
    );
    Ok(())
}

fn local_review(entry: &str) -> Result<PluginReview, StoreError> {
    let mcp = format!(r#"{{"mcpServers":{{"local":{entry}}}}}"#);
    review_plugin(&files(&portable(&mcp)))
}

#[test]
fn a_command_server_is_local_with_names_only_and_honest_warnings() -> Result<(), StoreError> {
    let r = local_review(
        r#"{"type":"stdio","command":"npx","args":["-y","@acme/mcp","${HOME}/x","data/file.json"],"env":{"ZED_KEY":"ignored value","API_TOKEN":"also ignored"},"cwd":"."}"#,
    )?;
    assert_eq!(
        r.servers,
        vec![PluginServerSpec {
            key: "local".into(),
            kind: ServerKind::Local {
                command: "npx".into(),
                args: vec![
                    "-y".into(),
                    "@acme/mcp".into(),
                    "${HOME}/x".into(),
                    "data/file.json".into()
                ],
                env: vec!["API_TOKEN".into(), "ZED_KEY".into()],
            },
        }]
    );
    assert!(r.servers[0].kind.needs_key());
    assert!(r.servers[0].kind.downloads_code());
    for needle in ["cwd", "${", "relative"] {
        assert!(
            r.warnings.iter().any(|w| w.contains(needle)),
            "{needle}: {:?}",
            r.warnings
        );
    }
    // Package values never reach the stored package or the review.
    let json = r.json()?;
    assert!(!json.contains("ignored value") && !json.contains("also ignored"));
    let v: serde_json::Value = serde_json::from_str(&json).map_err(|_| StoreError::InvalidInput)?;
    assert_eq!(v["servers"][0]["kind"]["kind"], "local");
    assert_eq!(v["servers"][0]["needs_key"], true);
    assert_eq!(v["servers"][0]["downloads"], true);

    let plain = local_review(r#"{"command":"/usr/local/bin/mcp-time"}"#)?;
    assert!(!plain.servers[0].kind.needs_key());
    assert!(!plain.servers[0].kind.downloads_code());
    for (command, args, downloads) in [
        ("uvx", r#"["mcp-server-time"]"#, true),
        ("/opt/homebrew/bin/bunx", r#"["x"]"#, true),
        ("pnpm", r#"["dlx","x"]"#, true),
        ("yarn", r#"["dlx","x"]"#, true),
        ("pipx", r#"["run","x"]"#, true),
        ("pnpm", r#"["exec","x"]"#, false),
        ("node", r#"["/opt/server.js"]"#, false),
    ] {
        let r = local_review(&format!(r#"{{"command":"{command}","args":{args}}}"#))?;
        assert_eq!(r.servers[0].kind.downloads_code(), downloads, "{command}");
    }
    Ok(())
}

#[test]
fn local_limits_forbidden_names_and_url_with_command_reject_the_package() {
    let long = "a".repeat(1025);
    let many_args = serde_json::to_string(&vec!["a"; 33]).unwrap_or_default();
    let many_env = (0..17)
        .map(|i| format!(r#""V{i}":"x""#))
        .collect::<Vec<_>>()
        .join(",");
    for entry in [
        r#"{"command":"x","url":"https://mcp.example.com/mcp"}"#.to_owned(),
        r#"{"command":""}"#.into(),
        format!(r#"{{"command":"{long}"}}"#),
        format!(r#"{{"command":"x","args":["{long}"]}}"#),
        format!(r#"{{"command":"x","args":{many_args}}}"#),
        r#"{"command":"x\u0000y"}"#.into(),
        r#"{"command":"x","args":["a\u0000"]}"#.into(),
        format!(r#"{{"command":"x","env":{{{many_env}}}}}"#),
        r#"{"command":"x","env":{"lower":"x"}}"#.into(),
        r#"{"command":"x","env":{"1ABC":"x"}}"#.into(),
        format!(r#"{{"command":"x","env":{{"{}":"x"}}}}"#, "A".repeat(65)),
        r#"{"command":"x","env":{"PATH":"x"}}"#.into(),
        r#"{"command":"x","env":{"HOME":"x"}}"#.into(),
        r#"{"command":"x","env":{"TMPDIR":"x"}}"#.into(),
        r#"{"command":"x","env":{"DYLD_INSERT_LIBRARIES":"x"}}"#.into(),
        r#"{"command":"x","env":{"LD_PRELOAD":"x"}}"#.into(),
        r#"{"command":"x","args":"not a list"}"#.into(),
        r#"{"command":"x","env":["A"]}"#.into(),
    ] {
        assert_eq!(
            local_review(&entry).err(),
            Some(StoreError::InvalidInput),
            "{entry}"
        );
    }
    // Exactly at the limits is accepted.
    let args = serde_json::to_string(&vec!["a".repeat(1024); 32]).unwrap_or_default();
    let env = (0..16)
        .map(|i| format!(r#""V{i}":"x""#))
        .collect::<Vec<_>>()
        .join(",");
    let entry = format!(
        r#"{{"command":"{}","args":{args},"env":{{{env}}}}}"#,
        "c".repeat(1024)
    );
    assert!(local_review(&entry).is_ok());
}
