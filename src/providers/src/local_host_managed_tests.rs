//! The managed server against the test-only stateless HTTP imitation of workspace-mcp
//! (`tests/fixtures/workspace_server.py`), one ADR-022 «Testing Decisions» case per behaviour.
use super::*;
use crate::mcp::McpCall;
use ambient_core::canonical_tool;
use serde_json::json;
use std::sync::atomic::AtomicU64;

const SERVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/workspace_server.py"
);
const GOOGLE: ManagedServerId = ManagedServerId::GoogleWorkspace;
const TOKEN: &str = "ya29.test-access-token";
static NEXT: AtomicU64 = AtomicU64::new(0);
static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One managed server at a time across every test thread and process: a server binds its port
/// a second after the probe, and parallel test processes would otherwise meet on one port (the
/// documented same-user race), so this file lock serialises them.
fn serial() -> std::io::Result<std::fs::File> {
    let lock = std::fs::File::create(std::env::temp_dir().join("ambient-managed-tests.lock"))?;
    lock.lock()?;
    Ok(lock)
}
/// A plugins root whose installed venv `bin/python` runs the fixture in `mode` with the
/// recipe's arguments, as the real interpreter would run `-m main`.
struct Root(PathBuf);
impl Root {
    fn installed(mode: &str) -> Result<Self> {
        let root = Self::empty();
        let bin = crate::managed::venv(&root.0, GOOGLE).join("bin");
        std::fs::create_dir_all(&bin)?;
        let python = bin.join("python");
        std::fs::write(
            &python,
            format!("#!/bin/sh\nexec /usr/bin/python3 {SERVER} {mode} \"$@\"\n"),
        )?;
        let mut perms = std::fs::metadata(&python)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&python, perms)?;
        Ok(root)
    }
    fn empty() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-managed-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn private(&self, plugin: u64) -> PathBuf {
        self.0.join(plugin.to_string())
    }
    fn starts(&self, plugin: u64) -> Vec<i32> {
        std::fs::read_to_string(self.private(plugin).join("starts"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.parse().ok())
            .collect()
    }
    fn launch(&self, plugin: u64) -> Result<serde_json::Value> {
        Ok(serde_json::from_str(&std::fs::read_to_string(
            self.private(plugin).join("launch.json"),
        )?)?)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn pinned(name: &str) -> [u8; 32] {
    canonical_tool(&json!({"name":name,"description":name,"inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}}))
        .map(|t| t.hash)
        .unwrap_or_default()
}
fn call<'a>(
    tool: &'a str,
    list_first: bool,
    key: Option<&'a str>,
    flag: &'a AtomicBool,
) -> McpCall<'a> {
    McpCall {
        key,
        tool,
        arguments: r#"{"query":"plan"}"#,
        list_first,
        pinned: pinned(tool),
        timeout: Duration::from_secs(5),
        cancellation: flag,
    }
}
fn run(host: &mut LocalHost, plugin: u64, c: &McpCall<'_>) -> ToolCallResult {
    host.managed_call((plugin, "google".into()), GOOGLE, c)
}
fn group_gone(pid: i32) -> bool {
    Pid::from_raw(pid).is_some_and(|p| test_kill_process_group(p) == Err(rustix::io::Errno::SRCH))
}

#[test]
fn the_managed_server_starts_once_on_loopback_with_the_recipe_and_gets_the_bearer() -> Result {
    let _serial = serial()?;
    let root = Root::installed("ok")?;
    let mut host = LocalHost::new(&root.0);
    let r = run(
        &mut host,
        7,
        &call("search_docs", true, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed, "{r:?}");
    assert_eq!(r.text, r#"found {"query": "plan"}"#);
    assert!(
        r.listed
            .is_some_and(|l| l.contains_key("get_drive_file_download_url"))
    );
    let r = run(
        &mut host,
        7,
        &call("search_docs", false, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed);
    let starts = root.starts(7);
    assert_eq!(starts.len(), 1, "one process for the request");
    let launch = root.launch(7)?;
    assert_eq!(
        launch["argv"],
        json!([
            "-P",
            "-m",
            "main",
            "--transport",
            "streamable-http",
            "--read-only",
            "--tools",
            "drive",
            "docs"
        ])
    );
    let env = launch["env"].as_object().ok_or("env")?;
    let private = root.private(7);
    assert_eq!(env["HOME"], private.to_string_lossy().as_ref());
    assert_eq!(env["TMPDIR"], private.to_string_lossy().as_ref());
    assert_eq!(
        launch["cwd"],
        private.canonicalize()?.to_string_lossy().as_ref()
    );
    for (k, v) in [
        ("MCP_ENABLE_OAUTH21", "true"),
        ("EXTERNAL_OAUTH21_PROVIDER", "true"),
        ("WORKSPACE_MCP_STATELESS_MODE", "true"),
        ("WORKSPACE_MCP_HOST", "127.0.0.1"),
        ("WORKSPACE_MCP_MAX_FILE_BYTES", "131072"),
        ("WORKSPACE_MCP_DISABLE_LOCAL_FILES", "true"),
        (
            "GOOGLE_OAUTH_CLIENT_ID",
            "test-client.apps.googleusercontent.com",
        ),
    ] {
        assert_eq!(env[k], v, "{k}");
    }
    let port: u16 = env["WORKSPACE_MCP_PORT"].as_str().ok_or("port")?.parse()?;
    assert!(port >= 1024);
    assert!(
        env["FASTMCP_SERVER_AUTH_GOOGLE_JWT_SIGNING_KEY"]
            .as_str()
            .is_some_and(|k| k.len() >= 43)
    );
    // The access token travels only as the Bearer header; no client secret and no PORT.
    let text = launch.to_string();
    assert!(!text.contains(TOKEN) && !text.contains("test-secret"));
    assert!(!env.contains_key("PORT") && !env.keys().any(|k| k.starts_with("OTEL_")));
    let requests = std::fs::read_to_string(private.join("requests"))?;
    assert!(
        requests
            .lines()
            .all(|l| l.ends_with(&format!("Bearer {TOKEN}")))
    );
    // Stateless: every operation initializes again.
    assert_eq!(
        requests
            .lines()
            .filter(|l| l.starts_with("initialize "))
            .count(),
        2
    );
    assert!(host.finish().is_empty());
    assert!(group_gone(starts[0]));
    Ok(())
}

#[test]
fn an_oversized_answer_fails_only_that_call_and_the_server_keeps_running() -> Result {
    let _serial = serial()?;
    let root = Root::installed("big")?;
    let mut host = LocalHost::new(&root.0);
    let r = run(
        &mut host,
        8,
        &call("search_docs", true, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Invalid);
    // Not Failed: the next call reaches the same process.
    let r = run(
        &mut host,
        8,
        &call("search_docs", false, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Invalid);
    assert_eq!(root.starts(8).len(), 1);
    assert!(host.finish().is_empty());
    Ok(())
}

#[test]
fn without_install_or_sign_in_nothing_starts() -> Result {
    let _serial = serial()?;
    let root = Root::empty();
    let mut host = LocalHost::new(&root.0);
    let r = run(
        &mut host,
        9,
        &call("search_docs", true, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Unavailable);
    let root = Root::installed("ok")?;
    let mut host = LocalHost::new(&root.0);
    let r = run(
        &mut host,
        9,
        &call("search_docs", true, None, &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::LoginRequired);
    assert!(root.starts(9).is_empty());
    // A rejected token is a sign-in problem of this call, not a dead server.
    let r = run(
        &mut host,
        9,
        &call(
            "search_docs",
            true,
            Some("not-a-google-token"),
            &NOT_CANCELLED,
        ),
    );
    assert_eq!(r.outcome, PluginCallOutcome::LoginRequired);
    let r = run(
        &mut host,
        9,
        &call("search_docs", true, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed);
    assert_eq!(root.starts(9).len(), 1);
    Ok(())
}

#[test]
fn an_early_exit_or_an_occupied_port_fails_and_is_never_restarted() -> Result {
    let _serial = serial()?;
    let root = Root::installed("exit-early")?;
    let mut host = LocalHost::new(&root.0);
    let flag = AtomicBool::new(false);
    let (r, tail) = match host.with_managed(
        (10, "google".into()),
        GOOGLE,
        Some(TOKEN),
        &flag,
        Instant::now() + LOCAL_START,
        |_| Ok(()),
    ) {
        Ok(()) => (PluginCallOutcome::Completed, None),
        Err((o, t)) => (o, t),
    };
    assert_eq!(r, PluginCallOutcome::Unavailable);
    assert!(tail.is_some_and(|t| t.contains("failed to start")));
    let r = run(
        &mut host,
        10,
        &call("search_docs", true, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Unavailable);
    assert_eq!(root.starts(10).len(), 1, "Failed is never restarted");
    // Someone else holds the port: the server cannot bind, and whatever answers there is not
    // a usable MCP server. The call fails and no process is left.
    let taken = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = taken.local_addr()?.port();
    // Accepts and closes every connection; the thread ends with the test process.
    std::thread::spawn(move || {
        for s in taken.incoming() {
            drop(s);
        }
    });
    let root = Root::installed("ok")?;
    let mut host = LocalHost::with_port(&root.0, port);
    let r = run(
        &mut host,
        11,
        &call("search_docs", true, Some(TOKEN), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Unavailable);
    // The stop was confirmed (a fast failure may end it before it even wrote its pid).
    assert!(host.finish().is_empty());
    assert!(root.starts(11).into_iter().all(group_gone));
    Ok(())
}

#[test]
fn cancel_during_the_start_ends_the_process_before_return() -> Result {
    let _serial = serial()?;
    let root = Root::installed("slow-start")?;
    let mut host = LocalHost::new(&root.0);
    let flag = Arc::new(AtomicBool::new(false));
    let stop = flag.clone();
    // Cancel once the program is running, while it has not bound its port yet.
    let pids = root.private(12).join("starts");
    let canceller = thread::spawn(move || {
        let waited = Instant::now();
        // The file exists before the pid is in it: wait for the written line.
        while std::fs::read_to_string(&pids).map_or(true, |t| !t.ends_with('\n'))
            && waited.elapsed() < Duration::from_secs(10)
        {
            thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Release);
    });
    let started = Instant::now();
    let r = run(
        &mut host,
        12,
        &call("search_docs", true, Some(TOKEN), &flag),
    );
    canceller.join().map_err(|_| "canceller")?;
    assert_eq!(r.outcome, PluginCallOutcome::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(10));
    let starts = root.starts(12);
    assert_eq!(starts.len(), 1);
    assert!(group_gone(starts[0]));
    Ok(())
}

#[test]
fn discovery_starts_lists_and_stops_the_managed_server() -> Result {
    let _serial = serial()?;
    let root = Root::installed("ok")?;
    let d = discover(
        &root.0,
        13,
        "google",
        &Target::Managed(GOOGLE),
        Some(TOKEN),
        &NOT_CANCELLED,
    );
    let names = d
        .listed
        .map_err(|o| format!("{o:?}"))?
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert!(names.contains(&"search_drive_files".to_owned()));
    assert!(d.stop_confirmed);
    let starts = root.starts(13);
    assert!(group_gone(starts[0]));
    let d = discover(
        &root.0,
        14,
        "google",
        &Target::Managed(GOOGLE),
        None,
        &NOT_CANCELLED,
    );
    assert_eq!(d.listed, Err(PluginCallOutcome::LoginRequired));
    Ok(())
}

/// Manual acceptance on a Mac with uv (ADR-022 «Ручные проверки приёмки 24B»), never in CI:
/// `AMBIENT_MANUAL_MANAGED_ROOT=<empty folder> cargo test -p ambient-providers --lib manual_ -- --ignored --nocapture`.
/// The real install through the product path, then the real server started by the recipe: where
/// it listens, whether the LAN address answers, cold start, and stop. No Google call: the start
/// sends no request, so the placeholder token never leaves this Mac.
#[test]
#[ignore = "manual: real uv, PyPI and GitHub downloads"]
fn manual_real_install_and_start_on_this_mac() -> Result {
    let root = PathBuf::from(std::env::var("AMBIENT_MANUAL_MANAGED_ROOT")?);
    let started = Instant::now();
    let installed = crate::managed::managed_install(&root, GOOGLE, &NOT_CANCELLED);
    println!(
        "install {:?} in {:.1} s",
        installed,
        started.elapsed().as_secs_f64()
    );
    assert_eq!(
        installed.status,
        crate::managed_types::InstallStatus::Installed
    );
    let mut host = LocalHost::new(&root);
    let started = Instant::now();
    let r = host.with_managed(
        (1, "google".into()),
        GOOGLE,
        Some("ya29.manual-placeholder"),
        &NOT_CANCELLED,
        Instant::now() + LOCAL_START,
        |port| {
            println!(
                "cold start to accepting {:.2} s on port {port}",
                started.elapsed().as_secs_f64()
            );
            let lsof = std::process::Command::new("/usr/sbin/lsof")
                .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
                .output()
                .map_err(|_| PluginCallOutcome::Unavailable)?;
            println!("{}", String::from_utf8_lossy(&lsof.stdout));
            let lan = std::process::Command::new("/usr/sbin/ipconfig")
                .args(["getifaddr", "en0"])
                .output()
                .map_err(|_| PluginCallOutcome::Unavailable)?;
            let lan = String::from_utf8_lossy(&lan.stdout).trim().to_owned();
            let answered = lan.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| {
                std::net::TcpStream::connect_timeout(&(ip, port).into(), Duration::from_secs(2))
                    .is_ok()
            });
            println!("LAN address {lan} answers: {answered}");
            Ok((port, answered))
        },
    );
    let (port, answered) = r.map_err(|(o, tail)| format!("{o:?} {tail:?}"))?;
    assert!(!answered, "the server must not answer on the LAN address");
    assert!(host.finish().is_empty(), "stop confirmed");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
    Ok(())
}

/// Outgoing connections, the readiness probe's own included, take their local port from the
/// system's ephemeral range. A server port from that range lets a probe connect to itself and
/// hold the port the server is about to bind (seen under load: «Address already in use»).
#[test]
fn the_server_port_is_never_one_the_system_gives_to_outgoing_connections() -> Result {
    let sysctl = |name: &str| -> std::result::Result<u16, Box<dyn std::error::Error>> {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", name])
            .output()?;
        Ok(String::from_utf8(out.stdout)?.trim().parse()?)
    };
    let ephemeral = sysctl("net.inet.ip.portrange.first")?..=sysctl("net.inet.ip.portrange.last")?;
    for _ in 0..200 {
        let port = free_port().map_err(|o| format!("{o:?}"))?;
        assert!(!ephemeral.contains(&port), "{port} is in {ephemeral:?}");
    }
    Ok(())
}
