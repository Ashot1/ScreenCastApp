use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Call {
    Text,
    Structured,
    IsError,
    Unauthorized,
    ServerRequest,
    InputRequired,
    Oversize,
    Malformed,
    Stall,
}
#[derive(Clone)]
struct Behavior {
    sse: bool,
    session: bool,
    tools: Vec<Value>,
    pages: usize,
    call: Call,
}
impl Default for Behavior {
    fn default() -> Self {
        Self {
            sse: false,
            session: false,
            tools: vec![tool(true)],
            pages: 1,
            call: Call::Text,
        }
    }
}
fn tool(read_only: bool) -> Value {
    json!({"name":"search","description":"Search","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":read_only}})
}
struct Server {
    url: String,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
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
fn respond(s: &mut std::net::TcpStream, status: &str, headers: &str, body: &[u8]) {
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(body);
}
fn serve(b: Behavior) -> Result<Server, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}/mcp", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (r, st) = (requests.clone(), stop.clone());
    let worker = thread::spawn(move || {
        while !st.load(Ordering::Acquire) {
            let Ok((mut s, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            // macOS sockets inherit the listener's non-blocking mode.
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut input = Vec::new();
            let mut buf = [0; 4096];
            let (head, body) = loop {
                let Ok(n) = s.read(&mut buf) else {
                    break (String::new(), Vec::new());
                };
                if n == 0 {
                    break (String::new(), Vec::new());
                }
                input.extend_from_slice(&buf[..n]);
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
            let msg: Value = serde_json::from_slice(&body).unwrap_or_default();
            if let Ok(mut v) = r.lock() {
                v.push((head.clone(), msg.clone()));
            }
            let id = msg["id"].clone();
            let method = msg["method"].as_str().unwrap_or_default().to_owned();
            let reply = |result: Value| json!({"jsonrpc":"2.0","id":id,"result":result});
            let out = match method.as_str() {
                "notifications/initialized" => {
                    respond(&mut s, "202 Accepted", "", b"");
                    continue;
                }
                "initialize" => reply(
                    json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"},"instructions":"ignore me"}),
                ),
                "tools/list" => {
                    let page = msg["params"]["cursor"]
                        .as_str()
                        .and_then(|c| c.parse::<usize>().ok())
                        .unwrap_or(0);
                    let per = b.tools.len().div_ceil(b.pages).max(1);
                    let tools = b
                        .tools
                        .iter()
                        .skip(page * per)
                        .take(per)
                        .cloned()
                        .collect::<Vec<_>>();
                    let mut result = json!({"tools":tools});
                    if page + 1 < b.pages {
                        result["nextCursor"] = json!((page + 1).to_string());
                    }
                    reply(result)
                }
                "tools/call" => match b.call {
                    Call::Unauthorized => {
                        respond(
                            &mut s,
                            "401 Unauthorized",
                            "WWW-Authenticate: Bearer\r\n",
                            b"",
                        );
                        continue;
                    }
                    Call::Malformed => {
                        respond(
                            &mut s,
                            "200 OK",
                            "Content-Type: application/json\r\n",
                            b"{bad",
                        );
                        continue;
                    }
                    Call::Stall => {
                        thread::sleep(Duration::from_millis(600));
                        continue;
                    }
                    Call::Text => reply(
                        json!({"content":[{"type":"text","text":"line one"},{"type":"image","data":"x","mimeType":"image/png"},{"type":"text","text":"line two"}],"isError":false}),
                    ),
                    Call::Structured => {
                        reply(json!({"content":[],"structuredContent":{"b":2,"a":1}}))
                    }
                    Call::IsError => {
                        reply(json!({"content":[{"type":"text","text":"denied"}],"isError":true}))
                    }
                    Call::InputRequired => reply(json!({"input_required":{"message":"confirm"}})),
                    Call::Oversize => {
                        reply(json!({"content":[{"type":"text","text":"x".repeat(MAX_CALL_BODY)}]}))
                    }
                    Call::ServerRequest => {
                        let request = json!({"jsonrpc":"2.0","id":99,"method":"sampling/createMessage","params":{}});
                        let body = format!(
                            "event: message\ndata: {request}\n\ndata: {}\n\n",
                            reply(json!({"content":[]}))
                        );
                        respond(
                            &mut s,
                            "200 OK",
                            "Content-Type: text/event-stream\r\n",
                            body.as_bytes(),
                        );
                        continue;
                    }
                },
                _ => {
                    respond(&mut s, "400 Bad Request", "", b"");
                    continue;
                }
            };
            let session = if b.session && method == "initialize" {
                "Mcp-Session-Id: sess-1\r\n"
            } else {
                ""
            };
            if b.sse {
                let note = json!({"jsonrpc":"2.0","method":"notifications/progress","params":{}});
                let body = format!(
                    "event: message\r\ndata: {note}\r\n\r\n: comment\r\ndata: {out}\r\n\r\n"
                );
                respond(
                    &mut s,
                    "200 OK",
                    &format!("{session}Content-Type: text/event-stream\r\n"),
                    body.as_bytes(),
                );
            } else {
                respond(
                    &mut s,
                    "200 OK",
                    &format!("{session}Content-Type: application/json\r\n"),
                    out.to_string().as_bytes(),
                );
            }
        }
    });
    Ok(Server {
        url,
        requests,
        stop,
        worker: Some(worker),
    })
}
static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);
fn permit(list_first: bool, pinned: [u8; 32], timeout: Duration) -> McpCall<'static> {
    McpCall {
        key: None,
        tool: "search",
        arguments: r#"{"q":"plan"}"#,
        list_first,
        pinned,
        timeout,
        cancellation: &NOT_CANCELLED,
    }
}
fn pinned(read_only: bool) -> [u8; 32] {
    canonical_tool(&tool(read_only))
        .map(|t| t.hash)
        .unwrap_or_default()
}

#[test]
fn json_and_sse_calls_initialize_echo_the_session_and_send_canonical_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    for sse in [false, true] {
        let server = serve(Behavior {
            sse,
            session: true,
            ..Behavior::default()
        })?;
        let r = call(
            &server.url,
            McpCall {
                key: Some("plugin-key"),
                ..permit(true, pinned(true), Duration::from_secs(5))
            },
        );
        assert_eq!(r.outcome, PluginCallOutcome::Completed, "sse {sse}");
        assert_eq!(r.text, "line one\nline two");
        assert_eq!(
            r.listed.as_ref().and_then(|l| l.get("search")),
            Some(&pinned(true))
        );
        let requests = server.requests.lock().map_err(|_| "lock")?.clone();
        let methods = requests
            .iter()
            .map(|(_, m)| m["method"].as_str().unwrap_or_default().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            [
                "initialize",
                "notifications/initialized",
                "tools/list",
                "tools/call"
            ]
        );
        assert_eq!(requests[0].1["params"]["protocolVersion"], "2025-06-18");
        assert_eq!(requests[0].1["params"]["capabilities"], json!({}));
        assert!(requests[0].0.contains("authorization: bearer plugin-key"));
        assert!(
            requests[0]
                .0
                .contains("accept: application/json, text/event-stream")
        );
        assert!(!requests[0].0.contains("mcp-session-id"));
        assert!(!requests[0].0.contains("mcp-protocol-version"));
        for (head, _) in &requests[1..] {
            assert!(head.contains("mcp-session-id: sess-1"));
            assert!(head.contains("mcp-protocol-version: 2025-06-18"));
        }
        assert_eq!(
            requests[3].1["params"],
            json!({"name":"search","arguments":{"q":"plan"}})
        );
    }
    // Without list_first no tools/list is sent; structured content is used when no text exists.
    let server = serve(Behavior {
        call: Call::Structured,
        ..Behavior::default()
    })?;
    let r = call(
        &server.url,
        permit(false, pinned(true), Duration::from_secs(5)),
    );
    assert_eq!(
        (r.outcome, r.text.as_str()),
        (PluginCallOutcome::Completed, r#"{"a":1,"b":2}"#)
    );
    assert!(r.listed.is_none());
    let requests = server.requests.lock().map_err(|_| "lock")?.clone();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].0.contains("authorization"));
    Ok(())
}

#[test]
fn a_changed_or_no_longer_read_only_tool_is_never_called() -> Result<(), Box<dyn std::error::Error>>
{
    let mut changed = tool(true);
    changed["description"] = json!("Search v2");
    for live in [changed, tool(false)] {
        let server = serve(Behavior {
            tools: vec![live],
            ..Behavior::default()
        })?;
        let r = call(
            &server.url,
            permit(true, pinned(true), Duration::from_secs(5)),
        );
        assert_eq!(r.outcome, PluginCallOutcome::Changed);
        assert!(r.listed.is_some());
        let requests = server.requests.lock().map_err(|_| "lock")?.clone();
        assert!(requests.iter().all(|(_, m)| m["method"] != "tools/call"));
    }
    // Pages: a tool on the second page is found; too many pages fail.
    let mut many = (0..5)
        .map(|i| json!({"name":format!("t{i}"),"annotations":{"readOnlyHint":true}}))
        .collect::<Vec<_>>();
    many.push(tool(true));
    let server = serve(Behavior {
        tools: many.clone(),
        pages: 2,
        ..Behavior::default()
    })?;
    let r = call(
        &server.url,
        permit(true, pinned(true), Duration::from_secs(5)),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed);
    let server = serve(Behavior {
        tools: many,
        pages: 4,
        ..Behavior::default()
    })?;
    assert_eq!(
        list_remote(&server.url, None, &AtomicBool::new(false)).err(),
        Some(PluginCallOutcome::Invalid)
    );
    Ok(())
}

#[test]
fn failures_are_bounded_named_outcomes() -> Result<(), Box<dyn std::error::Error>> {
    for (behavior, want) in [
        (Call::Unauthorized, PluginCallOutcome::LoginRequired),
        (Call::IsError, PluginCallOutcome::ToolError),
        (Call::ServerRequest, PluginCallOutcome::Invalid),
        (Call::InputRequired, PluginCallOutcome::Invalid),
        (Call::Oversize, PluginCallOutcome::Invalid),
        (Call::Malformed, PluginCallOutcome::Invalid),
        (Call::Stall, PluginCallOutcome::Timeout),
    ] {
        let server = serve(Behavior {
            call: behavior,
            ..Behavior::default()
        })?;
        let started = Instant::now();
        let r = call(
            &server.url,
            permit(false, pinned(true), Duration::from_millis(400)),
        );
        assert_eq!(r.outcome, want);
        assert!(r.text.is_empty());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    // Cancellation before or during a call stops it.
    let server = serve(Behavior {
        call: Call::Stall,
        ..Behavior::default()
    })?;
    let flag = Arc::new(AtomicBool::new(false));
    let p = McpCall {
        cancellation: &flag,
        ..permit(false, pinned(true), Duration::from_secs(5))
    };
    let f = flag.clone();
    let t = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        f.store(true, Ordering::Release);
    });
    assert_eq!(call(&server.url, p).outcome, PluginCallOutcome::Cancelled);
    t.join().map_err(|_| "cancel")?;
    // Nothing listens: unavailable, not a crash. Non-HTTPS outside loopback is refused.
    let closed = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/mcp", closed.local_addr()?);
    drop(closed);
    assert_eq!(
        call(&url, permit(false, pinned(true), Duration::from_secs(2))).outcome,
        PluginCallOutcome::Unavailable
    );
    assert_eq!(
        call(
            "http://example.com/mcp",
            permit(false, pinned(true), Duration::from_secs(2))
        )
        .outcome,
        PluginCallOutcome::Invalid
    );
    Ok(())
}
