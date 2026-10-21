use ambient_bridge::*;
use std::mem::{offset_of, size_of};
fn text(s: &str) -> AmbientText {
    AmbientText {
        bytes: s.as_ptr(),
        length: s.len() as u32,
    }
}
const MANIFEST: &str = r#"{"name":"Docs","version":"1.0.0"}"#;
const MCP: &str = r#"{"mcpServers":{"docs":{"url":"https://mcp.example.com/mcp"}}}"#;
fn files() -> [AmbientPluginFile; 3] {
    [
        AmbientPluginFile {
            kind: 1,
            reserved: 0,
            path: text("plugin.json"),
            bytes: text(MANIFEST),
        },
        AmbientPluginFile {
            kind: 2,
            reserved: 0,
            path: text(".mcp.json"),
            bytes: text(MCP),
        },
        // An empty file crosses as {NULL, 0}; Core decides it is not a valid skill.
        AmbientPluginFile {
            kind: 3,
            reserved: 0,
            path: text("skills/empty/SKILL.md"),
            bytes: AmbientText {
                bytes: std::ptr::null(),
                length: 0,
            },
        },
    ]
}
fn json(out: &[u8], used: u32) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_slice(&out[..used as usize])
}

#[test]
fn layouts_review_install_and_capacity_errors_keep_outputs()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(size_of::<AmbientPluginFile>(), 40);
    assert_eq!(offset_of!(AmbientPluginFile, path), 8);
    assert_eq!(offset_of!(AmbientPluginFile, bytes), 24);
    assert_eq!(size_of::<AmbientPluginKey>(), 48);
    assert_eq!(offset_of!(AmbientPluginKey, server), 8);
    assert_eq!(offset_of!(AmbientPluginKey, generation), 24);
    assert_eq!(offset_of!(AmbientPluginKey, key), 32);
    let path =
        std::env::temp_dir().join(format!("ambient-plugin-abi-{}.sqlite3", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    assert_eq!(
        unsafe { ambient_v3_open(p.as_ptr(), p.len() as u32, &mut h) },
        0
    );
    let f = files();
    let mut out = vec![0u8; 256 * 1024];
    let mut used = 7u32;
    // Small capacity and a nonzero reserved field fail before any work and keep outputs.
    assert_eq!(
        unsafe { ambient_plugin_v2_review(f.as_ptr(), 3, out.as_mut_ptr(), 1024, &mut used) },
        6
    );
    let mut bad = files();
    bad[0].reserved = 1;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(
                bad.as_ptr(),
                3,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        1
    );
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(f.as_ptr(), 0, out.as_mut_ptr(), out.len() as u32, &mut used)
        },
        1
    );
    assert_eq!(used, 7);
    // The empty skill file is read but is no valid skill name source: the review still works.
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(f.as_ptr(), 2, out.as_mut_ptr(), out.len() as u32, &mut used)
        },
        0
    );
    let review = json(&out, used)?;
    assert_eq!(review["servers"][0]["key"], "docs");
    let hash = review["hash"].as_str().ok_or("hash")?.to_owned();
    let mut id = 0u64;
    assert_eq!(
        unsafe { ambient_plugin_v2_install(h, f.as_ptr(), 2, text(&"0".repeat(64)), 1, &mut id) },
        11
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_install(h, f.as_ptr(), 2, text(&hash), 1, &mut id) },
        0
    );
    assert!(id > 0);
    let mut list = vec![0u8; 1024 * 1024];
    assert_eq!(
        unsafe { ambient_plugin_v2_list(h, list.as_mut_ptr(), 1024, &mut used) },
        6
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_list(h, list.as_mut_ptr(), list.len() as u32, &mut used) },
        0
    );
    let listed = json(&list, used)?;
    let plugin = &listed["plugins"][0];
    let revision = plugin["revision"].as_u64().ok_or("revision")?;
    let generation = plugin["servers"][0]["generation"]
        .as_u64()
        .ok_or("generation")?;
    let mut calls = vec![0u8; 768 * 1024];
    assert_eq!(
        unsafe { ambient_plugin_v2_calls(h, calls.as_mut_ptr(), calls.len() as u32, &mut used) },
        0
    );
    assert_eq!(json(&calls, used)?, serde_json::json!([]));
    // Lifecycle statuses follow ambient_source.h.
    let mut r = 0u64;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_unapprove(h, id, revision, text("docs"), text("search"), &mut r)
        },
        14
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_revoke(h, id, revision + 1, text("docs"), &mut r) },
        11
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_revoke(h, id, revision, text("docs"), &mut r) },
        0
    );
    assert!(r > generation);
    assert_eq!(ambient_plugin_v2_set_enabled(h, id, 2), 1);
    assert_eq!(ambient_plugin_v2_set_enabled(h, id, 1), 0);
    let mut removal = 0u64;
    assert_eq!(
        unsafe { ambient_plugin_v2_begin_removal(h, id, r, &mut removal) },
        0
    );
    assert_eq!(ambient_plugin_v2_finish_removal(h, id, removal + 1), 11);
    assert_eq!(ambient_plugin_v2_finish_removal(h, id, removal), 0);
    assert_eq!(ambient_plugin_v2_finish_removal(h, id, removal), 14);
    // Discovery of a removed plugin fails before any network use.
    let mut d = vec![0u8; 512 * 1024];
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    let mut op = 0;
    assert_eq!(unsafe { ambient_plugin_v2_operation_begin(&mut op) }, 0);
    let discover = |op, root: AmbientText, d: &mut [u8], used: &mut u32| unsafe {
        ambient_plugin_v2_discover(
            h,
            op,
            root,
            id,
            text("docs"),
            removal,
            none,
            d.as_mut_ptr(),
            d.len() as u32,
            used,
        )
    };
    assert_eq!(
        discover(op, text("/tmp/ambient-plugins"), &mut d, &mut used),
        14
    );
    // The root must be an absolute path without NUL, 1..1024 bytes; the handle must exist.
    for root in ["relative/root", "/with\0nul", ""] {
        assert_eq!(discover(op, text(root), &mut d, &mut used), 1, "{root:?}");
    }
    let long = format!("/{}", "r".repeat(1024));
    assert_eq!(discover(op, text(&long), &mut d, &mut used), 1);
    assert_eq!(discover(u64::MAX, text("/tmp/r"), &mut d, &mut used), 7);
    assert_eq!(ambient_plugin_v2_operation_cancel(op), 0);
    assert_eq!(ambient_plugin_v2_operation_close(op), 0);
    assert_eq!(ambient_plugin_v2_operation_close(op), 7);
    assert_eq!(ambient_plugin_v2_operation_cancel(op), 7);
    assert_eq!(ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn plugin_keys_are_validated_before_any_core_use() -> Result<(), Box<dyn std::error::Error>> {
    let key = |plugin, server: &'static str, generation, secret: &'static str| AmbientPluginKey {
        plugin,
        server: text(server),
        generation,
        key: text(secret),
    };
    let token = "test-token";
    let mut out = AmbientModelCompletion::default();
    for keys in [
        vec![key(0, "docs", 1, "k")],
        vec![key(1, "docs", 0, "k")],
        vec![key(1, "Docs", 1, "k")],
        vec![key(1, "docs", 1, "with space")],
        vec![key(1, "docs", 1, "a"), key(1, "docs", 2, "b")],
        (0..65).map(|_| key(1, "docs", 1, "k")).collect(),
    ] {
        // An unknown operation handle would be 7; invalid keys are refused first with 1.
        assert_eq!(
            unsafe {
                ambient_model_v6_dispatch(
                    u64::MAX,
                    token.as_ptr(),
                    token.len() as u32,
                    keys.as_ptr(),
                    keys.len() as u32,
                    text("/tmp/ambient-plugins"),
                    &mut out,
                )
            },
            1
        );
    }
    let fine = [key(1, "docs", 1, "k")];
    assert_eq!(
        unsafe {
            ambient_model_v6_dispatch(
                u64::MAX,
                token.as_ptr(),
                token.len() as u32,
                fine.as_ptr(),
                1,
                text("/tmp/ambient-plugins"),
                &mut out,
            )
        },
        7
    );
    // The root folder is checked before Core too.
    for root in ["relative", "/a\0b", ""] {
        assert_eq!(
            unsafe {
                ambient_model_v6_dispatch(
                    u64::MAX,
                    token.as_ptr(),
                    token.len() as u32,
                    fine.as_ptr(),
                    1,
                    text(root),
                    &mut out,
                )
            },
            1,
            "{root:?}"
        );
    }
    Ok(())
}

#[test]
fn the_built_in_google_package_reviews_and_installs_as_a_managed_server()
-> Result<(), Box<dyn std::error::Error>> {
    let mut out = vec![0u8; 262_144];
    let mut used = 0;
    let id = text("google-workspace");
    assert_eq!(
        unsafe {
            ambient_plugin_v2_builtin(
                text("google"),
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        1
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_builtin(id, out.as_mut_ptr(), 1024, &mut used) },
        6
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_builtin(id, out.as_mut_ptr(), out.len() as u32, &mut used) },
        0
    );
    let built = json(&out, used)?;
    let files = built["files"].as_array().ok_or("files")?;
    let texts = files
        .iter()
        .map(|f| f["text"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    let paths = files
        .iter()
        .map(|f| f["path"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    let raw = files
        .iter()
        .zip(paths.iter().zip(&texts))
        .map(|(f, (p, t))| AmbientPluginFile {
            kind: f["kind"].as_u64().unwrap_or_default() as u32,
            reserved: 0,
            path: text(p),
            bytes: text(t),
        })
        .collect::<Vec<_>>();
    // The bytes native passes back review exactly like any folder: same hash, a managed server.
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(
                raw.as_ptr(),
                raw.len() as u32,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let review = json(&out, used)?;
    assert_eq!(
        review["servers"][0]["kind"],
        serde_json::json!({"kind":"managed","id":"google-workspace"})
    );
    assert_eq!(review["servers"][0]["needs_key"], true);
    let path = std::env::temp_dir().join(format!(
        "ambient-plugin-builtin-{}.sqlite3",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    assert_eq!(
        unsafe { ambient_v3_open(p.as_ptr(), p.len() as u32, &mut h) },
        0
    );
    let hash = review["hash"].as_str().ok_or("hash")?.to_owned();
    let mut plugin = 0;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_install(
                h,
                raw.as_ptr(),
                raw.len() as u32,
                text(&hash),
                1,
                &mut plugin,
            )
        },
        0
    );
    assert_eq!(ambient_v3_close(h), 0);
    let _ = std::fs::remove_file(&path);
    Ok(())
}

#[test]
fn managed_status_and_install_refuse_bad_input_and_a_cancelled_install_runs_nothing()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("ambient-managed-abi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let r = root.to_str().ok_or("root")?.to_owned();
    let id = text("google-workspace");
    let mut out = vec![0u8; 65_536];
    let mut used = 0;
    for (root_text, id_text) in [(text("relative"), id), (text(&r), text("unknown"))] {
        assert_eq!(
            unsafe {
                ambient_plugin_v2_managed_status(
                    root_text,
                    id_text,
                    out.as_mut_ptr(),
                    1024,
                    &mut used,
                )
            },
            1
        );
    }
    assert_eq!(
        unsafe {
            ambient_plugin_v2_managed_status(text(&r), id, out.as_mut_ptr(), 1024, &mut used)
        },
        0
    );
    // Whether this machine has uv decides between the two; nothing is installed either way.
    let status = json(&out, used)?["status"].clone();
    assert!(
        status == "not_installed" || status == "uv_missing",
        "{status}"
    );
    let mut op = 0;
    assert_eq!(unsafe { ambient_plugin_v2_operation_begin(&mut op) }, 0);
    assert_eq!(
        unsafe {
            ambient_plugin_v2_managed_install(
                op,
                text(&r),
                text("unknown"),
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        1
    );
    assert_eq!(ambient_plugin_v2_operation_cancel(op), 0);
    assert_eq!(
        unsafe {
            ambient_plugin_v2_managed_install(
                op,
                text(&r),
                id,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let answer = json(&out, used)?;
    assert!(
        answer["status"] == "cancelled" || answer["status"] == "uv_missing",
        "{answer}"
    );
    assert!(!root.join("google-workspace-mcp").exists());
    assert_eq!(ambient_plugin_v2_operation_close(op), 0);
    Ok(())
}

#[test]
fn oauth_calls_validate_their_input_before_any_network_use()
-> Result<(), Box<dyn std::error::Error>> {
    let id = text("google-workspace");
    let mut out = vec![0u8; 65_536];
    let mut used = 0;
    for redirect in [
        "http://localhost:1234/",
        "http://127.0.0.1:1234/auth/callback",
    ] {
        assert_eq!(
            unsafe {
                ambient_plugin_v2_oauth_begin(
                    id,
                    text(redirect),
                    out.as_mut_ptr(),
                    out.len() as u32,
                    &mut used,
                )
            },
            1
        );
    }
    assert_eq!(
        unsafe {
            ambient_plugin_v2_oauth_begin(
                id,
                text("http://127.0.0.1:1234/"),
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    // A build without the built-in client says so; with it, the URL goes to Google.
    let begin = json(&out, used)?;
    assert!(
        begin["status"] == "unavailable"
            || begin["url"]
                .as_str()
                .is_some_and(|u| u.starts_with("https://accounts.google.com/")),
        "{begin}"
    );
    // A space or an empty state never reaches the exchange.
    for (query, state) in [("code=x y", "s"), ("code=x", "")] {
        assert_eq!(
            unsafe {
                ambient_plugin_v2_oauth_exchange(
                    id,
                    text("http://127.0.0.1:1234/"),
                    text(query),
                    text(state),
                    text("v"),
                    out.as_mut_ptr(),
                    out.len() as u32,
                    &mut used,
                )
            },
            1
        );
    }
    // A wrong state is refused by the check itself, without a token request.
    assert_eq!(
        unsafe {
            ambient_plugin_v2_oauth_exchange(
                id,
                text("http://127.0.0.1:1234/"),
                text("state=other&code=x&iss=https%3A%2F%2Faccounts.google.com"),
                text("mine"),
                text("v"),
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let status = json(&out, used)?["status"].clone();
    assert!(status == "invalid" || status == "failed", "{status}");
    assert_eq!(
        unsafe {
            ambient_plugin_v2_oauth_refresh(
                id,
                text("with space"),
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        1
    );
    for (id_text, token) in [(id, "with space"), (text("unknown"), "1//r")] {
        assert_eq!(
            unsafe {
                ambient_plugin_v2_oauth_revoke(
                    id_text,
                    text(token),
                    out.as_mut_ptr(),
                    out.len() as u32,
                    &mut used,
                )
            },
            1
        );
    }
    Ok(())
}
