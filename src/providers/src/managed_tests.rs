//! Install and status of the managed server with a test `uv` (ADR-022 «Testing Decisions», 24B):
//! no uv, success with the import check and rename, a refused hash, an outdated folder, cancel.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
static NEXT: AtomicU64 = AtomicU64::new(0);
const GOOGLE: ManagedServerId = ManagedServerId::GoogleWorkspace;

/// A temporary plugins root and a test `uv` that logs its argv and environment, then acts per
/// `mode`: `venv` makes `<venv>/bin/python` (itself a test script that answers the import check),
/// `pip install` succeeds, refuses a hash, hangs or leaves a failing import.
struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new(mode: &str) -> Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "ambient-managed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("root"))?;
        let log = dir.join("log");
        let script = format!(
            r##"#!/usr/bin/python3
import json, os, sys, time
args = sys.argv[1:]
with open({log:?}, "a") as f:
    f.write(json.dumps({{"argv": args, "env": dict(os.environ), "cwd": os.getcwd()}}) + "\n")
os.makedirs(os.environ["UV_CACHE_DIR"], exist_ok=True)
open(os.path.join(os.environ["UV_CACHE_DIR"], "wheel"), "w").close()
if args[0] == "venv":
    bin = os.path.join(args[-1], "bin")
    os.makedirs(bin)
    os.makedirs(os.environ["UV_PYTHON_INSTALL_DIR"], exist_ok=True)
    python = os.path.join(bin, "python")
    with open(python, "w") as p:
        p.write("#!/bin/sh\necho \"$@\" >> {log}.python\nexit {import_exit}\n")
    os.chmod(python, 0o755)
    sys.exit(0)
if {mode:?} == "hash":
    sys.stderr.write("error: Hash mismatch for `fastmcp==4.0.10`\n")
    sys.exit(2)
if {mode:?} == "hang":
    time.sleep(60)
if {mode:?} == "fail":
    sys.stderr.write("error: Failed to fetch: https://pypi.org/simple/fastmcp/\n")
    sys.exit(2)
"##,
            log = log.display().to_string(),
            import_exit = if mode == "import" { 1 } else { 0 },
        );
        let uv = dir.join("uv");
        std::fs::write(&uv, script)?;
        let mut perms = std::fs::metadata(&uv)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&uv, perms)?;
        Ok(Self { dir })
    }
    fn root(&self) -> PathBuf {
        self.dir.join("root")
    }
    fn base(&self) -> PathBuf {
        self.root().join("google-workspace-mcp")
    }
    fn uv(&self) -> Option<PathBuf> {
        Some(self.dir.join("uv"))
    }
    fn calls(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.dir.join("log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
    fn entries(&self) -> Vec<String> {
        let mut names = std::fs::read_dir(self.base())
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
    fn install(&self, flag: &AtomicBool) -> InstallResult {
        install_with(
            &self.root(),
            GOOGLE,
            flag,
            self.uv(),
            Duration::from_secs(20),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn never() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn without_uv_nothing_runs_and_the_status_asks_for_uv() -> Result {
    let f = Fixture::new("ok")?;
    assert_eq!(
        status_with(&f.root(), GOOGLE, false),
        ManagedStatus::UvMissing
    );
    assert_eq!(
        status_with(&f.root(), GOOGLE, true),
        ManagedStatus::NotInstalled
    );
    let r = install_with(&f.root(), GOOGLE, &never(), None, Duration::from_secs(5));
    assert_eq!(r.status, InstallStatus::UvMissing);
    assert!(f.calls().is_empty());
    assert!(!f.base().exists());
    Ok(())
}

#[test]
fn an_install_checks_the_import_renames_the_folder_and_keeps_only_python_and_this_version() -> Result
{
    let f = Fixture::new("ok")?;
    // A crash left a partial folder of an earlier attempt: it goes before anything starts.
    std::fs::create_dir_all(f.base().join("deadbeef.partial/lib"))?;
    let r = f.install(&never());
    assert_eq!(
        r,
        InstallResult {
            status: InstallStatus::Installed,
            stderr_tail: None
        }
    );
    let version = version(GOOGLE);
    assert_eq!(f.entries(), vec![version.clone(), "python".into()]);
    assert_eq!(
        status_with(&f.root(), GOOGLE, false),
        ManagedStatus::Installed
    );
    let calls = f.calls();
    assert_eq!(calls.len(), 2);
    let partial = f.base().join(format!("{version}.partial"));
    assert_eq!(
        calls[0]["argv"],
        serde_json::json!([
            "venv",
            "--python",
            "cpython@3.12.12",
            partial.to_string_lossy()
        ])
    );
    let requirements = partial.join("ambient-requirements.txt");
    assert_eq!(
        calls[1]["argv"],
        serde_json::json!([
            "pip",
            "install",
            "--python",
            partial.join("bin/python").to_string_lossy(),
            "--require-hashes",
            "--only-binary",
            ":all:",
            "--no-deps",
            "--index-url",
            "https://pypi.org/simple",
            "-r",
            requirements.to_string_lossy()
        ])
    );
    for c in &calls {
        let env = c["env"].as_object().ok_or("env")?;
        // Nothing of this process's environment reaches uv: no proxy, config or index
        // variable, no toolchain variable (the system python shim adds its own few).
        assert!(
            env.keys()
                .all(|k| !k.starts_with("CARGO") && !k.starts_with("RUST") && !k.contains("PROXY")),
            "{:?}",
            env.keys()
        );
        assert_ne!(
            env["HOME"],
            std::env::var("HOME").unwrap_or_default().as_str()
        );
        assert!(
            env["PATH"]
                .as_str()
                .is_some_and(|p| p.starts_with("/opt/homebrew/bin:/usr/local/bin:"))
        );
        assert_eq!(env["UV_NO_CONFIG"], "1");
        assert_eq!(env["UV_PYTHON_PREFERENCE"], "only-managed");
        assert_eq!(
            env["UV_PYTHON_INSTALL_DIR"],
            f.base().join("python").to_string_lossy().as_ref()
        );
        assert_eq!(
            env["UV_CACHE_DIR"],
            f.base().join("cache").to_string_lossy().as_ref()
        );
        assert_eq!(
            env["HOME"],
            f.base().join("home").to_string_lossy().as_ref()
        );
    }
    // The renamed folder holds exactly this release's pinned requirements.
    assert_eq!(
        std::fs::read_to_string(f.base().join(&version).join("ambient-requirements.txt"))?,
        crate::managed_recipe::recipe(GOOGLE).requirements
    );
    // The import check ran before the rename, isolated from the working folder.
    assert_eq!(
        std::fs::read_to_string(f.dir.join("log.python"))?,
        "-P -c import main\n"
    );
    Ok(())
}

#[test]
fn a_refused_hash_installs_nothing_and_keeps_the_previous_version() -> Result {
    let f = Fixture::new("hash")?;
    let old = "a".repeat(64);
    std::fs::create_dir_all(f.base().join(&old).join("bin"))?;
    std::fs::write(f.base().join(&old).join("bin/python"), "")?;
    // Another release's folder only: this release needs an update.
    assert_eq!(
        status_with(&f.root(), GOOGLE, true),
        ManagedStatus::Outdated
    );
    let r = f.install(&never());
    assert_eq!(r.status, InstallStatus::HashMismatch);
    assert!(
        r.stderr_tail
            .as_deref()
            .is_some_and(|t| t.contains("Hash mismatch"))
    );
    assert_eq!(f.entries(), vec![old.clone(), "python".into()]);
    assert_eq!(
        status_with(&f.root(), GOOGLE, true),
        ManagedStatus::Outdated
    );
    // Any other refusal is a plain failure; a failing import never gets renamed.
    for mode in ["fail", "import"] {
        let f = Fixture::new(mode)?;
        let r = f.install(&never());
        assert_eq!(r.status, InstallStatus::Failed, "{mode}");
        assert_eq!(f.entries(), vec!["python".to_owned()], "{mode}");
    }
    Ok(())
}

#[test]
fn an_update_replaces_the_previous_version_after_success() -> Result {
    let f = Fixture::new("ok")?;
    let old = "b".repeat(64);
    std::fs::create_dir_all(f.base().join(&old).join("bin"))?;
    assert_eq!(f.install(&never()).status, InstallStatus::Installed);
    assert_eq!(f.entries(), vec![version(GOOGLE), "python".into()]);
    Ok(())
}

#[test]
fn cancel_stops_the_running_step_and_leaves_no_partial_folder() -> Result {
    let f = Fixture::new("hang")?;
    let flag = Arc::new(AtomicBool::new(false));
    let stop = flag.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        stop.store(true, std::sync::atomic::Ordering::Release);
    });
    let started = Instant::now();
    let r = f.install(&flag);
    canceller.join().map_err(|_| "canceller")?;
    assert_eq!(r.status, InstallStatus::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        f.entries().iter().all(|e| e == "python"),
        "{:?}",
        f.entries()
    );
    // A deadline ends a step the same way.
    let f = Fixture::new("hang")?;
    let r = install_with(
        &f.root(),
        GOOGLE,
        &never(),
        f.uv(),
        Duration::from_millis(800),
    );
    assert_eq!(r.status, InstallStatus::Timeout);
    assert!(
        f.entries().iter().all(|e| e == "python"),
        "{:?}",
        f.entries()
    );
    Ok(())
}
