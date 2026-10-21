//! Status and install of a managed server (ADR-022 «Установка»): the state is only the folder
//! layout `<root>/<folder>/{<sha256 of requirements>, python}`; every program runs through
//! `local_host::run_step`. The recipe data is in `managed_recipe.rs`.
use crate::local_host::{Step, run_step};
use crate::managed_recipe::{INSTALL_TIMEOUT, UV, recipe};
use crate::managed_types::{InstallResult, InstallStatus, ManagedStatus};
use ambient_core::ManagedServerId;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const PYTHON: &str = "python";
fn folder(root: &Path, id: ManagedServerId) -> PathBuf {
    root.join(recipe(id).folder)
}
/// The version of this release: the SHA-256 of its pinned requirements.
fn version(id: ManagedServerId) -> String {
    Sha256::digest(recipe(id).requirements.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// The venv the run recipe uses: the folder named by this release's version.
pub(crate) fn venv(root: &Path, id: ManagedServerId) -> PathBuf {
    folder(root, id).join(version(id))
}
fn installed(venv: &Path) -> bool {
    venv.join("bin").join(PYTHON).exists()
}
/// The first of the four fixed places that holds `uv`; Ambient never installs it (D69).
fn uv() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    UV.iter()
        .map(|p| match p.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(p),
        })
        .find(|p| p.is_file())
}
/// A folder of another release: a 64-hex name other than this release's.
fn other_version(name: &OsStr, current: &str) -> bool {
    name.to_str()
        .is_some_and(|n| n != current && n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn managed_status(root: &Path, id: ManagedServerId) -> ManagedStatus {
    status_with(root, id, uv().is_some())
}
fn status_with(root: &Path, id: ManagedServerId, uv: bool) -> ManagedStatus {
    if installed(&venv(root, id)) {
        return ManagedStatus::Installed;
    }
    if !uv {
        return ManagedStatus::UvMissing;
    }
    let current = version(id);
    let outdated = std::fs::read_dir(folder(root, id)).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| other_version(&e.file_name(), &current))
    });
    if outdated {
        ManagedStatus::Outdated
    } else {
        ManagedStatus::NotInstalled
    }
}
/// Blocking, cancellable install with the user's `uv`; at most 10 minutes.
pub fn managed_install(root: &Path, id: ManagedServerId, flag: &AtomicBool) -> InstallResult {
    install_with(root, id, flag, uv(), INSTALL_TIMEOUT)
}
fn result(status: InstallStatus, stderr_tail: Option<String>) -> InstallResult {
    InstallResult {
        status,
        stderr_tail,
    }
}
/// Removes the entries of `folder` that `keep` rejects; a missing folder is already clean.
fn remove_except(folder: &Path, keep: impl Fn(&OsStr) -> bool) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for e in entries.flatten() {
        if keep(&e.file_name()) {
            continue;
        }
        let path = e.path();
        // `remove_dir_all` never follows links; a stray file goes the same way.
        let _ = std::fs::remove_dir_all(&path).or_else(|_| std::fs::remove_file(&path));
    }
}
/// The steps of ADR-022: leftover `.partial` folders go first, then `uv venv` with the pinned
/// managed Python and `uv pip install` with required hashes, binaries only and no dependency
/// resolution, all into `<version>.partial`; `python -P -c "import main"` must pass before the
/// atomic rename. Only after success are other versions removed. A failure leaves nothing new
/// behind; the uv cache never stays.
pub(crate) fn install_with(
    root: &Path,
    id: ManagedServerId,
    flag: &AtomicBool,
    uv: Option<PathBuf>,
    timeout: Duration,
) -> InstallResult {
    let Some(uv) = uv else {
        return result(InstallStatus::UvMissing, None);
    };
    // A request cancelled before it starts runs nothing at all.
    if flag.load(std::sync::atomic::Ordering::Acquire) {
        return result(InstallStatus::Cancelled, None);
    }
    let deadline = Instant::now() + timeout;
    let r = recipe(id);
    let base = folder(root, id);
    let current = version(id);
    let partial = base.join(format!("{current}.partial"));
    let home = base.join("home");
    let transient =
        |n: &OsStr| n == "home" || n == "cache" || n.to_string_lossy().ends_with(".partial");
    remove_except(&base, |n| !transient(n));
    if std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .is_err()
    {
        return result(InstallStatus::Failed, None);
    }
    let path = |p: &Path| p.as_os_str().to_owned();
    // uv reads no configuration and no proxy; Python and its cache live in Ambient's folder.
    let uv_env = vec![
        ("UV_NO_CONFIG".to_owned(), "1".to_owned()),
        ("UV_PYTHON_PREFERENCE".to_owned(), "only-managed".to_owned()),
        (
            "UV_PYTHON_INSTALL_DIR".to_owned(),
            base.join(PYTHON).to_string_lossy().into_owned(),
        ),
        (
            "UV_CACHE_DIR".to_owned(),
            base.join("cache").to_string_lossy().into_owned(),
        ),
    ];
    let requirements = partial.join("ambient-requirements.txt");
    let python = partial.join("bin").join(PYTHON);
    let outcome = (|| {
        let venv_args = [
            OsStr::new("venv"),
            OsStr::new("--python"),
            OsStr::new(r.python),
        ];
        let target = path(&partial);
        let mut args = venv_args.to_vec();
        args.push(&target);
        step(
            run_step(&home, &uv, &args, uv_env.clone(), flag, deadline),
            false,
        )?;
        std::fs::write(&requirements, r.requirements)
            .map_err(|_| result(InstallStatus::Failed, None))?;
        let (python_arg, requirements_arg) = (path(&python), path(&requirements));
        let pip = [
            OsStr::new("pip"),
            OsStr::new("install"),
            OsStr::new("--python"),
            &python_arg,
            OsStr::new("--require-hashes"),
            OsStr::new("--only-binary"),
            OsStr::new(":all:"),
            OsStr::new("--no-deps"),
            OsStr::new("--index-url"),
            OsStr::new(r.index),
            OsStr::new("-r"),
            &requirements_arg,
        ];
        step(
            run_step(&home, &uv, &pip, uv_env.clone(), flag, deadline),
            true,
        )?;
        // Importing `main` runs its top-level code, so it gets the server's stateless setting.
        let check = vec![
            ("WORKSPACE_MCP_STATELESS_MODE".to_owned(), "true".to_owned()),
            ("MCP_ENABLE_OAUTH21".to_owned(), "true".to_owned()),
        ];
        let import = [
            OsStr::new("-P"),
            OsStr::new("-c"),
            OsStr::new("import main"),
        ];
        step(
            run_step(&home, &python, &import, check, flag, deadline),
            false,
        )?;
        let installed_venv = base.join(&current);
        let _ = std::fs::remove_dir_all(&installed_venv);
        std::fs::rename(&partial, &installed_venv).map_err(|_| result(InstallStatus::Failed, None))
    })();
    match outcome {
        Ok(()) => {
            remove_except(&base, |n| n == PYTHON || n == current.as_str());
            result(InstallStatus::Installed, None)
        }
        Err(failed) => {
            remove_except(&base, |n| !transient(n));
            failed
        }
    }
}
/// Maps one step's end to the install answer; uv names a refused hash in its stderr.
fn step((step, tail): (Step, Option<String>), hashes: bool) -> Result<(), InstallResult> {
    let status = match step {
        Step::Exited(true) => return Ok(()),
        Step::Exited(false)
            if hashes
                && tail
                    .as_deref()
                    .is_some_and(|t| t.to_ascii_lowercase().contains("hash mismatch")) =>
        {
            InstallStatus::HashMismatch
        }
        Step::Exited(false) | Step::Unavailable => InstallStatus::Failed,
        Step::Cancelled => InstallStatus::Cancelled,
        Step::Timeout => InstallStatus::Timeout,
    };
    Err(result(status, tail))
}

#[cfg(test)]
#[path = "managed_tests.rs"]
mod tests;
