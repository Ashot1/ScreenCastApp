//! Loopback C ABI flow: scripted SSE model server and MCP fixture, test-support builds only.
use super::*;
use model::support::ambient_model_test_dispatch;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
type Handler = Box<dyn Fn(usize, &serde_json::Value) -> (String, String, Vec<u8>) + Send>;
struct Server {
    url: String,
    seen: Seen,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn serve(path: &str, handler: Handler) -> Result<Server, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}{path}", listener.local_addr()?);
    let seen: Seen = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, st) = (seen.clone(), stop.clone());
    let worker = thread::spawn(move || {
        let mut n = 0;
        while !st.load(Ordering::Acquire) {
            let Ok((mut s, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            // Accepted sockets inherit the listener's non-blocking mode on macOS.
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let (mut input, mut buf) = (Vec::new(), [0u8; 8192]);
            let (head, body) = loop {
                let Ok(r) = s.read(&mut buf) else {
                    break (String::new(), Vec::new());
                };
                if r == 0 {
                    break (String::new(), Vec::new());
                }
                input.extend_from_slice(&buf[..r]);
                if let Some(p) = input.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&input[..p]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if input.len() >= p + 4 + len {
                        break (head, input[p + 4..p + 4 + len].to_vec());
                    }
                }
            };
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            if let Ok(mut seen) = s2.lock() {
                seen.push((head, body));
            }
            let (status, headers, out) = handler(n, &v);
            n += 1;
            let _ = write!(
                s,
                "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                out.len()
            );
            let _ = s.write_all(&out);
        }
    });
    Ok(Server {
        url,
        seen,
        stop,
        worker: Some(worker),
    })
}
fn event(v: serde_json::Value) -> String {
    format!("data: {v}\n\n")
}
fn completed(output: serde_json::Value, items: &[serde_json::Value]) -> String {
    let mut s = String::new();
    for (i, item) in items.iter().enumerate() {
        s.push_str(&event(
            serde_json::json!({"type":"response.output_item.done","output_index":i,"item":item}),
        ));
    }
    s.push_str(&event(serde_json::json!({"type":"response.completed","response":{"status":"completed","model":"gpt-6-luna","reasoning":{"effort":"high"},"service_tier":"priority","output":output,"usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}})));
    s
}
fn call_response(i: usize) -> String {
    call_named(i, "search")
}
fn call_named(i: usize, name: &str) -> String {
    let items = [
        serde_json::json!({"id":format!("rs_{i}"),"type":"reasoning","summary":[],"encrypted_content":"opaque"}),
        serde_json::json!({"id":format!("fc_{i}"),"type":"function_call","status":"completed","call_id":format!("call_{i}"),"name":name,"arguments":format!("{{\"q\":\"plan {i}\"}}")}),
    ];
    completed(serde_json::json!(items), &items)
}
fn answer_response(evidence: &[&str]) -> String {
    let text = serde_json::json!({"proposals":[{"proposal_id":null,"kind":"clarification","message":"Which plan?","rationale":"Found in the plugin","evidence_ids":evidence,"step_id":null}],"rationale":"Full set"}).to_string();
    let item = serde_json::json!({"id":"msg","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]});
    completed(serde_json::json!([item.clone()]), &[item])
}
fn sse(body: String) -> (String, String, Vec<u8>) {
    (
        "200 OK".into(),
        "Content-Type: text/event-stream\r\n".into(),
        body.into_bytes(),
    )
}
fn mcp() -> Result<Server, std::io::Error> {
    serve(
        "/mcp",
        Box::new(|_, m| {
            let reply = |r: serde_json::Value| {
                (
                    "200 OK".to_owned(),
                    "Content-Type: application/json\r\n".to_owned(),
                    serde_json::json!({"jsonrpc":"2.0","id":m["id"],"result":r})
                        .to_string()
                        .into_bytes(),
                )
            };
            match m["method"].as_str().unwrap_or_default() {
                "initialize" => reply(
                    serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"f","version":"1"}}),
                ),
                "notifications/initialized" => ("202 Accepted".into(), String::new(), Vec::new()),
                "tools/list" => reply(
                    serde_json::json!({"tools":[{"name":"search","description":"Search plans","inputSchema":{"type":"object","properties":{"q":{"type":"string"}}},"annotations":{"readOnlyHint":true}}]}),
                ),
                "tools/call" => reply(
                    serde_json::json!({"content":[{"type":"text","text":format!("Result for {}", m["params"]["arguments"]["q"].as_str().unwrap_or_default())}]}),
                ),
                _ => ("400 Bad Request".into(), String::new(), Vec::new()),
            }
        }),
    )
}
fn text(s: &str) -> AmbientText {
    AmbientText {
        bytes: s.as_ptr(),
        length: s.len() as u32,
    }
}
/// The root folder of local servers of one test database (plugin ids repeat across databases).
fn root(memory: u64) -> String {
    std::env::temp_dir()
        .join(format!(
            "ambient-plugin-ffi-root-{}-{memory}",
            std::process::id()
        ))
        .display()
        .to_string()
}
/// One blocking discovery through a fresh operation handle.
fn discover(
    memory: u64,
    id: u64,
    server: &str,
    generation: u64,
    key: Option<&str>,
    out: &mut [u8],
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut op = 0;
    assert_eq!(unsafe { ambient_plugin_v2_operation_begin(&mut op) }, 0);
    let root = root(memory);
    let mut used = 0;
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    let status = unsafe {
        ambient_plugin_v2_discover(
            memory,
            op,
            text(&root),
            id,
            text(server),
            generation,
            key.map_or(none, text),
            out.as_mut_ptr(),
            out.len() as u32,
            &mut used,
        )
    };
    assert_eq!(ambient_plugin_v2_operation_close(op), 0);
    assert_eq!(status, 0);
    Ok(serde_json::from_slice(&out[..used as usize])?)
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    memory: u64,
    path: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = ambient_v3_close(self.memory);
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir_all(root(self.memory));
    }
}
/// A memory handle with current High assistance, a task and, if `mcp_url` is given, the Docs
/// plugin installed, discovered, approved and enabled through the plugin ABI.
fn fixture(mcp_url: Option<&str>) -> Result<(Fixture, u64, u64), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-plugin-ffi-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let p = path.to_str().ok_or("path")?.as_bytes();
    let mut memory = 0;
    assert_eq!(
        unsafe { ambient_v3_open(p.as_ptr(), p.len() as u32, &mut memory) },
        0
    );
    let f = Fixture { memory, path };
    assert_eq!(ambient_model_v6_session(memory, 42, 1, 1), 0);
    model::memory(memory, |s| {
        let a = s
            .prepare_check(true, ModelProfile::High)
            .map_err(|_| 1u32)?;
        let flag = a.cancellation();
        let d = s.dispatch_check(a, 1).map_err(|_| 1u32)?;
        s.finish_check(
            d.id(),
            ModelCheckResult {
                outcome: CheckOutcome::Confirmed,
                actual: ActualConfig {
                    model: ActualModel::Luna,
                    effort: ActualEffort::High,
                    tier: ActualTier::Fast,
                },
                usage: None,
            },
        )
        .map_err(|_| 1u32)?;
        s.release_check(&flag);
        Ok(())
    })
    .map_err(|e| format!("ready {e}"))?;
    let Some(url) = mcp_url else {
        return Ok((f, 0, 0));
    };
    let manifest = r#"{"name":"Plans","version":"2.0.0"}"#;
    let servers = format!(
        r#"{{"mcpServers":{{"docs":{{"url":"{url}","headers":{{"Authorization":"Bearer ${{TOKEN}}"}}}}}}}}"#
    );
    let files = [
        AmbientPluginFile {
            kind: 1,
            reserved: 0,
            path: text("plugin.json"),
            bytes: text(manifest),
        },
        AmbientPluginFile {
            kind: 2,
            reserved: 0,
            path: text("mcp.json"),
            bytes: text(&servers),
        },
    ];
    let mut out = vec![0u8; 1024 * 1024];
    let mut used = 0;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(
                files.as_ptr(),
                2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let review: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let mut id = 0;
    let hash = review["hash"].as_str().ok_or("hash")?.to_owned();
    assert_eq!(
        unsafe { ambient_plugin_v2_install(memory, files.as_ptr(), 2, text(&hash), 1, &mut id) },
        0
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_list(memory, out.as_mut_ptr(), out.len() as u32, &mut used) },
        0
    );
    let list: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let generation = list["plugins"][0]["servers"][0]["generation"]
        .as_u64()
        .ok_or("generation")?;
    let revision = list["plugins"][0]["revision"].as_u64().ok_or("revision")?;
    let found = discover(
        memory,
        id,
        "docs",
        generation,
        Some("plugin-secret"),
        &mut out,
    )?;
    assert_eq!(found["status"], "ok");
    let tool = &found["tools"][0];
    assert_eq!(tool["approvable"], true);
    let mut r = 0;
    let def = tool["definition"].as_str().ok_or("definition")?;
    let h = tool["hash"].as_str().ok_or("hash")?;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_approve(
                memory,
                id,
                revision,
                text("docs"),
                text(def),
                text(h),
                &mut r,
            )
        },
        0
    );
    assert_eq!(ambient_plugin_v2_set_enabled(memory, id, 1), 0);
    Ok((f, id, generation))
}
fn begin(memory: u64) -> Result<u64, Box<dyn std::error::Error>> {
    let request = model::memory(memory, |s| {
        let t = s.create_task("Find the launch plan").map_err(|_| 1u32)?;
        let p = s
            .prepare_assistance(AssistanceRequest {
                task: TaskVersion {
                    task_id: t.id,
                    revision: t.revision,
                },
                workspace_revision: 1,
                mode: AssistanceMode::NewContext,
                selected_evidence: vec![],
                reply: None,
            })
            .map_err(|_| 1u32)?;
        Ok(p.request_id)
    })
    .map_err(|e| format!("prepare {e}"))?;
    let mut op = 0;
    assert_eq!(
        unsafe { ambient_model_v6_begin(memory, 1, 2, 3, request, &mut op) },
        0
    );
    Ok(op)
}
fn dispatch(
    memory: u64,
    op: u64,
    keys: &[AmbientPluginKey],
    url: &str,
) -> (u32, AmbientModelCompletion) {
    let mut out = AmbientModelCompletion::default();
    let token = "chatgpt-access";
    let status = unsafe {
        ambient_model_test_dispatch(
            op,
            token.as_ptr(),
            token.len() as u32,
            keys.as_ptr(),
            keys.len() as u32,
            text(&root(memory)),
            url.as_ptr(),
            url.len() as u32,
            &mut out,
        )
    };
    (status, out)
}
fn bodies(s: &Server) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let seen = s.seen.lock().map_err(|_| "seen")?;
    Ok(seen
        .iter()
        .map(|(_, b)| serde_json::from_slice(b).unwrap_or_default())
        .collect())
}

#[test]
fn one_step_without_plugins_is_the_plain_request() -> Result<(), Box<dyn std::error::Error>> {
    let model = serve(
        "/responses",
        Box::new(|_, _| sse(answer_response(&["task:1"]))),
    )?;
    let (f, _, _) = fixture(None)?;
    let op = begin(f.memory)?;
    let (status, out) = dispatch(f.memory, op, &[], &model.url);
    assert_eq!(status, 0);
    assert!(out.published_task > 0);
    assert_eq!(ambient_model_v6_close(op), 0);
    let b = bodies(&model)?;
    assert_eq!(b.len(), 1);
    assert_eq!(b[0]["tools"], serde_json::json!([]));
    assert_eq!(b[0]["input"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn two_steps_call_the_plugin_forward_items_and_save_the_cited_result()
-> Result<(), Box<dyn std::error::Error>> {
    let tools = mcp()?;
    let (f, id, generation) = fixture(Some(&tools.url))?;
    let model = serve(
        "/responses",
        Box::new(|n, _| {
            sse(if n == 0 {
                call_response(0)
            } else {
                answer_response(&["task:1", "tool:1"])
            })
        }),
    )?;
    let op = begin(f.memory)?;
    let keys = [AmbientPluginKey {
        plugin: id,
        server: text("docs"),
        generation,
        key: text("plugin-secret"),
    }];
    let (status, out) = dispatch(f.memory, op, &keys, &model.url);
    assert_eq!(status, 0);
    assert!(out.published_task > 0);
    assert_eq!(out.attempt.outcome, CheckOutcome::Confirmed as u32);
    assert_eq!(ambient_model_v6_close(op), 0);
    let b = bodies(&model)?;
    assert_eq!(b.len(), 2);
    assert_eq!(b[0]["tools"][0]["type"], "namespace");
    let second = b[1]["input"].as_array().ok_or("input")?;
    assert_eq!(second[1]["type"], "reasoning");
    assert_eq!(second[2]["type"], "function_call");
    assert_eq!(second[3]["type"], "function_call_output");
    let envelope: serde_json::Value =
        serde_json::from_str(second[3]["output"].as_str().unwrap_or_default())?;
    assert_eq!(envelope["text"], "Result for plan 0");
    assert_eq!(envelope["citable"], true);
    let seen = tools.seen.lock().map_err(|_| "seen")?.clone();
    assert!(
        seen.iter()
            .all(|(h, _)| h.contains("authorization: bearer plugin-secret"))
    );
    assert!(!seen.iter().any(|(h, _)| h.contains("chatgpt-access")));
    let calls = model::memory(f.memory, |s| s.plugin_calls_json().map_err(|_| 1))
        .map_err(|e| format!("{e}"))?;
    let calls: serde_json::Value = serde_json::from_str(&calls)?;
    assert_eq!(calls[0]["outcome"], PluginCallOutcome::Completed as u32);
    assert!(!calls.to_string().contains("plugin-secret"));
    let saved = model::memory(f.memory, |s| {
        let task = TaskId(out.published_task as i64);
        s.evidence(task, 0, 50).map_err(|_| 1)
    })
    .map_err(|e| format!("{e}"))?;
    assert_eq!(saved.items.len(), 1);
    assert_eq!(saved.items[0].text, "Result for plan 0");
    Ok(())
}

#[test]
fn n_steps_end_with_a_final_post_without_tools() -> Result<(), Box<dyn std::error::Error>> {
    let tools = mcp()?;
    let (f, id, generation) = fixture(Some(&tools.url))?;
    let model = serve(
        "/responses",
        Box::new(|n, _| {
            sse(if n < 4 {
                call_response(n)
            } else {
                answer_response(&["tool:4"])
            })
        }),
    )?;
    let op = begin(f.memory)?;
    let keys = [AmbientPluginKey {
        plugin: id,
        server: text("docs"),
        generation,
        key: text("plugin-secret"),
    }];
    let (status, out) = dispatch(f.memory, op, &keys, &model.url);
    assert_eq!(status, 0);
    assert!(out.published_task > 0);
    assert_eq!(ambient_model_v6_close(op), 0);
    let b = bodies(&model)?;
    assert_eq!(b.len(), 5);
    assert!(
        b[..4]
            .iter()
            .all(|v| v["tools"].as_array().is_some_and(|t| t.len() == 1))
    );
    assert_eq!(b[4]["tools"], serde_json::json!([]));
    let methods = bodies(&tools)?
        .iter()
        .map(|v| v["method"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    // Discovery listed once, then each call lists only the first time in its request.
    assert_eq!(methods.iter().filter(|m| *m == "tools/call").count(), 4);
    assert_eq!(methods.iter().filter(|m| *m == "tools/list").count(), 2);
    let summary = model::memory(f.memory, |s| s.check_summary().map_err(|_| 1))
        .map_err(|e| format!("{e}"))?;
    assert_eq!(summary.attempts, 6, "one connectivity check and five POSTs");
    Ok(())
}

const STDIO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/tests/fixtures/stdio_server.py"
);
/// A local plugin with one command server that needs `API_TOKEN`, installed through the ABI.
fn local_plugin(memory: u64, mode: &str) -> Result<(u64, u64, u64), Box<dyn std::error::Error>> {
    let servers = serde_json::json!({"mcpServers":{"local":{"command":"/usr/bin/python3","args":[STDIO, mode],"env":{"API_TOKEN":"ignored"}}}}).to_string();
    let files = [
        AmbientPluginFile {
            kind: 1,
            reserved: 0,
            path: text("plugin.json"),
            bytes: text(r#"{"name":"Local tools"}"#),
        },
        AmbientPluginFile {
            kind: 2,
            reserved: 0,
            path: text("mcp.json"),
            bytes: text(&servers),
        },
    ];
    let mut out = vec![0u8; 1024 * 1024];
    let mut used = 0;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(
                files.as_ptr(),
                2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let review: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    assert_eq!(review["servers"][0]["kind"]["kind"], "local");
    assert_eq!(
        review["servers"][0]["kind"]["env"],
        serde_json::json!(["API_TOKEN"])
    );
    let hash = review["hash"].as_str().ok_or("hash")?.to_owned();
    let mut id = 0;
    assert_eq!(
        unsafe { ambient_plugin_v2_install(memory, files.as_ptr(), 2, text(&hash), 1, &mut id) },
        0
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_list(memory, out.as_mut_ptr(), out.len() as u32, &mut used) },
        0
    );
    let list: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let p = &list["plugins"][0];
    let generation = p["servers"][0]["generation"].as_u64().ok_or("generation")?;
    Ok((id, p["revision"].as_u64().ok_or("revision")?, generation))
}
fn starts(memory: u64, id: u64) -> Vec<i32> {
    std::fs::read_to_string(
        std::path::Path::new(&root(memory))
            .join(id.to_string())
            .join("starts"),
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|l| l.parse().ok())
    .collect()
}
fn group_gone(pid: i32) -> bool {
    // kill(-pid, 0) through the shell's own tool keeps this crate free of new dependencies.
    std::process::Command::new("/bin/kill")
        .args(["-0", &format!("-{pid}")])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| !s.success())
}

#[test]
fn a_local_server_is_discovered_called_by_the_model_and_stopped_with_the_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let (f, _, _) = fixture(None)?;
    let (id, revision, generation) = local_plugin(f.memory, "ok")?;
    let mut out = vec![0u8; 512 * 1024];
    let env = r#"{"API_TOKEN":"local-secret-1"}"#;
    // Without values nothing starts; with them the program is started, listed and stopped.
    let missing = discover(f.memory, id, "local", generation, None, &mut out)?;
    assert_eq!(missing["status"], "not_configured");
    assert!(starts(f.memory, id).is_empty());
    let found = discover(f.memory, id, "local", generation, Some(env), &mut out)?;
    assert_eq!(found["status"], "ok", "{found}");
    assert!(found.get("stop_confirmed").is_none());
    let discovered = starts(f.memory, id);
    assert_eq!(discovered.len(), 1);
    assert!(group_gone(discovered[0]), "discovery stops its process");
    let echo = found["tools"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["name"] == "echo"))
        .ok_or("echo")?;
    let mut r = 0;
    let (def, h) = (
        echo["definition"].as_str().ok_or("definition")?,
        echo["hash"].as_str().ok_or("hash")?,
    );
    assert_eq!(
        unsafe {
            ambient_plugin_v2_approve(
                f.memory,
                id,
                revision,
                text("local"),
                text(def),
                text(h),
                &mut r,
            )
        },
        0
    );
    assert_eq!(ambient_plugin_v2_set_enabled(f.memory, id, 1), 0);
    let model = serve(
        "/responses",
        Box::new(|n, _| {
            sse(if n < 2 {
                call_named(n, "echo")
            } else {
                answer_response(&["tool:2"])
            })
        }),
    )?;
    let op = begin(f.memory)?;
    let keys = [AmbientPluginKey {
        plugin: id,
        server: text("local"),
        generation,
        key: text(env),
    }];
    let (status, done) = dispatch(f.memory, op, &keys, &model.url);
    assert_eq!(status, 0);
    assert!(done.published_task > 0);
    // One start for the whole request (two calls), stopped before dispatch returned.
    let all = starts(f.memory, id);
    assert_eq!(all.len(), 2);
    assert!(group_gone(all[1]));
    assert_eq!(ambient_model_v6_close(op), 0);
    let b = bodies(&model)?;
    let second = b[1]["input"].as_array().ok_or("input")?;
    let envelope: serde_json::Value =
        serde_json::from_str(second[3]["output"].as_str().unwrap_or_default())?;
    assert_eq!(envelope["text"], r#"echo:{"q": "plan 0"}"#);
    let calls = model::memory(f.memory, |s| s.plugin_calls_json().map_err(|_| 1))
        .map_err(|e| format!("{e}"))?;
    let calls: serde_json::Value = serde_json::from_str(&calls)?;
    assert_eq!(calls.as_array().map(Vec::len), Some(2));
    assert!(calls.as_array().is_some_and(|c| {
        c.iter()
            .all(|c| c["outcome"] == PluginCallOutcome::Completed as u32)
    }));
    assert!(!calls.to_string().contains("local-secret-1"));
    // The secret never reached SQLite.
    let bytes = std::fs::read(&f.path)?;
    assert!(!bytes.windows(14).any(|w| w == b"local-secret-1"));
    Ok(())
}

#[test]
fn a_discovery_is_cancelled_through_its_operation_and_close_waits_for_it()
-> Result<(), Box<dyn std::error::Error>> {
    let (f, _, _) = fixture(None)?;
    let (id, _, generation) = local_plugin(f.memory, "silent")?;
    let mut op = 0;
    assert_eq!(unsafe { ambient_plugin_v2_operation_begin(&mut op) }, 0);
    let memory = f.memory;
    let worker = thread::spawn(move || {
        let root = root(memory);
        let env = r#"{"API_TOKEN":"x"}"#;
        let mut out = vec![0u8; 512 * 1024];
        let mut used = 0;
        let started = std::time::Instant::now();
        let status = unsafe {
            ambient_plugin_v2_discover(
                memory,
                op,
                text(&root),
                id,
                text("local"),
                generation,
                text(env),
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        };
        let v: serde_json::Value =
            serde_json::from_slice(&out[..used as usize]).unwrap_or_default();
        (status, v, started.elapsed())
    });
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while starts(f.memory, id).is_empty() && std::time::Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        ambient_plugin_v2_operation_close(op),
        4,
        "busy while discovering"
    );
    assert_eq!(ambient_plugin_v2_operation_cancel(op), 0);
    let (status, v, took) = worker.join().map_err(|_| "worker")?;
    assert_eq!(status, 0);
    assert_eq!(v["status"], "cancelled");
    assert!(took < Duration::from_secs(10));
    assert!(group_gone(starts(f.memory, id)[0]));
    assert_eq!(ambient_plugin_v2_operation_close(op), 0);
    Ok(())
}

/// Manual acceptance only (ADR-022): one real local server, never in CI. Run with
/// `AMBIENT_24C_SERVER='{"command":"npx","args":[...],"tool":"<read-only tool>","arguments":{...}}'
/// cargo test -p ambient-bridge --features model-test-support --lib manual_acceptance -- --ignored
/// --nocapture`. Prints the phases and their times as JSON.
#[test]
#[ignore = "manual acceptance against a real local server"]
fn manual_acceptance_of_one_real_local_server() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    let spec: serde_json::Value = serde_json::from_str(&std::env::var("AMBIENT_24C_SERVER")?)?;
    let (f, _, _) = fixture(None)?;
    let servers =
        serde_json::json!({"mcpServers":{"real":{"command":spec["command"],"args":spec["args"]}}})
            .to_string();
    let files = [
        AmbientPluginFile {
            kind: 1,
            reserved: 0,
            path: text("plugin.json"),
            bytes: text(r#"{"name":"Acceptance"}"#),
        },
        AmbientPluginFile {
            kind: 2,
            reserved: 0,
            path: text("mcp.json"),
            bytes: text(&servers),
        },
    ];
    let mut out = vec![0u8; 1024 * 1024];
    let mut used = 0;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(
                files.as_ptr(),
                2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let review: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let hash = review["hash"].as_str().ok_or("hash")?.to_owned();
    let mut id = 0;
    assert_eq!(
        unsafe { ambient_plugin_v2_install(f.memory, files.as_ptr(), 2, text(&hash), 1, &mut id) },
        0
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_list(f.memory, out.as_mut_ptr(), out.len() as u32, &mut used) },
        0
    );
    let list: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let (revision, generation) = (
        list["plugins"][0]["revision"].as_u64().ok_or("revision")?,
        list["plugins"][0]["servers"][0]["generation"]
            .as_u64()
            .ok_or("generation")?,
    );
    let started = Instant::now();
    let found = discover(f.memory, id, "real", generation, None, &mut out)?;
    let cold = started.elapsed();
    let started = Instant::now();
    let warm_found = discover(f.memory, id, "real", generation, None, &mut out)?;
    let warm = started.elapsed();
    let name: &'static str = Box::leak(
        spec["tool"]
            .as_str()
            .ok_or("tool")?
            .to_owned()
            .into_boxed_str(),
    );
    let tool = found["tools"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["name"] == name))
        .ok_or("tool not listed")?
        .clone();
    let mut r = 0;
    let (def, h) = (
        tool["definition"]
            .as_str()
            .ok_or("not approvable (no readOnlyHint: true)")?,
        tool["hash"].as_str().ok_or("hash")?,
    );
    assert_eq!(
        unsafe {
            ambient_plugin_v2_approve(
                f.memory,
                id,
                revision,
                text("real"),
                text(def),
                text(h),
                &mut r,
            )
        },
        0
    );
    assert_eq!(ambient_plugin_v2_set_enabled(f.memory, id, 1), 0);
    let arguments = spec["arguments"].to_string();
    let call = move |n: usize| {
        let items = [
            serde_json::json!({"id":format!("rs_{n}"),"type":"reasoning","summary":[],"encrypted_content":"opaque"}),
            serde_json::json!({"id":format!("fc_{n}"),"type":"function_call","status":"completed","call_id":format!("call_{n}"),"name":tool["name"],"arguments":arguments}),
        ];
        completed(serde_json::json!(items), &items)
    };
    let model = serve(
        "/responses",
        Box::new(move |n, _| {
            sse(if n == 0 {
                call(n)
            } else {
                answer_response(&["task:1"])
            })
        }),
    )?;
    let op = begin(f.memory)?;
    let started = Instant::now();
    let (status, _) = dispatch(f.memory, op, &[], &model.url);
    let dispatched = started.elapsed();
    assert_eq!(ambient_model_v6_close(op), 0);
    let calls = model::memory(f.memory, |s| s.plugin_calls_json().map_err(|_| 1))
        .map_err(|e| format!("{e}"))?;
    let b = bodies(&model)?;
    let result = b
        .get(1)
        .and_then(|v| v["input"].as_array())
        .and_then(|i| i.get(3))
        .and_then(|o| o["output"].as_str())
        .map(|o| o.chars().take(300).collect::<String>());
    // Cancel while the server starts: the dispatch returns after its process group is gone.
    let slow = serve("/responses", Box::new(move |_, _| sse(call_named(0, name))))?;
    let op = begin(f.memory)?;
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        ambient_model_v6_cancel(op)
    });
    let started = Instant::now();
    let (cancel_status, _) = dispatch(f.memory, op, &[], &slow.url);
    let cancelled = started.elapsed();
    let _ = canceller.join();
    assert_eq!(ambient_model_v6_close(op), 0);
    let after_cancel = model::memory(f.memory, |s| s.plugin_calls_json().map_err(|_| 1))
        .map_err(|e| format!("{e}"))?;
    // No process of the server may outlive the returned dispatch: every one of them runs from
    // this database's private root (uv's cache and the interpreter live there).
    let left = std::process::Command::new("/usr/bin/pgrep")
        .args(["-f", &root(f.memory)])
        .output()?;
    println!(
        "{}",
        serde_json::json!({
            "review_servers": review["servers"],
            "discovery_cold_ms": cold.as_millis(), "discovery_status": found["status"],
            "tools_listed": found["tools"].as_array().map(Vec::len),
            "discovery_warm_ms": warm.as_millis(), "warm_status": warm_found["status"],
            "dispatch_status": status, "dispatch_ms": dispatched.as_millis(),
            "call_log": serde_json::from_str::<serde_json::Value>(&calls)?,
            "result_envelope_prefix": result,
            "cancel_status": cancel_status, "cancel_return_ms": cancelled.as_millis(),
            "call_log_after_cancel": serde_json::from_str::<serde_json::Value>(&after_cancel)?,
            "processes_left": String::from_utf8_lossy(&left.stdout).lines().count(),
        })
    );
    Ok(())
}

const WORKSPACE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/tests/fixtures/workspace_server.py"
);
const REQUIREMENTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/managed/google-workspace-requirements.txt"
);
/// The installed managed server of one test database: `<root>/google-workspace-mcp/<sha256 of
/// the pinned requirements>/bin/python` runs the stateless workspace-mcp imitation.
fn installed_google(memory: u64) -> Result<(), Box<dyn std::error::Error>> {
    let sum = std::process::Command::new("/usr/bin/shasum")
        .args(["-a", "256", REQUIREMENTS])
        .output()?;
    let version = String::from_utf8(sum.stdout)?
        .split_whitespace()
        .next()
        .ok_or("sum")?
        .to_owned();
    let bin = std::path::Path::new(&root(memory))
        .join("google-workspace-mcp")
        .join(version)
        .join("bin");
    std::fs::create_dir_all(&bin)?;
    let python = bin.join("python");
    std::fs::write(
        &python,
        format!("#!/bin/sh\nexec /usr/bin/python3 {WORKSPACE} ok \"$@\"\n"),
    )?;
    std::process::Command::new("/bin/chmod")
        .args(["755", &python.display().to_string()])
        .status()?;
    Ok(())
}

#[test]
fn the_google_plugin_is_discovered_with_its_token_called_by_the_model_and_stopped()
-> Result<(), Box<dyn std::error::Error>> {
    let (f, _, _) = fixture(None)?;
    let mut out = vec![0u8; 1024 * 1024];
    let mut used = 0;
    let id_text = text("google-workspace");
    assert_eq!(
        unsafe {
            ambient_plugin_v2_builtin(id_text, out.as_mut_ptr(), out.len() as u32, &mut used)
        },
        0
    );
    let built: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let owned = built["files"]
        .as_array()
        .ok_or("files")?
        .iter()
        .map(|f| {
            (
                f["kind"].as_u64().unwrap_or_default() as u32,
                f["path"].as_str().unwrap_or_default().to_owned(),
                f["text"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let files = owned
        .iter()
        .map(|(k, p, t)| AmbientPluginFile {
            kind: *k,
            reserved: 0,
            path: text(p),
            bytes: text(t),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        unsafe {
            ambient_plugin_v2_review(
                files.as_ptr(),
                2,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut used,
            )
        },
        0
    );
    let review: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let hash = review["hash"].as_str().ok_or("hash")?.to_owned();
    let mut id = 0;
    assert_eq!(
        unsafe { ambient_plugin_v2_install(f.memory, files.as_ptr(), 2, text(&hash), 1, &mut id) },
        0
    );
    assert_eq!(
        unsafe { ambient_plugin_v2_list(f.memory, out.as_mut_ptr(), out.len() as u32, &mut used) },
        0
    );
    let list: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
    let p = &list["plugins"][0];
    let (revision, generation) = (
        p["revision"].as_u64().ok_or("revision")?,
        p["servers"][0]["generation"].as_u64().ok_or("generation")?,
    );
    let token = "ya29.bridge-test-token";
    // Not installed yet: «unavailable», nothing starts.
    let missing = discover(f.memory, id, "google", generation, Some(token), &mut out)?;
    assert_eq!(missing["status"], "unavailable");
    installed_google(f.memory)?;
    let signed_out = discover(f.memory, id, "google", generation, None, &mut out)?;
    assert_eq!(signed_out["status"], "login_required");
    assert!(starts(f.memory, id).is_empty());
    let found = discover(f.memory, id, "google", generation, Some(token), &mut out)?;
    assert_eq!(found["status"], "ok", "{found}");
    let tools = found["tools"].as_array().ok_or("tools")?;
    let by_name = |n: &str| tools.iter().find(|t| t["name"] == n).cloned();
    // Declared read-only but outside Core's allowlist: shown, never approvable.
    assert_eq!(
        by_name("get_drive_file_download_url").ok_or("download")?["approvable"],
        false
    );
    let search = by_name("search_docs").ok_or("search")?;
    assert_eq!(search["approvable"], true);
    assert!(
        group_gone(starts(f.memory, id)[0]),
        "discovery stops its process"
    );
    let mut r = 0;
    assert_eq!(
        unsafe {
            ambient_plugin_v2_approve(
                f.memory,
                id,
                revision,
                text("google"),
                text(search["definition"].as_str().ok_or("definition")?),
                text(search["hash"].as_str().ok_or("hash")?),
                &mut r,
            )
        },
        0
    );
    assert_eq!(ambient_plugin_v2_set_enabled(f.memory, id, 1), 0);
    let model = serve(
        "/responses",
        Box::new(|n, _| {
            sse(if n < 1 {
                call_named(n, "search_docs")
            } else {
                answer_response(&["tool:1"])
            })
        }),
    )?;
    let op = begin(f.memory)?;
    let keys = [AmbientPluginKey {
        plugin: id,
        server: text("google"),
        generation,
        key: text(token),
    }];
    let (status, done) = dispatch(f.memory, op, &keys, &model.url);
    assert_eq!(status, 0);
    assert!(done.published_task > 0);
    assert_eq!(ambient_model_v6_close(op), 0);
    let all = starts(f.memory, id);
    assert_eq!(all.len(), 2);
    assert!(group_gone(all[1]), "stopped before dispatch returned");
    let b = bodies(&model)?;
    let second = b[1]["input"].as_array().ok_or("input")?;
    let envelope: serde_json::Value = serde_json::from_str(
        second
            .iter()
            .find_map(|i| i["output"].as_str())
            .unwrap_or_default(),
    )?;
    assert_eq!(envelope["text"], r#"found {"q": "plan 0"}"#);
    // The token went only as the Bearer header; it is in neither SQLite nor the call log.
    let requests = std::fs::read_to_string(
        std::path::Path::new(&root(f.memory))
            .join(id.to_string())
            .join("requests"),
    )?;
    assert!(
        requests
            .lines()
            .all(|l| l.ends_with(&format!("Bearer {token}")))
    );
    let calls = model::memory(f.memory, |s| s.plugin_calls_json().map_err(|_| 1))
        .map_err(|e| format!("{e}"))?;
    assert!(!calls.contains(token));
    let bytes = std::fs::read(&f.path)?;
    assert!(!bytes.windows(token.len()).any(|w| w == token.as_bytes()));
    Ok(())
}
