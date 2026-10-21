//! Minimal MCP 2025-06-18 client for plugin tools (ADR-021, ADR-022): one JSON-RPC protocol over
//! two wires, streamable HTTP for remote servers and stdio for local ones (`local_host.rs`). No
//! sampling or elicitation is declared, so any server request is a failed call.
use crate::wait::{Stop, waiting};
use ambient_core::{
    MAX_LISTED_TOOLS, PluginCallOutcome, ToolCallPermit, ToolCallResult, canonical_tool,
};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderValue};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const PROTOCOL: &str = "2025-06-18";
/// One `tools/call` response read; Core keeps a 12 KiB prefix of the text.
pub(crate) const MAX_CALL_BODY: usize = 256 * 1024;
/// All `tools/list` pages of one server.
const MAX_LIST_BODY: usize = 128 * 1024;
const MAX_LIST_PAGES: usize = 3;
pub(crate) const MAX_SMALL_BODY: usize = 64 * 1024;
const MAX_HEADERS: usize = 32 * 1024;
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// One MCP transport: it frames a JSON-RPC message, returns the response with our id and the
/// bytes read for it, and enforces the operation's byte limit while reading.
pub(crate) trait Wire {
    fn next_id(&mut self) -> u64;
    fn request(
        &mut self,
        id: u64,
        message: &Value,
        limit: usize,
    ) -> Result<(Value, usize), PluginCallOutcome>;
    fn notify(&mut self, message: &Value) -> Result<(), PluginCallOutcome>;
}
/// One incoming message on either wire: `true` for the answer to `id`, `false` for a notification
/// to skip. A request from the server fails: no client capability is declared.
pub(crate) fn answer(v: &Value, id: u64) -> Result<bool, PluginCallOutcome> {
    match (v.get("method").is_some(), v.get("id")) {
        (true, Some(_)) => Err(PluginCallOutcome::Invalid),
        (true, None) => Ok(false),
        (false, Some(i)) if i.as_u64() == Some(id) => Ok(true),
        _ => Err(PluginCallOutcome::Invalid),
    }
}
/// The JSON-RPC logic shared by both wires: initialize, list and call.
pub(crate) struct Protocol<W>(pub W);
impl<W: Wire> Protocol<W> {
    fn request(
        &mut self,
        method: &str,
        params: Value,
        limit: usize,
    ) -> Result<(Value, usize), PluginCallOutcome> {
        let id = self.0.next_id();
        let (v, read) = self.0.request(
            id,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            limit,
        )?;
        if v.get("id").and_then(Value::as_u64) != Some(id)
            || v.get("jsonrpc") != Some(&json!("2.0"))
        {
            return Err(PluginCallOutcome::Invalid);
        }
        match (v.get("result"), v.get("error")) {
            (Some(r), None) if r.is_object() => Ok((r.clone(), read)),
            (None, Some(e)) if e.is_object() => Err(PluginCallOutcome::ToolError),
            _ => Err(PluginCallOutcome::Invalid),
        }
    }
    pub(crate) fn initialize(&mut self) -> Result<(), PluginCallOutcome> {
        let (r, _) = self.request(
            "initialize",
            json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"Ambient","version":"1"}}),
            MAX_SMALL_BODY,
        )?;
        if r.get("protocolVersion").and_then(Value::as_str) != Some(PROTOCOL) {
            return Err(PluginCallOutcome::Unavailable);
        }
        self.0
            .notify(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
    }
    pub(crate) fn list(&mut self) -> Result<Vec<Value>, PluginCallOutcome> {
        let (mut tools, mut cursor, mut read) = (Vec::new(), None::<String>, 0usize);
        for _ in 0..MAX_LIST_PAGES {
            let params = cursor.map_or_else(|| json!({}), |c| json!({ "cursor": c }));
            let (r, n) = self.request("tools/list", params, MAX_LIST_BODY - read)?;
            read += n;
            tools.extend(
                r.get("tools")
                    .and_then(Value::as_array)
                    .ok_or(PluginCallOutcome::Invalid)?
                    .iter()
                    .cloned(),
            );
            if tools.len() > MAX_LISTED_TOOLS {
                return Err(PluginCallOutcome::Invalid);
            }
            match r.get("nextCursor") {
                None | Some(Value::Null) => return Ok(tools),
                Some(Value::String(c)) => cursor = Some(c.clone()),
                Some(_) => return Err(PluginCallOutcome::Invalid),
            }
        }
        Err(PluginCallOutcome::Invalid)
    }
    fn call(&mut self, name: &str, arguments: &str) -> Result<String, PluginCallOutcome> {
        let arguments: Value =
            serde_json::from_str(arguments).map_err(|_| PluginCallOutcome::Invalid)?;
        let (r, _) = self.request(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            MAX_CALL_BODY,
        )?;
        // Later protocol revisions ask for user input instead of answering: never supported here.
        if r.get("input_required").is_some() || r.get("inputRequired").is_some() {
            return Err(PluginCallOutcome::Invalid);
        }
        if r.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(PluginCallOutcome::ToolError);
        }
        // Text content only; images, resources and links are dropped.
        let text = r
            .get("content")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|i| i.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|i| i.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if text.is_empty()
            && let Some(structured) = r.get("structuredContent")
        {
            return Ok(structured.to_string());
        }
        Ok(text)
    }
    /// One admitted call: list first (once per server per request), never call a tool whose
    /// live definition differs from the pin, then `tools/call`.
    pub(crate) fn admitted_call(
        &mut self,
        c: &McpCall<'_>,
        listed: &mut Option<BTreeMap<String, [u8; 32]>>,
    ) -> Result<String, PluginCallOutcome> {
        if c.list_first {
            let hashes = self
                .list()?
                .iter()
                .filter_map(|t| canonical_tool(t).ok())
                .map(|t| (t.name, t.hash))
                .collect::<BTreeMap<_, _>>();
            let same = hashes.get(c.tool) == Some(&c.pinned);
            *listed = Some(hashes);
            if !same {
                return Err(PluginCallOutcome::Changed);
            }
        }
        self.call(c.tool, c.arguments)
    }
}
/// Streamable HTTP for remote servers: one runtime and client per operation; every request is
/// driven by `block_on` on that runtime from the calling thread.
struct HttpWire<'a> {
    runtime: &'a tokio::runtime::Runtime,
    client: reqwest::Client,
    url: &'a str,
    key: Option<&'a str>,
    flag: &'a AtomicBool,
    deadline: Instant,
    session: Option<HeaderValue>,
    initialized: bool,
    next: u64,
}
fn stop(s: Stop) -> PluginCallOutcome {
    match s {
        Stop::Cancelled => PluginCallOutcome::Cancelled,
        Stop::Timeout => PluginCallOutcome::Timeout,
        Stop::Failed => PluginCallOutcome::Unavailable,
    }
}
impl HttpWire<'_> {
    async fn send(&mut self, message: &Value) -> Result<reqwest::Response, PluginCallOutcome> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PluginCallOutcome::Timeout);
        }
        let mut request = self
            .client
            .post(self.url)
            .timeout(remaining)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .body(message.to_string());
        if let Some(key) = self.key {
            request = request.bearer_auth(key);
        }
        if let Some(id) = &self.session {
            request = request.header("mcp-session-id", id.clone());
        }
        if self.initialized {
            request = request.header("mcp-protocol-version", PROTOCOL);
        }
        let request = request.build().map_err(|_| PluginCallOutcome::Invalid)?;
        let response = waiting(self.client.execute(request), self.flag, self.deadline)
            .await
            .map_err(stop)?;
        let headers = response.headers().iter().try_fold(0usize, |n, (k, v)| {
            n.checked_add(k.as_str().len() + v.as_bytes().len() + 4)
        });
        if headers.is_none_or(|n| n > MAX_HEADERS) {
            return Err(PluginCallOutcome::Invalid);
        }
        match response.status().as_u16() {
            401 => Err(PluginCallOutcome::LoginRequired),
            200..=299 => Ok(response),
            _ => Err(PluginCallOutcome::Unavailable),
        }
    }
    async fn body(
        &self,
        mut response: reqwest::Response,
        limit: usize,
    ) -> Result<Vec<u8>, PluginCallOutcome> {
        let mut body = Vec::new();
        while let Some(chunk) = waiting(response.chunk(), self.flag, self.deadline)
            .await
            .map_err(stop)?
        {
            if body.len() + chunk.len() > limit {
                return Err(PluginCallOutcome::Invalid);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
    /// The answer is either one JSON object or an SSE stream that ends at the message with our id.
    async fn exchange(
        &mut self,
        id: u64,
        message: &Value,
        limit: usize,
    ) -> Result<(Value, usize), PluginCallOutcome> {
        let initialize = message.get("method") == Some(&json!("initialize"));
        let response = self.send(message).await?;
        if initialize && let Some(v) = response.headers().get("mcp-session-id") {
            if v.is_empty()
                || v.len() > 1024
                || !v.as_bytes().iter().all(|b| (0x21..=0x7e).contains(b))
            {
                return Err(PluginCallOutcome::Invalid);
            }
            self.session = Some(v.clone());
        }
        let sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(|v| v.trim().to_ascii_lowercase());
        let message = match sse.as_deref() {
            Some("text/event-stream") => self.stream(response, id, limit).await?,
            Some("application/json") => {
                let body = self.body(response, limit).await?;
                let v: Value =
                    serde_json::from_slice(&body).map_err(|_| PluginCallOutcome::Invalid)?;
                (v, body.len())
            }
            _ => return Err(PluginCallOutcome::Invalid),
        };
        if initialize {
            self.initialized = true;
        }
        Ok(message)
    }
    /// SSE framing with bounded total bytes; notifications are skipped, a server request fails.
    async fn stream(
        &self,
        mut response: reqwest::Response,
        id: u64,
        limit: usize,
    ) -> Result<(Value, usize), PluginCallOutcome> {
        let (mut total, mut line, mut data) = (0usize, Vec::new(), Vec::new());
        loop {
            let Some(chunk) = waiting(response.chunk(), self.flag, self.deadline)
                .await
                .map_err(stop)?
            else {
                return Err(PluginCallOutcome::Invalid);
            };
            total += chunk.len();
            if total > limit {
                return Err(PluginCallOutcome::Invalid);
            }
            for &b in chunk.iter() {
                if b != b'\n' {
                    line.push(b);
                    continue;
                }
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.is_empty() {
                    if !data.is_empty() {
                        let v: Value = serde_json::from_slice(&data)
                            .map_err(|_| PluginCallOutcome::Invalid)?;
                        data.clear();
                        if answer(&v, id)? {
                            return Ok((v, total));
                        }
                    }
                } else if let Some(d) = line.strip_prefix(b"data:") {
                    if !data.is_empty() {
                        data.push(b'\n');
                    }
                    data.extend_from_slice(d.strip_prefix(b" ").unwrap_or(d));
                }
                line.clear();
            }
        }
    }
}
impl Wire for HttpWire<'_> {
    fn next_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
    fn request(
        &mut self,
        id: u64,
        message: &Value,
        limit: usize,
    ) -> Result<(Value, usize), PluginCallOutcome> {
        let runtime = self.runtime;
        runtime.block_on(self.exchange(id, message, limit))
    }
    fn notify(&mut self, message: &Value) -> Result<(), PluginCallOutcome> {
        let runtime = self.runtime;
        runtime.block_on(async {
            let response = self.send(message).await?;
            self.body(response, MAX_SMALL_BODY).await.map(|_| ())
        })
    }
}
/// Runs one remote MCP operation; only stored HTTPS URLs pass `allowed`.
fn operation<T>(
    url: &str,
    key: Option<&str>,
    flag: &AtomicBool,
    timeout: Duration,
    run: impl FnOnce(&mut Protocol<HttpWire<'_>>) -> Result<T, PluginCallOutcome>,
) -> Result<T, PluginCallOutcome> {
    if !allowed(url) {
        return Err(PluginCallOutcome::Invalid);
    }
    session(url, key, flag, timeout, run)
}
/// One HTTP MCP operation on its own runtime and client; both are dropped before return, so the
/// caller releases busy only after actual cleanup.
fn session<T>(
    url: &str,
    key: Option<&str>,
    flag: &AtomicBool,
    timeout: Duration,
    run: impl FnOnce(&mut Protocol<HttpWire<'_>>) -> Result<T, PluginCallOutcome>,
) -> Result<T, PluginCallOutcome> {
    if key.is_some_and(|k| {
        k.is_empty() || k.len() > 16384 || !k.bytes().all(|b| (33..=126).contains(&b))
    }) {
        return Err(PluginCallOutcome::Invalid);
    }
    let deadline = Instant::now() + timeout;
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Err(PluginCallOutcome::Unavailable);
    };
    let result = (|| {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| PluginCallOutcome::Unavailable)?;
        let mut p = Protocol(HttpWire {
            runtime: &runtime,
            client,
            url,
            key,
            flag,
            deadline,
            session: None,
            initialized: false,
            next: 0,
        });
        p.initialize()?;
        run(&mut p)
    })();
    drop(runtime);
    result
}
/// Core stored only HTTPS URLs; loopback HTTP exists only in test-support builds.
fn allowed(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            || (cfg!(any(test, feature = "model-test-support"))
                && u.scheme() == "http"
                && u.host_str() == Some("127.0.0.1"))
    })
}
/// The I/O part of one admitted call; Core keeps the permit itself.
pub(crate) struct McpCall<'a> {
    pub key: Option<&'a str>,
    pub tool: &'a str,
    pub arguments: &'a str,
    pub list_first: bool,
    pub pinned: [u8; 32],
    pub timeout: Duration,
    pub cancellation: &'a AtomicBool,
}
impl<'a> McpCall<'a> {
    pub(crate) fn of(permit: &'a ToolCallPermit, key: Option<&'a str>) -> Self {
        Self {
            key,
            tool: &permit.tool,
            arguments: &permit.arguments,
            list_first: permit.list_first,
            pinned: permit.pinned,
            timeout: permit.timeout,
            cancellation: &permit.cancellation,
        }
    }
}
pub(crate) fn result(
    r: Result<String, PluginCallOutcome>,
    listed: Option<BTreeMap<String, [u8; 32]>>,
) -> ToolCallResult {
    match r {
        Ok(text) => ToolCallResult {
            outcome: PluginCallOutcome::Completed,
            text,
            listed,
        },
        Err(outcome) => ToolCallResult {
            outcome,
            text: String::new(),
            listed,
        },
    }
}
/// One admitted call to a remote server.
pub(crate) fn call(url: &str, c: McpCall<'_>) -> ToolCallResult {
    let mut listed = None;
    let r = operation(url, c.key, c.cancellation, c.timeout, |p| {
        p.admitted_call(&c, &mut listed)
    });
    result(r, listed)
}
/// The managed server on loopback (ADR-022): only the managed branch builds this URL after it
/// chose the port, so `allowed` stays the rule for stored URLs. Stateless, so every operation
/// sends `initialize` again, like a remote server.
#[cfg(unix)]
fn loopback(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}
/// One admitted call to the managed server; `listed` is set when this call listed first.
#[cfg(unix)]
pub(crate) fn loopback_call(
    port: u16,
    c: &McpCall<'_>,
    listed: &mut Option<BTreeMap<String, [u8; 32]>>,
) -> Result<String, PluginCallOutcome> {
    session(&loopback(port), c.key, c.cancellation, c.timeout, |p| {
        p.admitted_call(c, listed)
    })
}
#[cfg(unix)]
pub(crate) fn loopback_list(
    port: u16,
    key: Option<&str>,
    flag: &AtomicBool,
    timeout: Duration,
) -> Result<Vec<Value>, PluginCallOutcome> {
    session(&loopback(port), key, flag, timeout, |p| p.list())
}
/// Stateless blocking discovery of a remote server: the raw tool objects.
pub(crate) fn list_remote(
    url: &str,
    key: Option<&str>,
    flag: &AtomicBool,
) -> Result<Vec<Value>, PluginCallOutcome> {
    operation(url, key, flag, DISCOVERY_TIMEOUT, |p| p.list())
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
