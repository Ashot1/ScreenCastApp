use crate::{AuthError, endpoints};
use openidconnect::{HttpRequest, HttpResponse, SyncHttpClient};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) const RESPONSE_LIMIT: usize = 512 * 1024;
pub(crate) struct Transport {
    allowed: Vec<(String, String)>,
    used: AtomicU8,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    timeout: Duration,
    read_timeout: Duration,
    #[cfg(test)]
    resolver: Option<Arc<tests::SlowResolver>>,
}
impl Transport {
    pub(crate) fn new(
        preparing: bool,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Self, AuthError> {
        let allowed = if preparing {
            vec![
                ("GET".into(), endpoints::DISCOVERY.into()),
                ("GET".into(), endpoints::JWKS.into()),
            ]
        } else {
            vec![("POST".into(), endpoints::TOKEN.into())]
        };
        Self::routes(allowed, cancelled, deadline)
    }
    /// A one-shot transport for exactly these routes (each used at most once), e.g. the token
    /// endpoint of one OAuth operation; every operation builds its own.
    pub(crate) fn routes(
        allowed: Vec<(String, String)>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Self, AuthError> {
        Self::build(allowed, cancelled, deadline, Duration::from_secs(15))
    }
    fn build(
        allowed: Vec<(String, String)>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        timeout: Duration,
    ) -> Result<Self, AuthError> {
        Ok(Self {
            allowed,
            used: AtomicU8::new(0),
            cancelled,
            deadline,
            timeout,
            read_timeout: Duration::from_secs(5),
            #[cfg(test)]
            resolver: None,
        })
    }
    fn check(&self) -> Result<(), AuthError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(AuthError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(AuthError::Expired)
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    fn local(url: String, timeout: Duration) -> Result<Self, AuthError> {
        Self::build(
            vec![("POST".into(), url)],
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(10),
            timeout,
        )
    }
    #[cfg(test)]
    pub(crate) fn test_routes(allowed: Vec<(String, String)>) -> Result<Self, AuthError> {
        Self::build(
            allowed,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(10),
            Duration::from_secs(2),
        )
    }
}
impl SyncHttpClient for Transport {
    type Error = AuthError;
    fn call(&self, request: HttpRequest) -> Result<HttpResponse, AuthError> {
        self.check()?;
        let index = self
            .allowed
            .iter()
            .position(|(m, u)| request.method().as_str() == m && request.uri().to_string() == *u)
            .ok_or(AuthError::InvalidInput)?;
        if request.body().len() > 64 * 1024
            || (request.method() == "GET"
                && (!request.body().is_empty() || request.headers().contains_key("authorization")))
        {
            return Err(AuthError::ResourceLimit);
        }
        if self.used.fetch_or(1 << index, Ordering::AcqRel) & (1 << index) != 0 {
            return Err(AuthError::InvalidState);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| AuthError::Internal)?;
        // Normal runtime drop waits for internal blocking work, including DNS cleanup.
        runtime.block_on(async {
            let remaining = self
                .deadline
                .saturating_duration_since(Instant::now())
                .min(self.timeout);
            let builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .no_proxy()
                .timeout(remaining)
                .connect_timeout(Duration::from_secs(5))
                .read_timeout(self.read_timeout);
            #[cfg(test)]
            let builder = if let Some(resolver) = &self.resolver {
                builder.dns_resolver(resolver.clone())
            } else {
                builder
            };
            let client = builder.build().map_err(|_| AuthError::Internal)?;
            let request: reqwest::Request =
                request.try_into().map_err(|_| AuthError::InvalidInput)?;
            self.check()?;
            let mut response = client
                .execute(request)
                .await
                .map_err(|_| AuthError::TransportUnknown)?;
            if response.status().is_redirection() {
                return Err(AuthError::InvalidResponse);
            }
            let mut builder = openidconnect::http::Response::builder().status(response.status());
            for (name, value) in response.headers() {
                builder = builder.header(name, value);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| AuthError::TransportUnknown)?
            {
                self.check()?;
                if bytes
                    .len()
                    .checked_add(chunk.len())
                    .filter(|n| *n <= RESPONSE_LIMIT)
                    .is_none()
                {
                    return Err(AuthError::ResourceLimit);
                }
                bytes.extend_from_slice(&chunk);
            }
            self.check()?;
            builder.body(bytes).map_err(|_| AuthError::Internal)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidconnect::http;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn server(body: Vec<u8>) -> Result<(String, thread::JoinHandle<usize>), std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/token", listener.local_addr()?);
        let worker = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return 0;
            };
            let mut input = [0; 2048];
            let _ = stream.read(&mut input);
            let _ = stream.write_all(&body);
            drop(stream);
            if listener.set_nonblocking(true).is_err() {
                return 0;
            }
            thread::sleep(Duration::from_millis(70));
            if listener.accept().is_ok() { 2 } else { 1 }
        });
        Ok((url, worker))
    }
    fn request(url: &str) -> Result<HttpRequest, openidconnect::http::Error> {
        http::Request::builder()
            .uri(url)
            .method("POST")
            .body(vec![])
    }
    #[test]
    fn chunked_limit_is_not_eof_and_post_never_retries_or_follows_redirects()
    -> Result<(), Box<dyn std::error::Error>> {
        for size in [RESPONSE_LIMIT, RESPONSE_LIMIT + 1] {
            let mut bytes=format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",size).into_bytes();
            bytes.extend(std::iter::repeat_n(b'x', size));
            bytes.extend_from_slice(b"\r\n0\r\n\r\n");
            let (url, worker) = server(bytes)?;
            let transport = Transport::local(url.clone(), Duration::from_secs(2))?;
            let result = transport.call(request(&url)?);
            if size == RESPONSE_LIMIT {
                assert_eq!(result?.body().len(), RESPONSE_LIMIT);
            } else {
                assert!(matches!(result, Err(AuthError::ResourceLimit)));
            }
            assert!(matches!(
                transport.call(request(&url)?),
                Err(AuthError::InvalidState)
            ));
            assert_eq!(worker.join().map_err(|_| "worker")?, 1);
        }
        for body in [b"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/redirect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),vec![]] {
            let (url,worker)=server(body)?;let transport=Transport::local(url.clone(),Duration::from_secs(1))?;
            assert!(transport.call(request(&url)?).is_err());
            assert_eq!(worker.join().map_err(|_|"worker")?,1);
        }
        Ok(())
    }
    #[test]
    fn stalled_read_and_slow_trickle_have_separate_deadlines()
    -> Result<(), Box<dyn std::error::Error>> {
        for trickle in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let url = format!("http://{}/token", listener.local_addr()?);
            let worker = thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut input = [0; 2048];
                let _ = stream.read(&mut input);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n",
                );
                for _ in 0..20 {
                    thread::sleep(Duration::from_millis(if trickle { 20 } else { 250 }));
                    if stream.write_all(b"x").is_err() {
                        break;
                    }
                }
            });
            let mut transport = Transport::local(
                url.clone(),
                Duration::from_millis(if trickle { 150 } else { 1000 }),
            )?;
            transport.read_timeout = Duration::from_millis(70);
            let start = Instant::now();
            assert!(matches!(
                transport.call(request(&url)?),
                Err(AuthError::TransportUnknown)
            ));
            assert!(start.elapsed() < Duration::from_millis(750));
            worker.join().map_err(|_| "worker")?;
        }
        Ok(())
    }
    pub(super) struct SlowResolver;
    impl reqwest::dns::Resolve for SlowResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            Box::pin(async {
                tokio::task::spawn_blocking(|| {
                    thread::sleep(Duration::from_millis(250));
                    Err::<reqwest::dns::Addrs, std::io::Error>(std::io::Error::other(
                        "test resolver",
                    ))
                })
                .await
                .map_err(|_| std::io::Error::other("test join"))?
                .map_err(Into::into)
            })
        }
    }
    #[test]
    fn request_timeout_waits_for_actual_dns_cleanup_before_return()
    -> Result<(), Box<dyn std::error::Error>> {
        let url = "http://only-test.invalid/token".to_owned();
        let mut transport = Transport::local(url.clone(), Duration::from_millis(50))?;
        transport.resolver = Some(Arc::new(SlowResolver));
        let start = Instant::now();
        assert!(transport.call(request(&url)?).is_err());
        assert!(start.elapsed() >= Duration::from_millis(225));
        assert!(start.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn bounded_transport_reads_a_real_http_response() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/token", listener.local_addr()?);
        let worker = thread::spawn(move || -> std::io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut input = [0; 2048];
            let _ = stream.read(&mut input)?;
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
        });
        let request = http::Request::builder()
            .uri(&url)
            .method("POST")
            .body(vec![])?;
        let transport = Transport::local(url.clone(), std::time::Duration::from_secs(2))?;
        let response = transport.call(request);
        assert!(response.is_ok());
        worker.join().map_err(|_| "worker")??;
        assert_eq!(response?.body(), b"{}");
        Ok(())
    }
}
