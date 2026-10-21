//! One bounded HTTP GET transport for built-in integrations. Core owns the cancel flag,
//! limits and every decision about the response; secrets live only in this transport.
use crate::wait::Stop;
use crate::{github_registry, jira_registry};
use ambient_core::{
    IntegrationHeaders, IntegrationKind, IntegrationRequest, IntegrationResponse, IntegrationStep,
    MAX_INTEGRATION_BODY,
};
use reqwest::header::{HeaderMap, HeaderName};
use std::{
    future::Future,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationTransportError {
    Invalid,
    Cancelled,
    Timeout,
    Transport,
    Limit,
}
impl std::fmt::Display for IntegrationTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for IntegrationTransportError {}

/// Response headers an adapter may receive; values and their limits are fixed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseHeader {
    Link,
    RetryAfter,
    RateRemaining,
    RateReset,
}
/// Endpoint registry of one adapter beside its URL builder: fixed request headers, extracted
/// response headers and the statuses whose oversized headers stay meaningful as ambiguous.
pub(crate) struct Registry {
    pub headers: &'static [(HeaderName, &'static str)],
    pub extracted: &'static [ResponseHeader],
    pub ambiguous_statuses: &'static [u16],
}
fn route(request: &IntegrationRequest) -> Option<(reqwest::Url, &'static Registry)> {
    match request {
        IntegrationRequest::GitHub(r) => {
            Some((github_registry::url(r)?, &github_registry::REGISTRY))
        }
        IntegrationRequest::Jira(r) => Some((
            jira_registry::url(&r.cloud_id, &r.issue, r.category, r.start_at, r.max_results)?,
            &jira_registry::REGISTRY,
        )),
    }
}

/// The adapter's authorization scheme. reqwest marks the resulting header sensitive.
enum Authorization {
    Basic { user: String, password: String },
    Bearer(String),
}
fn ascii_graphic(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|b| b.is_ascii_graphic())
}
/// Checks the secret form for a kind before any network work; secrets never enter Core.
fn authorization(
    kind: IntegrationKind,
    first: &str,
    second: Option<&str>,
) -> Result<Authorization, IntegrationTransportError> {
    match (kind, second) {
        (IntegrationKind::GitHub, None) if ascii_graphic(first, 16384) => {
            Ok(Authorization::Bearer(first.into()))
        }
        (IntegrationKind::Jira, Some(token))
            if ascii_graphic(first, 254) && !first.contains(':') && ascii_graphic(token, 16384) =>
        {
            Ok(Authorization::Basic {
                user: first.into(),
                password: token.into(),
            })
        }
        _ => Err(IntegrationTransportError::Invalid),
    }
}

/// Field drop order closes the client before runtime drop waits for actual DNS cleanup.
pub struct IntegrationTransport {
    client: reqwest::Client,
    authorization: Authorization,
    runtime: tokio::runtime::Runtime,
    flag: Arc<AtomicBool>,
    base: Option<reqwest::Url>,
}
impl IntegrationTransport {
    pub fn new(
        kind: IntegrationKind,
        first: &str,
        second: Option<&str>,
        flag: Arc<AtomicBool>,
    ) -> Result<Self, IntegrationTransportError> {
        Self::create(kind, first, second, flag, None)
    }
    fn create(
        kind: IntegrationKind,
        first: &str,
        second: Option<&str>,
        flag: Arc<AtomicBool>,
        base: Option<reqwest::Url>,
    ) -> Result<Self, IntegrationTransportError> {
        let authorization = authorization(kind, first, second)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| IntegrationTransportError::Transport)?;
        let client = runtime
            .block_on(async {
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .no_proxy()
                    .connect_timeout(Duration::from_secs(5))
                    .read_timeout(Duration::from_secs(10))
                    .build()
            })
            .map_err(|_| IntegrationTransportError::Transport)?;
        Ok(Self {
            client,
            authorization,
            runtime,
            flag,
            base,
        })
    }
    /// Test-only isolated loopback authority replacing the adapter's fixed one.
    #[cfg(any(test, feature = "model-test-support"))]
    pub fn test(
        kind: IntegrationKind,
        first: &str,
        second: Option<&str>,
        flag: Arc<AtomicBool>,
        base: &str,
    ) -> Result<Self, IntegrationTransportError> {
        let u = reqwest::Url::parse(base).map_err(|_| IntegrationTransportError::Invalid)?;
        if u.scheme() != "http"
            || u.host_str() != Some("127.0.0.1")
            || u.port().is_none()
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(IntegrationTransportError::Invalid);
        }
        Self::create(kind, first, second, flag, Some(u))
    }
    pub fn get(
        &mut self,
        step: IntegrationStep,
    ) -> Result<IntegrationResponse, IntegrationTransportError> {
        if step.remaining.is_zero() || step.remaining > Duration::from_secs(30) {
            return Err(IntegrationTransportError::Invalid);
        }
        let (mut url, registry) = route(&step.request).ok_or(IntegrationTransportError::Invalid)?;
        if let Some(base) = &self.base {
            url.set_scheme(base.scheme())
                .map_err(|_| IntegrationTransportError::Invalid)?;
            url.set_host(base.host_str())
                .map_err(|_| IntegrationTransportError::Invalid)?;
            url.set_port(base.port())
                .map_err(|_| IntegrationTransportError::Invalid)?;
        }
        let deadline = Instant::now()
            .checked_add(step.remaining)
            .ok_or(IntegrationTransportError::Invalid)?;
        self.runtime.block_on(async {
            let mut request = match &self.authorization {
                Authorization::Basic { user, password } => {
                    self.client.get(url).basic_auth(user, Some(password))
                }
                Authorization::Bearer(token) => self.client.get(url).bearer_auth(token),
            };
            for (name, value) in registry.headers {
                request = request.header(name, *value);
            }
            let request = request
                .timeout(step.remaining)
                .build()
                .map_err(|_| IntegrationTransportError::Invalid)?;
            let mut response = waiting(self.client.execute(request), &self.flag, deadline).await?;
            let status = response.status().as_u16();
            let headers = extract(response.headers(), status, registry)?;
            let mut body = Vec::new();
            if status == 200 {
                while let Some(chunk) = waiting(response.chunk(), &self.flag, deadline).await? {
                    if body
                        .len()
                        .checked_add(chunk.len())
                        .is_none_or(|n| n > MAX_INTEGRATION_BODY)
                    {
                        return Err(IntegrationTransportError::Limit);
                    }
                    body.extend_from_slice(&chunk);
                }
            }
            Ok(IntegrationResponse {
                status,
                body,
                headers,
            })
        })
    }
}
/// A header with several values, non-UTF-8 text or an oversized value is ambiguous, never guessed.
fn extract(
    map: &HeaderMap,
    status: u16,
    registry: &Registry,
) -> Result<IntegrationHeaders, IntegrationTransportError> {
    // This rejects accepted headers after Hyper parsing; it is not an allocation limit inside Hyper.
    let bytes = map
        .iter()
        .try_fold(0usize, |n, (k, v)| {
            n.checked_add(k.as_str().len() + v.as_bytes().len() + 4)
        })
        .ok_or(IntegrationTransportError::Limit)?;
    if bytes > 32768 {
        return if registry.ambiguous_statuses.contains(&status) {
            Ok(IntegrationHeaders {
                link_ambiguous: true,
                rate_ambiguous: true,
                ..IntegrationHeaders::default()
            })
        } else {
            Err(IntegrationTransportError::Limit)
        };
    }
    let mut h = IntegrationHeaders::default();
    for header in registry.extracted {
        let (name, max, value, ambiguous) = match header {
            ResponseHeader::Link => ("link", 8192, &mut h.link, &mut h.link_ambiguous),
            ResponseHeader::RetryAfter => (
                "retry-after",
                1024,
                &mut h.retry_after,
                &mut h.rate_ambiguous,
            ),
            ResponseHeader::RateRemaining => (
                "x-ratelimit-remaining",
                128,
                &mut h.remaining,
                &mut h.rate_ambiguous,
            ),
            ResponseHeader::RateReset => (
                "x-ratelimit-reset",
                128,
                &mut h.reset,
                &mut h.rate_ambiguous,
            ),
        };
        let mut values = map.get_all(name).iter();
        match (values.next(), values.next()) {
            (None, _) => {}
            (Some(v), None) => match v.to_str() {
                Ok(s) if s.len() <= max => *value = Some(s.into()),
                _ => *ambiguous = true,
            },
            _ => *ambiguous = true,
        }
    }
    Ok(h)
}
async fn waiting<T>(
    future: impl Future<Output = Result<T, reqwest::Error>>,
    flag: &AtomicBool,
    deadline: Instant,
) -> Result<T, IntegrationTransportError> {
    crate::wait::waiting(future, flag, deadline)
        .await
        .map_err(|stop| match stop {
            Stop::Cancelled => IntegrationTransportError::Cancelled,
            Stop::Timeout => IntegrationTransportError::Timeout,
            Stop::Failed => IntegrationTransportError::Transport,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ambient_core::{GitHubRequest, JiraRequest};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
    fn step(issue: &str, category: u32, start_at: i64, remaining: Duration) -> IntegrationStep {
        IntegrationStep {
            request: IntegrationRequest::Jira(JiraRequest {
                cloud_id: "Cloud-ID".into(),
                issue: issue.into(),
                category,
                start_at,
                max_results: 50,
            }),
            remaining,
        }
    }
    #[test]
    fn jira_secret_form_is_checked_before_any_network_without_guessing_prefix() {
        let jira = IntegrationKind::Jira;
        assert!(
            IntegrationTransport::new(jira, "fixture@example.invalid", Some("opaque"), flag())
                .is_ok()
        );
        for email in [
            "",
            "a:b",
            "space name",
            "é@example.invalid",
            &"e".repeat(255),
        ] {
            assert!(matches!(
                IntegrationTransport::new(jira, email, Some("token"), flag()),
                Err(IntegrationTransportError::Invalid)
            ));
        }
        for token in ["", "a b", "a\n", "é", &"t".repeat(16385)] {
            assert!(matches!(
                IntegrationTransport::new(jira, "e@example.invalid", Some(token), flag()),
                Err(IntegrationTransportError::Invalid)
            ));
        }
        // Jira needs both parts; a token-only form belongs to another adapter.
        assert!(matches!(
            IntegrationTransport::new(jira, "token-only", None, flag()),
            Err(IntegrationTransportError::Invalid)
        ));
    }
    #[test]
    fn jira_get_is_basic_bounded_and_never_redirects() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let route = format!("http://{}", listener.local_addr()?);
        let server = std::thread::spawn(move || -> std::io::Result<String> {
            let until = Instant::now() + Duration::from_secs(2);
            let mut socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => return Err(e),
                }
            };
            // The accepted socket inherits the listener's non-blocking mode on macOS; a read
            // before the request arrives would fail with WouldBlock instead of waiting.
            socket.set_nonblocking(false)?;
            socket.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut bytes = [0; 32768];
            let n = socket.read(&mut bytes)?;
            socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            Ok(String::from_utf8_lossy(&bytes[..n]).into_owned())
        });
        let mut t = IntegrationTransport::test(
            IntegrationKind::Jira,
            "fixture@example.invalid",
            Some("fixture-token"),
            flag(),
            &route,
        )?;
        let result = t.get(step("A/B", 1, 50, Duration::from_secs(1)));
        drop(t);
        let wire = server.join().map_err(|_| "server thread")?;
        let response = result?;
        assert_eq!(response.status, 302);
        let wire = wire?.to_ascii_lowercase();
        assert!(wire.starts_with("get /ex/jira/cloud-id/rest/api/3/issue/a%2fb/comment?startat=50&maxresults=50&orderby=-created http/1.1\r\n"));
        assert!(wire.contains(
            "authorization: basic zml4dhvyzublegftcgxllmludmfsawq6zml4dhvyzs10b2tlbg==\r\n"
        ));
        assert!(wire.contains("accept: application/json\r\n"));
        assert!(wire.contains("user-agent: ambient/0.1.0\r\n"));
        assert!(!wire.contains("cookie:") && !wire.contains("x-github"));
        Ok(())
    }
    #[test]
    fn transport_bounds_chunked_bodies_headers_and_ignores_error_bodies()
    -> Result<(), Box<dyn std::error::Error>> {
        for mode in [
            "body",
            "headers",
            "401-headers",
            "retry-duplicate",
            "error-body",
            "timeout",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let route = format!("http://{}", listener.local_addr()?);
            let server = std::thread::spawn(move || -> std::io::Result<()> {
                let (mut socket, _) = listener.accept()?;
                socket.set_read_timeout(Some(Duration::from_secs(2)))?;
                let mut input = [0; 32768];
                if socket.read(&mut input)? == 0 {
                    return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
                }
                let reply = match mode {
                    "body" => format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",MAX_INTEGRATION_BODY+1,"x".repeat(MAX_INTEGRATION_BODY+1)),
                    "headers" | "401-headers" => format!("HTTP/1.1 {}\r\nX-Large: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", if mode=="headers" {"200 OK"} else {"401 Unauthorized"},"x".repeat(40000)),
                    "retry-duplicate" => "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 10\r\nRetry-After: 20\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
                    "error-body" => "HTTP/1.1 503 Unavailable\r\nContent-Length: 999999999\r\nConnection: close\r\n\r\nsecret error body".into(),
                    _ => { std::thread::sleep(Duration::from_millis(150)); return Ok(()); }
                };
                // Oversized bodies can close the client before the fixture finishes writing.
                match socket.write_all(reply.as_bytes()) {
                    Ok(()) => Ok(()),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            });
            let mut transport = IntegrationTransport::test(
                IntegrationKind::Jira,
                "fixture@example.invalid",
                Some("fixture-token"),
                flag(),
                &route,
            )?;
            // Only the timeout mode needs a deadline shorter than the fixture's delay; the bound
            // checks get a normal deadline so a loaded machine cannot turn them into a timeout.
            let deadline = if mode == "timeout" {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(10)
            };
            let result = transport.get(step("ID", 0, 0, deadline));
            drop(transport);
            server.join().map_err(|_| "server thread")??;
            match mode {
                "body" | "headers" => {
                    assert!(
                        matches!(result, Err(IntegrationTransportError::Limit)),
                        "{mode}: {result:?}"
                    )
                }
                "timeout" => assert!(matches!(result, Err(IntegrationTransportError::Timeout))),
                _ => {
                    let response = result?;
                    assert!(response.body.is_empty());
                    assert_eq!(
                        response.status,
                        if mode == "401-headers" {
                            401
                        } else if mode == "retry-duplicate" {
                            429
                        } else {
                            503
                        }
                    );
                    assert_eq!(response.headers.rate_ambiguous, mode != "error-body");
                    assert_eq!(response.headers.retry_after, None);
                }
            }
        }
        Ok(())
    }

    fn github_step(category: u32, page: u32) -> IntegrationStep {
        IntegrationStep {
            request: IntegrationRequest::GitHub(GitHubRequest {
                owner: "Example".into(),
                repository: "Project".into(),
                number: 3,
                category,
                page,
            }),
            remaining: Duration::from_secs(2),
        }
    }
    /// Serves one blocking loopback request with `reply` and returns the request text.
    fn serve_once(
        reply: String,
    ) -> std::io::Result<(String, std::thread::JoinHandle<std::io::Result<String>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let route = format!("http://{}", listener.local_addr()?);
        let server = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut bytes = [0; 32768];
            let n = socket.read(&mut bytes)?;
            socket.write_all(reply.as_bytes())?;
            Ok(String::from_utf8_lossy(&bytes[..n]).into_owned())
        });
        Ok((route, server))
    }
    #[test]
    fn github_secret_form_is_a_single_graphic_token_checked_before_any_network() {
        let github = IntegrationKind::GitHub;
        assert!(IntegrationTransport::new(github, "opaque-token", None, flag()).is_ok());
        for token in ["", "a b", "a\n", "é", &"t".repeat(16385)] {
            assert!(matches!(
                IntegrationTransport::new(github, token, None, flag()),
                Err(IntegrationTransportError::Invalid)
            ));
        }
        // GitHub takes the token alone; a second part belongs to another adapter's form.
        assert!(matches!(
            IntegrationTransport::new(github, "opaque-token", Some("second"), flag()),
            Err(IntegrationTransportError::Invalid)
        ));
    }
    #[test]
    fn github_get_is_bearer_with_fixed_headers_and_never_redirects()
    -> Result<(), Box<dyn std::error::Error>> {
        let (route, server) = serve_once("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into())?;
        let mut t = IntegrationTransport::test(
            IntegrationKind::GitHub,
            "test-token",
            None,
            flag(),
            &route,
        )?;
        let result = t.get(github_step(1, 2));
        drop(t);
        let wire = server
            .join()
            .map_err(|_| "server thread")??
            .to_ascii_lowercase();
        assert_eq!(result?.status, 302);
        assert!(wire.starts_with(
            "get /repos/example/project/pulls/3/reviews?per_page=100&page=2 http/1.1\r\n"
        ));
        assert!(wire.contains("authorization: bearer test-token\r\n"));
        assert!(wire.contains("x-github-api-version: 2026-03-10\r\n"));
        assert!(wire.contains("accept: application/vnd.github+json\r\n"));
        assert!(wire.contains("user-agent: ambient/0.1.0\r\n"));
        assert!(!wire.contains("cookie:"));
        Ok(())
    }
    #[test]
    fn github_pages_and_categories_outside_the_registry_are_refused_before_network()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut t = IntegrationTransport::test(
            IntegrationKind::GitHub,
            "test-token",
            None,
            flag(),
            "http://127.0.0.1:9/",
        )?;
        for (category, page) in [(1, 0), (1, 6), (4, 1)] {
            assert!(matches!(
                t.get(github_step(category, page)),
                Err(IntegrationTransportError::Invalid)
            ));
        }
        Ok(())
    }
    #[test]
    fn github_link_and_rate_headers_are_extracted_or_marked_ambiguous()
    -> Result<(), Box<dyn std::error::Error>> {
        for (reply, expected) in [
            (
                "HTTP/1.1 429 Too Many Requests\r\nLink: <https://api.github.com/x?page=2>; rel=\"next\"\r\nRetry-After: 30\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 999\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                IntegrationHeaders {
                    link: Some("<https://api.github.com/x?page=2>; rel=\"next\"".into()),
                    retry_after: Some("30".into()),
                    remaining: Some("0".into()),
                    reset: Some("999".into()),
                    ..IntegrationHeaders::default()
                },
            ),
            (
                "HTTP/1.1 429 Too Many Requests\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Remaining: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                IntegrationHeaders {
                    rate_ambiguous: true,
                    ..IntegrationHeaders::default()
                },
            ),
            (
                format!("HTTP/1.1 200 OK\r\nLink: {}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]", "l".repeat(8193)),
                IntegrationHeaders {
                    link_ambiguous: true,
                    ..IntegrationHeaders::default()
                },
            ),
            (
                format!("HTTP/1.1 403 Forbidden\r\nX-Large: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", "x".repeat(40000)),
                IntegrationHeaders {
                    link_ambiguous: true,
                    rate_ambiguous: true,
                    ..IntegrationHeaders::default()
                },
            ),
        ] {
            let (route, server) = serve_once(reply)?;
            let mut t = IntegrationTransport::test(
                IntegrationKind::GitHub,
                "test-token",
                None,
                flag(),
                &route,
            )?;
            let result = t.get(github_step(0, 1));
            drop(t);
            let wire = server.join().map_err(|_| "server thread")??.to_ascii_lowercase();
            assert!(wire.starts_with("get /repos/example/project/pulls/3 http/1.1\r\n"));
            assert_eq!(result?.headers, expected);
        }
        Ok(())
    }
}
