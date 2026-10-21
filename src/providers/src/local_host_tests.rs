//! Local host against the test-only stdio server (`tests/fixtures/stdio_server.py`), one
//! scenario of ADR-022 «Testing Decisions» per misbehaviour.
use super::*;
use crate::mcp::McpCall;
use ambient_core::canonical_tool;
use rustix::process::test_kill_process;
use serde_json::json;
use std::sync::atomic::AtomicU64;

const SERVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stdio_server.py"
);
static NEXT: AtomicU64 = AtomicU64::new(0);
static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A private data root per test, removed afterwards.
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ambient-local-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(p)
    }
    /// Pids of every start of plugin `plugin`'s servers, in order.
    fn starts(&self, plugin: u64) -> Vec<i32> {
        std::fs::read_to_string(self.0.join(plugin.to_string()).join("starts"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.parse().ok())
            .collect()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn args(mode: &str) -> Vec<String> {
    vec![SERVER.into(), mode.into()]
}
const PYTHON: &str = "/usr/bin/python3";
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
        arguments: r#"{"q":"plan"}"#,
        list_first,
        pinned: pinned(tool),
        timeout: Duration::from_secs(5),
        cancellation: flag,
    }
}
fn gone(pid: i32) -> bool {
    Pid::from_raw(pid).is_some_and(|p| test_kill_process(p) == Err(rustix::io::Errno::SRCH))
}
fn group_gone(pid: i32) -> bool {
    Pid::from_raw(pid).is_some_and(|p| test_kill_process_group(p) == Err(rustix::io::Errno::SRCH))
}
fn run(host: &mut LocalHost, plugin: u64, mode: &str, c: &McpCall<'_>) -> ToolCallResult {
    host.local_call((plugin, "local".into()), (PYTHON, &args(mode), &[]), c)
}

#[test]
fn a_server_starts_once_lists_first_and_stops_when_the_host_finishes() -> Result {
    let root = Root::new();
    let mut host = LocalHost::new(&root.0);
    let r = run(
        &mut host,
        1,
        "ok",
        &call("echo", true, None, &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed);
    assert_eq!(r.text, r#"echo:{"q": "plan"}"#);
    assert_eq!(r.listed.as_ref().map(|l| l.len()), Some(3));
    let r = run(
        &mut host,
        1,
        "ok",
        &call("pid", false, None, &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed);
    assert!(r.listed.is_none());
    // One process for the request: initialize was sent once, to the first start.
    let starts = root.starts(1);
    assert_eq!(starts.len(), 1);
    let pid: Value = serde_json::from_str(&r.text)?;
    assert_eq!(pid["leader"], starts[0]);
    // A changed live definition is never called, and the server keeps running.
    let mut changed = call("echo", true, None, &NOT_CANCELLED);
    changed.pinned = [7; 32];
    assert_eq!(
        run(&mut host, 1, "ok", &changed).outcome,
        PluginCallOutcome::Changed
    );
    assert!(!gone(starts[0]));
    assert_eq!(host.finish(), vec![]);
    assert!(gone(starts[0]) && group_gone(starts[0]));
    Ok(())
}

#[test]
fn the_environment_is_cleared_with_a_fixed_path_private_folders_and_secrets_never_in_argv() -> Result
{
    let root = Root::new();
    let mut host = LocalHost::new(&root.0);
    let names = vec!["API_TOKEN".to_owned()];
    let key = r#"{"API_TOKEN":"s3cr3t-value"}"#;
    let r = host.local_call(
        (2, "local".into()),
        (PYTHON, &args("ok"), &names),
        &call("env", false, Some(key), &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed, "{}", r.text);
    let v: Value = serde_json::from_str(&r.text)?;
    let folder = root.0.join("2");
    let home = std::env::home_dir().ok_or("home")?;
    let env = v["env"].as_object().ok_or("env")?;
    assert_eq!(env["PATH"], path(&home));
    assert!(env["PATH"].as_str().is_some_and(|p| !p.contains('~')));
    assert_eq!(env["HOME"], folder.display().to_string());
    assert_eq!(env["TMPDIR"], folder.display().to_string());
    assert_eq!(env["API_TOKEN"], "s3cr3t-value");
    // Nothing of this test process's environment is inherited. The /usr/bin/python3 shim adds
    // developer-tool variables and CoreFoundation sets __CF_USER_TEXT_ENCODING in every process,
    // so inherited name/value pairs are checked instead of an exact list.
    // Any variable of this process proves it (cargo's own are absent when the binary runs alone).
    let own = ["PATH", "HOME", "TMPDIR", "__CF_USER_TEXT_ENCODING"];
    let inherited = std::env::vars()
        .filter(|(name, _)| !own.contains(&name.as_str()))
        .collect::<Vec<_>>();
    assert!(!inherited.is_empty());
    for (name, value) in inherited {
        assert_ne!(env.get(&name), Some(&Value::String(value)), "{name}");
    }
    let cwd = std::fs::canonicalize(v["cwd"].as_str().ok_or("cwd")?)?;
    assert_eq!(cwd, std::fs::canonicalize(&folder)?);
    assert!(!v["argv"].to_string().contains("s3cr3t-value"));
    assert_eq!(host.finish(), vec![]);
    Ok(())
}

#[test]
fn stop_ends_servers_that_ignore_eof_or_sigterm_and_a_same_group_grandchild() -> Result {
    for mode in ["ignore-eof", "ignore-term", "grandchild"] {
        let root = Root::new();
        let mut host = LocalHost::new(&root.0);
        let r = run(
            &mut host,
            3,
            mode,
            &call("pid", false, None, &NOT_CANCELLED),
        );
        assert_eq!(r.outcome, PluginCallOutcome::Completed, "{mode}");
        let pids: Value = serde_json::from_str(&r.text)?;
        let leader = pids["leader"].as_i64().ok_or("leader")? as i32;
        let started = Instant::now();
        assert_eq!(host.finish(), vec![], "{mode}");
        let took = started.elapsed();
        assert!(gone(leader) && group_gone(leader), "{mode}");
        if let Some(grandchild) = pids["grandchild"].as_i64() {
            assert!(gone(grandchild as i32) || group_gone(leader), "{mode}");
        }
        if mode == "ignore-term" {
            // SIGTERM was ignored: SIGKILL came after the 300 ms wait.
            assert!(took >= Duration::from_millis(300), "{took:?}");
        }
        assert!(took < Duration::from_secs(3), "{mode} {took:?}");
    }
    Ok(())
}

#[test]
fn wire_failures_are_invalid_or_unavailable_end_the_process_and_never_respawn() -> Result {
    for (mode, tool, outcome) in [
        ("garbage", "echo", PluginCallOutcome::Invalid),
        ("long-line", "echo", PluginCallOutcome::Invalid),
        ("exit-early", "echo", PluginCallOutcome::Unavailable),
        ("server-request", "echo", PluginCallOutcome::Invalid),
        ("close-stdin", "echo", PluginCallOutcome::Unavailable),
    ] {
        let root = Root::new();
        let mut host = LocalHost::new(&root.0);
        let started = Instant::now();
        let r = run(&mut host, 4, mode, &call(tool, true, None, &NOT_CANCELLED));
        assert_eq!(r.outcome, outcome, "{mode}");
        assert!(started.elapsed() < Duration::from_secs(5), "{mode}");
        let starts = root.starts(4);
        assert_eq!(starts.len(), 1, "{mode}");
        assert!(
            group_gone(starts[0]),
            "{mode}: stopped before the call returned"
        );
        let again = run(&mut host, 4, mode, &call(tool, false, None, &NOT_CANCELLED));
        assert_eq!(again.outcome, PluginCallOutcome::Unavailable, "{mode}");
        assert_eq!(root.starts(4).len(), 1, "{mode}: Failed is never restarted");
        assert_eq!(host.finish(), vec![], "{mode}");
    }
    Ok(())
}

#[test]
fn start_and_call_are_bounded_and_cancellation_stops_the_process_before_returning() -> Result {
    // Never answers initialize: the start deadline ends it.
    let root = Root::new();
    let mut host = LocalHost::new(&root.0);
    let started = Instant::now();
    let r = host.with_process(
        (5, "local".into()),
        (PYTHON, &args("silent"), &[]),
        None,
        &NOT_CANCELLED,
        Instant::now() + Duration::from_secs(3),
        Some(Duration::from_secs(5)),
        |p| p.list(),
    );
    assert!(matches!(r, Err((PluginCallOutcome::Timeout, _))));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(group_gone(root.starts(5)[0]));
    // Cancellation during the start (initialize never answered) and during a call (a request
    // the live server never answers).
    for mode in ["silent", "ok"] {
        let root = Root::new();
        let mut host = LocalHost::new(&root.0);
        if mode == "ok" {
            let r = run(
                &mut host,
                6,
                mode,
                &call("pid", false, None, &NOT_CANCELLED),
            );
            assert_eq!(r.outcome, PluginCallOutcome::Completed);
        }
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        let starts = root.0.join("6").join("starts");
        // Cancel once the server is surely running (its start is recorded).
        let canceller = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            while !starts.exists() && Instant::now() < until {
                thread::sleep(Duration::from_millis(10));
            }
            thread::sleep(Duration::from_millis(100));
            f.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let r = host.with_process(
            (6, "local".into()),
            (PYTHON, &args(mode), &[]),
            None,
            &flag,
            Instant::now() + Duration::from_secs(5),
            Some(Duration::from_secs(5)),
            |p| {
                let m = json!({"jsonrpc":"2.0","id":99,"method":"never/answered"});
                p.0.request(99, &m, 1024).map(|_| ())
            },
        );
        let _ = canceller.join();
        assert!(
            matches!(r, Err((PluginCallOutcome::Cancelled, _))),
            "{mode}"
        );
        assert!(started.elapsed() < Duration::from_secs(4), "{mode}");
        assert!(
            group_gone(root.starts(6)[0]),
            "{mode}: stopped before return"
        );
        assert_eq!(host.finish(), vec![]);
    }
    Ok(())
}

#[test]
fn a_server_that_never_reads_stdin_cannot_block_a_request_larger_than_the_pipe() -> Result {
    let root = Root::new();
    let mut host = LocalHost::new(&root.0);
    let started = Instant::now();
    let big = "x".repeat(1024 * 1024);
    let r = host.with_process(
        (7, "local".into()),
        (PYTHON, &args("stop-reading"), &[]),
        None,
        &NOT_CANCELLED,
        Instant::now() + Duration::from_secs(3),
        None,
        |p| {
            let m = json!({"jsonrpc":"2.0","id":9,"method":"tools/list","params":{"x":big}});
            p.0.request(9, &m, 64).map(|_| ())
        },
    );
    assert!(matches!(r, Err((PluginCallOutcome::Timeout, _))));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(group_gone(root.starts(7)[0]));
    assert_eq!(host.finish(), vec![]);
    Ok(())
}

#[test]
fn at_most_four_processes_live_and_the_fifth_server_is_unavailable() -> Result {
    let root = Root::new();
    let mut host = LocalHost::new(&root.0);
    for plugin in 10..14 {
        let r = run(
            &mut host,
            plugin,
            "ok",
            &call("pid", false, None, &NOT_CANCELLED),
        );
        assert_eq!(r.outcome, PluginCallOutcome::Completed);
    }
    let r = run(
        &mut host,
        14,
        "ok",
        &call("pid", false, None, &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Unavailable);
    assert!(root.starts(14).is_empty());
    let leaders = (10..14).map(|p| root.starts(p)[0]).collect::<Vec<_>>();
    assert_eq!(host.finish(), vec![]);
    assert!(leaders.into_iter().all(group_gone));
    Ok(())
}

#[test]
fn a_leader_not_reaped_in_time_is_an_unconfirmed_stop_and_still_releases() -> Result {
    let root = Root::new();
    let quick = Bounds {
        term: Duration::ZERO,
        leader: Duration::ZERO,
        group: Duration::ZERO,
    };
    let mut host = LocalHost::with_bounds(&root.0, quick);
    let r = run(
        &mut host,
        8,
        "ignore-term",
        &call("pid", false, None, &NOT_CANCELLED),
    );
    assert_eq!(r.outcome, PluginCallOutcome::Completed);
    let started = Instant::now();
    assert_eq!(host.finish(), vec![(8, "local".to_owned())]);
    assert!(started.elapsed() < Duration::from_secs(1));
    // The background reaper still collects the killed leader.
    let pid = root.starts(8)[0];
    let until = Instant::now() + Duration::from_secs(3);
    while !gone(pid) && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(gone(pid));
    Ok(())
}

#[test]
fn discovery_shows_a_redacted_tail_on_failure_and_is_cancellable() -> Result {
    let root = Root::new();
    let names = vec!["TEST_SECRET".to_owned()];
    let target = Target::Local {
        command: PYTHON.into(),
        args: args("stderr-secret"),
        env_names: names,
    };
    let d = discover(
        &root.0,
        9,
        "local",
        &target,
        Some(r#"{"TEST_SECRET":"hunter2-secret"}"#),
        &NOT_CANCELLED,
    );
    assert_eq!(d.listed, Err(PluginCallOutcome::Unavailable));
    let tail = d.stderr.ok_or("tail")?;
    assert!(tail.contains("token=[redacted] failed"), "{tail}");
    assert!(!tail.contains("hunter2"));
    assert!(d.stop_confirmed);
    // Missing values: nothing starts.
    let d = discover(&root.0, 9, "local", &target, None, &NOT_CANCELLED);
    assert_eq!(d.listed, Err(PluginCallOutcome::NotConfigured));
    assert_eq!(root.starts(9).len(), 1);
    // A silent server: the user's cancel ends it long before the 120 s bound.
    assert_eq!(LOCAL_DISCOVERY, Duration::from_secs(120));
    let flag = Arc::new(AtomicBool::new(false));
    let f = flag.clone();
    let starts = root.0.join("15").join("starts");
    let canceller = thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        while !starts.exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        f.store(true, Ordering::Release);
    });
    let silent = Target::Local {
        command: PYTHON.into(),
        args: args("silent"),
        env_names: vec![],
    };
    let started = Instant::now();
    let d = discover(&root.0, 15, "local", &silent, None, &flag);
    let _ = canceller.join();
    assert_eq!(d.listed, Err(PluginCallOutcome::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(group_gone(root.starts(15)[0]));
    let ok = Target::Local {
        command: PYTHON.into(),
        args: args("ok"),
        env_names: vec![],
    };
    let d = discover(&root.0, 16, "local", &ok, None, &NOT_CANCELLED);
    assert_eq!(d.listed.map(|t| t.len()), Ok(3));
    assert!(d.stderr.is_none() && d.stop_confirmed);
    Ok(())
}

#[test]
fn the_env_map_is_parsed_strictly_after_decoding() {
    let names = ["A".to_owned(), "B".to_owned()];
    assert_eq!(
        parse_env_map(r#"{"B":"2","A":"1"}"#, &names),
        Ok(vec![("B".into(), "2".into()), ("A".into(), "1".into())])
    );
    for (text, outcome) in [
        (r#"{"A":"1","A":"2","B":"3"}"#, PluginCallOutcome::Invalid),
        (r#"{"A":"1","B":"2","C":"3"}"#, PluginCallOutcome::Invalid),
        (r#"{"A":"1","B":"a b"}"#, PluginCallOutcome::Invalid),
        (r#"{"A":"1","B":"a\u0000b"}"#, PluginCallOutcome::Invalid),
        (r#"{"A":"1","B":"é"}"#, PluginCallOutcome::Invalid),
        (r#"{"A":"1","B":""}"#, PluginCallOutcome::Invalid),
        (r#"{"A":"1","B":2}"#, PluginCallOutcome::Invalid),
        (r#"["A","1"]"#, PluginCallOutcome::Invalid),
        ("not json", PluginCallOutcome::Invalid),
        (r#"{"A":"1"}"#, PluginCallOutcome::NotConfigured),
    ] {
        assert_eq!(parse_env_map(text, &names).err(), Some(outcome), "{text}");
    }
    let many = (0..17).map(|i| format!("V{i}")).collect::<Vec<_>>();
    let text = serde_json::to_string(
        &many
            .iter()
            .map(|n| (n.clone(), "x".to_owned()))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap_or_default();
    assert_eq!(
        parse_env_map(&text, &many).err(),
        Some(PluginCallOutcome::Invalid)
    );
}
