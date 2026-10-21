//! Local MCP servers (ADR-022): the only place where child processes are spawned, spoken to over
//! stdio and stopped. One `LocalHost` lives for one dispatch or one discovery; nothing about the
//! processes is stored. Unix process groups; Windows (Job Objects) is a later adapter.
use crate::mcp::{MAX_CALL_BODY, McpCall, Protocol, Wire};
use ambient_core::{
    Discovery, LOCAL_START, ManagedServerId, PluginCallOutcome, Target, ToolCallPermit,
    ToolCallResult,
};
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::DirBuilderExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_LIVE: usize = 4;
/// Starting a cold `npx -y` server downloads code, so discovery gets more than a call.
const LOCAL_DISCOVERY: Duration = Duration::from_secs(120);
const SLICE: Duration = Duration::from_millis(25);
const STDERR_TAIL: usize = 8 * 1024;
/// Raw stderr kept beyond the shown tail, so a secret crossing the tail's start is still whole
/// when it is cut out (values are at most 16 KiB).
const STDERR_RAW: usize = STDERR_TAIL + 16 * 1024;
const MAX_ENV: usize = 16;

/// Bounds of the stop sequence; tests shorten the leader wait to reach the unconfirmed branch.
#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    pub term: Duration,
    pub leader: Duration,
    pub group: Duration,
}
pub(crate) const BOUNDS: Bounds = Bounds {
    term: Duration::from_millis(300),
    leader: Duration::from_secs(2),
    group: Duration::from_secs(2),
};

enum Line {
    Text(Vec<u8>),
    /// A line longer than the largest operation limit, cut while reading.
    Overlong,
    /// End of stdout, a read error or a failed write: the stream is gone.
    End,
}
struct Process {
    /// Taken by a background reaper when the leader is not reaped in time.
    child: Option<Child>,
    group: Pid,
    io: Io,
    stderr: Arc<Mutex<Vec<u8>>>,
    /// Closed when the stderr reader reaches end of stream.
    stderr_done: Receiver<()>,
    secrets: Vec<String>,
}
enum Io {
    Pipes(Pipes),
    /// The managed server answers HTTP on this loopback port; its stdin and stdout are null.
    Port(u16),
    /// An install step: only its exit matters.
    None,
}
/// A stdio server's writer queue, its read lines and the next request id.
struct Pipes {
    writer: Option<Sender<Vec<u8>>>,
    lines: Receiver<Line>,
    next: u64,
}
/// The stdio wire of one process for one operation: writes and reads go through their own
/// threads, so a server that never reads stdin cannot block this thread past the deadline.
pub(crate) struct StdioWire<'a> {
    process: &'a mut Pipes,
    flag: &'a AtomicBool,
    deadline: Instant,
}
impl Wire for StdioWire<'_> {
    fn next_id(&mut self) -> u64 {
        self.process.next += 1;
        self.process.next
    }
    fn request(
        &mut self,
        id: u64,
        message: &Value,
        limit: usize,
    ) -> Result<(Value, usize), PluginCallOutcome> {
        self.notify(message)?;
        let mut total = 0usize;
        loop {
            if self.flag.load(Ordering::Acquire) {
                return Err(PluginCallOutcome::Cancelled);
            }
            if Instant::now() >= self.deadline {
                return Err(PluginCallOutcome::Timeout);
            }
            let line = match self.process.lines.recv_timeout(SLICE) {
                Ok(Line::Text(line)) => line,
                Ok(Line::Overlong) => return Err(PluginCallOutcome::Invalid),
                Ok(Line::End) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(PluginCallOutcome::Unavailable);
                }
                Err(RecvTimeoutError::Timeout) => continue,
            };
            // Notifications count toward the operation's limit too.
            total += line.len() + 1;
            if total > limit {
                return Err(PluginCallOutcome::Invalid);
            }
            let v: Value = serde_json::from_slice(&line).map_err(|_| PluginCallOutcome::Invalid)?;
            if !v.is_object() || v.get("jsonrpc") != Some(&Value::from("2.0")) {
                return Err(PluginCallOutcome::Invalid);
            }
            if crate::mcp::answer(&v, id)? {
                return Ok((v, total));
            }
        }
    }
    fn notify(&mut self, message: &Value) -> Result<(), PluginCallOutcome> {
        let mut bytes = message.to_string().into_bytes();
        bytes.push(b'\n');
        self.process
            .writer
            .as_ref()
            .ok_or(PluginCallOutcome::Unavailable)?
            .send(bytes)
            .map_err(|_| PluginCallOutcome::Unavailable)
    }
}

/// The fixed PATH (ADR-022): no shell profile, `~` is the user's real home folder.
fn path(home: &Path) -> String {
    let h = home.display();
    format!(
        "/opt/homebrew/bin:/usr/local/bin:{h}/.local/bin:{h}/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    )
}
/// The empty private folder of a plugin's server: its working folder, HOME and TMPDIR (0700).
fn private(root: &Path, plugin: u64) -> Result<PathBuf, PluginCallOutcome> {
    let folder = root.join(plugin.to_string());
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)
        .map_err(|_| PluginCallOutcome::Unavailable)?;
    Ok(folder)
}
/// Starts a program in its own process group with a cleared environment and the fixed PATH;
/// `folder` is its working folder, HOME and TMPDIR. Only a stdio server gets stdin and stdout
/// pipes: any other program's stdout goes to /dev/null, so a full pipe never stops it. stderr
/// always feeds the bounded tail; `secrets` are cut out of it before it is shown.
fn launch(
    folder: &Path,
    command: &(impl AsRef<std::ffi::OsStr> + ?Sized),
    args: &[impl AsRef<std::ffi::OsStr>],
    env: Vec<(String, String)>,
    secrets: Vec<String>,
    stdio: bool,
) -> Result<Process, PluginCallOutcome> {
    let home = std::env::home_dir().ok_or(PluginCallOutcome::Unavailable)?;
    let (input, output) = if stdio {
        (Stdio::piped(), Stdio::piped())
    } else {
        (Stdio::null(), Stdio::null())
    };
    let mut child = Command::new(command)
        .args(args)
        .env_clear()
        .env("PATH", path(&home))
        .env("HOME", folder)
        .env("TMPDIR", folder)
        .envs(env)
        .current_dir(folder)
        .process_group(0)
        .stdin(input)
        .stdout(output)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| PluginCallOutcome::Unavailable)?;
    let group = Pid::from_child(&child);
    let Some(mut stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(PluginCallOutcome::Unavailable);
    };
    let tail = Arc::new(Mutex::new(Vec::new()));
    let keep = tail.clone();
    let (done, stderr_done) = mpsc::channel::<()>();
    thread::spawn(move || {
        let _done = done;
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Ok(mut t) = keep.lock() {
                t.extend_from_slice(&chunk[..n]);
                let excess = t.len().saturating_sub(STDERR_RAW);
                t.drain(..excess);
            }
        }
    });
    Ok(Process {
        child: Some(child),
        group,
        io: Io::None,
        stderr: tail,
        stderr_done,
        secrets,
    })
}
/// A stdio server: the launched process plus its writer and line-reader threads.
fn spawn(
    root: &Path,
    plugin: u64,
    command: &str,
    args: &[String],
    env: Vec<(String, String)>,
) -> Result<Process, PluginCallOutcome> {
    let folder = private(root, plugin)?;
    let secrets = env.iter().map(|(_, v)| v.clone()).collect();
    let mut p = launch(&folder, command, args, env, secrets, true)?;
    let child = p.child.as_mut().ok_or(PluginCallOutcome::Unavailable)?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        stop(&mut p, BOUNDS);
        return Err(PluginCallOutcome::Unavailable);
    };
    let (line_tx, lines) = mpsc::channel();
    let (writer, queue) = mpsc::channel::<Vec<u8>>();
    let failed = line_tx.clone();
    // Dropping the sender ends this thread, which closes stdin: the MCP shutdown request. A write
    // to an exited process fails with EPIPE (the app ignores SIGPIPE) and ends the stream.
    thread::spawn(move || {
        for bytes in queue {
            if stdin
                .write_all(&bytes)
                .and_then(|()| stdin.flush())
                .is_err()
            {
                let _ = failed.send(Line::End);
                break;
            }
        }
    });
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            // Bounded while reading: memory never exceeds the largest operation limit.
            match (&mut reader)
                .take(MAX_CALL_BODY as u64 + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(_) if line.last() == Some(&b'\n') => {
                    line.pop();
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    if line_tx.send(Line::Text(line)).is_err() {
                        break;
                    }
                }
                Ok(_) if line.len() > MAX_CALL_BODY => {
                    let _ = line_tx.send(Line::Overlong);
                    break;
                }
                _ => {
                    let _ = line_tx.send(Line::End);
                    break;
                }
            }
        }
    });
    p.io = Io::Pipes(Pipes {
        writer: Some(writer),
        lines,
        next: 0,
    });
    Ok(p)
}
/// Polls `done` every 5 ms while `bound` has not passed; a zero bound never polls.
fn within(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < bound {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    false
}
/// The stop sequence (ADR-022): close stdin, TERM the group, wait 300 ms, KILL the group, wait
/// for the leader, then probe the group until it is empty. A leader not reaped in time is left
/// to a background reaper and the stop is unconfirmed, so busy never waits forever on a process
/// Ambient can no longer end. After the leader is reaped the group is signalled only while the
/// probe shows members, so a recycled group id is never hit.
fn stop(p: &mut Process, bounds: Bounds) -> bool {
    if let Io::Pipes(pipes) = &mut p.io {
        pipes.writer = None;
    }
    let Some(mut child) = p.child.take() else {
        return true;
    };
    let group = p.group;
    // `try_wait` reaps the leader; an error (already reaped) counts as gone.
    let mut reaped = |bound| within(bound, || !matches!(child.try_wait(), Ok(None)));
    let _ = kill_process_group(group, Signal::TERM);
    if !reaped(bounds.term) {
        let _ = kill_process_group(group, Signal::KILL);
        if !reaped(bounds.leader) {
            thread::spawn(move || {
                let _ = child.wait();
            });
            return false;
        }
    }
    within(bounds.group, || match test_kill_process_group(group) {
        Err(e) => e == rustix::io::Errno::SRCH,
        Ok(()) => {
            let _ = kill_process_group(group, Signal::KILL);
            false
        }
    })
}
/// The stderr tail shown to the user: exact secret values of this server cut out, at most 8 KiB.
fn tail(raw: &Mutex<Vec<u8>>, secrets: &[String]) -> Option<String> {
    let raw = raw.lock().ok()?.clone();
    let mut text = String::from_utf8_lossy(&raw).into_owned();
    for s in secrets {
        text = text.replace(s.as_str(), "[redacted]");
    }
    let start = text.ceil_char_boundary(text.len().saturating_sub(STDERR_TAIL));
    let tail = text[start..].trim().to_owned();
    (!tail.is_empty()).then_some(tail)
}

/// The `env(map)` secret: compact JSON of name → value. Duplicates, names outside the permit
/// and more than 16 names are invalid; values are re-checked after decoding because escapes
/// could have produced spaces or NUL. A declared name without a value is «not configured».
pub(crate) fn parse_env_map(
    text: &str,
    names: &[String],
) -> Result<Vec<(String, String)>, PluginCallOutcome> {
    struct Pairs(Vec<(String, String)>);
    impl<'de> serde::Deserialize<'de> for Pairs {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Pairs;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("an object of strings")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut m: A,
                ) -> Result<Pairs, A::Error> {
                    // A map keeps every pair, so a repeated name is seen instead of overwritten.
                    let mut out = Vec::new();
                    while let Some(pair) = m.next_entry::<String, String>()? {
                        if out.len() == MAX_ENV {
                            return Err(serde::de::Error::custom("too many names"));
                        }
                        out.push(pair);
                    }
                    Ok(Pairs(out))
                }
            }
            d.deserialize_map(V)
        }
    }
    let Pairs(pairs) = serde_json::from_str(text).map_err(|_| PluginCallOutcome::Invalid)?;
    for (i, (name, value)) in pairs.iter().enumerate() {
        if !names.contains(name)
            || pairs[..i].iter().any(|(n, _)| n == name)
            || value.is_empty()
            || !value.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(PluginCallOutcome::Invalid);
        }
    }
    if names.iter().any(|n| !pairs.iter().any(|(p, _)| p == n)) {
        return Err(PluginCallOutcome::NotConfigured);
    }
    Ok(pairs)
}
fn secrets(
    names: &[String],
    key: Option<&str>,
) -> Result<Vec<(String, String)>, PluginCallOutcome> {
    match key {
        Some(text) => parse_env_map(text, names),
        None if names.is_empty() => Ok(Vec::new()),
        None => Err(PluginCallOutcome::NotConfigured),
    }
}

enum Server {
    Live(Box<Process>),
    /// A wire failure ended the process; later calls in this request get «unavailable».
    Failed,
}
/// The processes of one dispatch, by (plugin, server); at most four live at once.
pub struct LocalHost {
    root: PathBuf,
    servers: BTreeMap<(u64, String), Server>,
    unconfirmed: Vec<(u64, String)>,
    bounds: Bounds,
    /// Test-only: the managed server's port instead of a probed free one (an occupied port).
    #[cfg(test)]
    port: Option<u16>,
}
impl LocalHost {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            servers: BTreeMap::new(),
            unconfirmed: Vec::new(),
            bounds: BOUNDS,
            #[cfg(test)]
            port: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_bounds(root: &Path, bounds: Bounds) -> Self {
        let mut host = Self::new(root);
        host.bounds = bounds;
        host
    }
    #[cfg(test)]
    pub(crate) fn with_port(root: &Path, port: u16) -> Self {
        let mut host = Self::new(root);
        host.port = Some(port);
        host
    }
    /// One admitted call: remote servers over HTTP, local and managed ones over this host's
    /// processes.
    pub fn call_tool(&mut self, permit: &ToolCallPermit, key: Option<&str>) -> ToolCallResult {
        let c = McpCall::of(permit, key);
        let id = (permit.plugin, permit.server.clone());
        match &permit.target {
            Target::Remote { url } => crate::mcp::call(url, c),
            Target::Local {
                command,
                args,
                env_names,
            } => self.local_call(id, (command, args, env_names), &c),
            Target::Managed(managed) => self.managed_call(id, *managed, &c),
        }
    }
    pub(crate) fn local_call(
        &mut self,
        id: (u64, String),
        command: (&str, &[String], &[String]),
        c: &McpCall<'_>,
    ) -> ToolCallResult {
        let mut listed = None;
        let r = self.with_process(
            id,
            command,
            c.key,
            c.cancellation,
            Instant::now() + LOCAL_START,
            Some(c.timeout),
            |p| p.admitted_call(c, &mut listed),
        );
        crate::mcp::result(r.map_err(|(o, _)| o), listed)
    }
    pub(crate) fn managed_call(
        &mut self,
        id: (u64, String),
        managed: ManagedServerId,
        c: &McpCall<'_>,
    ) -> ToolCallResult {
        let mut listed = None;
        let r = self.with_managed(
            id,
            managed,
            c.key,
            c.cancellation,
            Instant::now() + LOCAL_START,
            |port| crate::mcp::loopback_call(port, c, &mut listed),
        );
        crate::mcp::result(r.map_err(|(o, _)| o), listed)
    }
    /// The live process of `id`, or why there is none: Failed earlier in this request, or the
    /// limit of live processes reached.
    fn live(&mut self, id: &(u64, String)) -> Result<Option<Box<Process>>, PluginCallOutcome> {
        match self.servers.remove(id) {
            Some(Server::Failed) => {
                self.servers.insert(id.clone(), Server::Failed);
                Err(PluginCallOutcome::Unavailable)
            }
            Some(Server::Live(p)) => Ok(Some(p)),
            None => {
                let live = self
                    .servers
                    .values()
                    .filter(|s| matches!(s, Server::Live(_)))
                    .count();
                if live >= MAX_LIVE {
                    return Err(PluginCallOutcome::Unavailable);
                }
                Ok(None)
            }
        }
    }
    /// Starts the server lazily (spawn and one `initialize` within LOCAL_START), then runs
    /// `op` within `timeout`. Any wire failure stops the process and marks the server Failed.
    /// Discovery passes no `timeout`: start and listing share the start deadline.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_process<T>(
        &mut self,
        id: (u64, String),
        (command, args, names): (&str, &[String], &[String]),
        key: Option<&str>,
        flag: &AtomicBool,
        start: Instant,
        timeout: Option<Duration>,
        op: impl FnOnce(&mut Protocol<StdioWire<'_>>) -> Result<T, PluginCallOutcome>,
    ) -> Result<T, (PluginCallOutcome, Option<String>)> {
        let mut process = match self.live(&id).map_err(|o| (o, None))? {
            Some(p) => p,
            None => {
                let env = secrets(names, key).map_err(|o| (o, None))?;
                let mut p = match spawn(&self.root, id.0, command, args, env) {
                    Ok(p) => Box::new(p),
                    Err(o) => {
                        self.servers.insert(id, Server::Failed);
                        return Err((o, None));
                    }
                };
                let Io::Pipes(pipes) = &mut p.io else {
                    return Err((PluginCallOutcome::Unavailable, self.fail(id, *p)));
                };
                let started = Protocol(StdioWire {
                    process: pipes,
                    flag,
                    deadline: start,
                })
                .initialize();
                if let Err(o) = started {
                    return Err((o, self.fail(id, *p)));
                }
                p
            }
        };
        let Io::Pipes(pipes) = &mut process.io else {
            return Err((PluginCallOutcome::Unavailable, self.fail(id, *process)));
        };
        let r = op(&mut Protocol(StdioWire {
            process: pipes,
            flag,
            deadline: timeout.map_or(start, |t| Instant::now() + t),
        }));
        match r {
            Err(
                o @ (PluginCallOutcome::Timeout
                | PluginCallOutcome::Cancelled
                | PluginCallOutcome::Invalid
                | PluginCallOutcome::Unavailable),
            ) => Err((o, self.fail(id, *process))),
            r => {
                self.servers.insert(id, Server::Live(process));
                r.map_err(|o| (o, None))
            }
        }
    }
    /// The managed server (ADR-022): `<venv>/bin/python -P -m main …` on a probed free loopback
    /// port, ready once it accepts connections; every operation is its own stateless HTTP
    /// session with the access token as Bearer. An oversized or malformed answer fails only that
    /// call (the stream is not desynchronised); a timeout, cancellation, refused connection or
    /// exit ends the process and marks the server Failed.
    pub(crate) fn with_managed<T>(
        &mut self,
        id: (u64, String),
        managed: ManagedServerId,
        key: Option<&str>,
        flag: &AtomicBool,
        start: Instant,
        op: impl FnOnce(u16) -> Result<T, PluginCallOutcome>,
    ) -> Result<T, (PluginCallOutcome, Option<String>)> {
        let process = match self.live(&id).map_err(|o| (o, None))? {
            Some(p) => p,
            None => {
                // The token goes to the server only in request headers, never into its env.
                let token = key.ok_or((PluginCallOutcome::LoginRequired, None))?;
                let mut p = match self.start_managed(id.0, managed, token) {
                    Ok(p) => Box::new(p),
                    Err(o) => {
                        self.servers.insert(id, Server::Failed);
                        return Err((o, None));
                    }
                };
                if let Err(o) = ready(&mut p, flag, start) {
                    return Err((o, self.fail(id, *p)));
                }
                p
            }
        };
        let Io::Port(port) = process.io else {
            return Err((PluginCallOutcome::Unavailable, self.fail(id, *process)));
        };
        match op(port) {
            Err(
                o @ (PluginCallOutcome::Timeout
                | PluginCallOutcome::Cancelled
                | PluginCallOutcome::Unavailable),
            ) => Err((o, self.fail(id, *process))),
            r => {
                self.servers.insert(id, Server::Live(process));
                r.map_err(|o| (o, None))
            }
        }
    }
    fn start_managed(
        &self,
        plugin: u64,
        managed: ManagedServerId,
        token: &str,
    ) -> Result<Process, PluginCallOutcome> {
        let python = crate::managed::venv(&self.root, managed)
            .join("bin")
            .join("python");
        let client = crate::oauth::client(managed).ok_or(PluginCallOutcome::Unavailable)?;
        if !python.exists() {
            return Err(PluginCallOutcome::Unavailable);
        }
        #[cfg(test)]
        let port = match self.port {
            Some(p) => p,
            None => free_port()?,
        };
        #[cfg(not(test))]
        let port = free_port()?;
        // Without a client secret the server signs its own session tokens with this key; it is
        // new for every start and lives only in the process environment.
        let signing = oauth2::CsrfToken::new_random_len(32).secret().clone();
        let recipe = crate::managed_recipe::recipe(managed);
        let mut env = recipe
            .env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<Vec<_>>();
        env.push(("WORKSPACE_MCP_PORT".into(), port.to_string()));
        env.push(("GOOGLE_OAUTH_CLIENT_ID".into(), client.0.to_owned()));
        env.push((
            "FASTMCP_SERVER_AUTH_GOOGLE_JWT_SIGNING_KEY".into(),
            signing.clone(),
        ));
        let folder = private(&self.root, plugin)?;
        let mut p = launch(
            &folder,
            &python,
            recipe.args,
            env,
            vec![signing, token.to_owned()],
            false,
        )?;
        p.io = Io::Port(port);
        Ok(p)
    }
    /// Ends a failed process now and returns its redacted stderr tail.
    fn fail(&mut self, id: (u64, String), mut p: Process) -> Option<String> {
        if !stop(&mut p, self.bounds) {
            self.unconfirmed.push(id.clone());
        }
        // The stream ends once the group is gone; a bounded wait collects its last lines.
        let _ = p.stderr_done.recv_timeout(Duration::from_millis(200));
        self.servers.insert(id, Server::Failed);
        tail(&p.stderr, &p.secrets)
    }
    /// Stops every live process and returns the servers whose stop was not confirmed.
    pub fn finish(mut self) -> Vec<(u64, String)> {
        self.stop_all();
        std::mem::take(&mut self.unconfirmed)
    }
    fn stop_all(&mut self) {
        for (id, s) in std::mem::take(&mut self.servers) {
            if let Server::Live(mut p) = s
                && !stop(&mut p, self.bounds)
            {
                self.unconfirmed.push(id);
            }
        }
    }
}
impl Drop for LocalHost {
    /// Every exit path of a dispatch ends its processes before busy is released.
    fn drop(&mut self) {
        self.stop_all();
    }
}
/// Managed-server ports: high, but below the ephemeral ranges of macOS and Windows (49152…) and
/// Linux (32768…). An outgoing connection, the readiness probe's own included, takes its local
/// port from the ephemeral range, so a server port there could be taken by a probe connected to
/// itself just before the server binds it.
const SERVER_PORTS: std::ops::RangeInclusive<u16> = 20000..=32767;
/// A random free port of `SERVER_PORTS`, checked by a probe bind (ADR-022). A same-user process
/// could take it before the server binds; that residual risk is documented.
fn free_port() -> Result<u16, PluginCallOutcome> {
    use std::hash::{BuildHasher, RandomState};
    let span = u64::from(SERVER_PORTS.end() - SERVER_PORTS.start()) + 1;
    let random = RandomState::new();
    for attempt in 0u32..64 {
        let offset = random.hash_one(attempt) % span;
        let port = SERVER_PORTS.start() + offset as u16;
        if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
            return Ok(port);
        }
    }
    Err(PluginCallOutcome::Unavailable)
}
/// Readiness of the managed server: its port accepts a connection before the start deadline.
/// An exit before that (for example a port taken meanwhile) fails at once.
fn ready(p: &mut Process, flag: &AtomicBool, start: Instant) -> Result<(), PluginCallOutcome> {
    let Io::Port(port) = p.io else {
        return Err(PluginCallOutcome::Unavailable);
    };
    let address = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
    loop {
        if flag.load(Ordering::Acquire) {
            return Err(PluginCallOutcome::Cancelled);
        }
        if Instant::now() >= start {
            return Err(PluginCallOutcome::Timeout);
        }
        if p.child
            .as_mut()
            .is_none_or(|c| !matches!(c.try_wait(), Ok(None)))
        {
            return Err(PluginCallOutcome::Unavailable);
        }
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return Ok(());
        }
        thread::sleep(SLICE);
    }
}

/// How one install step ended.
pub(crate) enum Step {
    Exited(bool),
    Cancelled,
    Timeout,
    Unavailable,
}
/// One install step (`uv venv`, `uv pip install`, the import check) run like a server: own
/// process group, cleared environment, `folder` as HOME, TMPDIR and working folder, stdout to
/// /dev/null. Cancellation and the deadline stop its group. Returns the redacted stderr tail.
pub(crate) fn run_step(
    folder: &Path,
    command: &Path,
    args: &[&std::ffi::OsStr],
    env: Vec<(String, String)>,
    flag: &AtomicBool,
    deadline: Instant,
) -> (Step, Option<String>) {
    let mut p = match launch(folder, command, args, env, Vec::new(), false) {
        Ok(p) => p,
        Err(_) => return (Step::Unavailable, None),
    };
    let step = loop {
        let status = match p.child.as_mut().map(Child::try_wait) {
            Some(Ok(Some(status))) => status,
            Some(Ok(None)) => {
                if flag.load(Ordering::Acquire) {
                    break Step::Cancelled;
                }
                if Instant::now() >= deadline {
                    break Step::Timeout;
                }
                thread::sleep(SLICE);
                continue;
            }
            _ => break Step::Unavailable,
        };
        break Step::Exited(status.success());
    };
    // The leader may have exited while children still run; the group is ended either way, and
    // a group not confirmed gone fails the step, so nothing is renamed under a live writer.
    let step = if stop(&mut p, BOUNDS) {
        step
    } else {
        Step::Unavailable
    };
    let _ = p.stderr_done.recv_timeout(Duration::from_millis(200));
    (step, tail(&p.stderr, &p.secrets))
}

/// Stateless blocking discovery of one server. A local or managed server is started for it,
/// listed and stopped; the operation's flag cancels it at any point.
pub fn discover(
    root: &Path,
    plugin: u64,
    server: &str,
    target: &Target,
    key: Option<&str>,
    flag: &AtomicBool,
) -> Discovery {
    let mut host = LocalHost::new(root);
    let id = (plugin, server.to_owned());
    let r = match target {
        Target::Remote { url } => {
            return Discovery {
                listed: crate::mcp::list_remote(url, key, flag),
                stderr: None,
                stop_confirmed: true,
            };
        }
        Target::Local {
            command,
            args,
            env_names,
        } => host.with_process(
            id,
            (command, args, env_names),
            key,
            flag,
            Instant::now() + LOCAL_DISCOVERY,
            None,
            |p| p.list(),
        ),
        Target::Managed(managed) => host.with_managed(
            id,
            *managed,
            key,
            flag,
            Instant::now() + LOCAL_START,
            |port| crate::mcp::loopback_list(port, key, flag, crate::mcp::DISCOVERY_TIMEOUT),
        ),
    };
    let stop_confirmed = host.finish().is_empty();
    let (listed, stderr) = match r {
        Ok(tools) => (Ok(tools), None),
        Err((o, tail)) => (Err(o), tail),
    };
    Discovery {
        listed,
        stderr,
        stop_confirmed,
    }
}

#[cfg(test)]
#[path = "local_host_managed_tests.rs"]
mod managed_tests;
#[cfg(test)]
#[path = "local_host_tests.rs"]
mod tests;
