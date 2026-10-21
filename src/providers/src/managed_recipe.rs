//! Data of the managed server recipe (ADR-022). Process code lives in `local_host.rs`.

pub(crate) struct Recipe {
    /// Folder under the plugins root: `<root>/<folder>/{<sha256 of requirements>, python, ...}`.
    pub folder: &'static str,
    /// Pinned arm64 requirements with hashes.
    pub requirements: &'static str,
    /// uv request of the managed interpreter.
    pub python: &'static str,
    pub index: &'static str,
    /// Arguments after `<venv>/bin/python`.
    pub args: &'static [&'static str],
    /// Fixed environment of the server; the port, the client id and a random signing key are
    /// added at start.
    pub env: &'static [(&'static str, &'static str)],
}

const GOOGLE: Recipe = Recipe {
    folder: "google-workspace-mcp",
    requirements: include_str!("../managed/google-workspace-requirements.txt"),
    python: "cpython@3.12.12",
    index: "https://pypi.org/simple",
    // `-P` keeps the current directory off `sys.path`, so nothing in the working folder can
    // shadow a module. `python -m main` is used instead of the console script because the
    // script's shebang breaks after the folder rename.
    args: &[
        "-P",
        "-m",
        "main",
        "--transport",
        "streamable-http",
        "--read-only",
        "--tools",
        "drive",
        "docs",
    ],
    env: &[
        ("MCP_ENABLE_OAUTH21", "true"),
        ("EXTERNAL_OAUTH21_PROVIDER", "true"),
        ("WORKSPACE_MCP_STATELESS_MODE", "true"),
        // Mandatory: without it OAuth 2.1 mode binds 0.0.0.0.
        ("WORKSPACE_MCP_HOST", "127.0.0.1"),
        ("WORKSPACE_MCP_MAX_FILE_BYTES", "131072"),
        ("WORKSPACE_MCP_DISABLE_LOCAL_FILES", "true"),
        // Defence in depth: no PyPI call (the banner is already off).
        ("FASTMCP_CHECK_FOR_UPDATES", "off"),
    ],
};

pub(crate) fn recipe(id: ambient_core::ManagedServerId) -> &'static Recipe {
    match id {
        ambient_core::ManagedServerId::GoogleWorkspace => &GOOGLE,
    }
}

/// uv in four fixed places (ADR-022); `~/` is relative to the user's real home.
pub(crate) const UV: [&str; 4] = [
    "/opt/homebrew/bin/uv",
    "/usr/local/bin/uv",
    "~/.local/bin/uv",
    "~/.cargo/bin/uv",
];

pub(crate) const INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

#[cfg(test)]
mod tests {
    use super::*;

    fn google() -> &'static Recipe {
        recipe(ambient_core::ManagedServerId::GoogleWorkspace)
    }

    #[test]
    fn args_isolate_path_before_running_main_module() -> Result<(), &'static str> {
        let args = google().args;
        let isolate = args.iter().position(|a| *a == "-P").ok_or("-P")?;
        let module = args.iter().position(|a| *a == "-m").ok_or("-m")?;
        assert!(isolate < module);
        assert_eq!(args[module + 1], "main");
        Ok(())
    }

    #[test]
    fn env_binds_loopback_and_has_no_telemetry() {
        let env = google().env;
        assert!(env.contains(&("WORKSPACE_MCP_HOST", "127.0.0.1")));
        assert!(env.iter().all(|(name, _)| !name.starts_with("OTEL_")));
    }

    #[test]
    fn requirements_end_with_pinned_hashed_server() {
        let lines: Vec<&str> = google().requirements.lines().collect();
        let n = lines.len();
        assert_eq!(lines[n - 2].trim_end(), "workspace-mcp==2.1.1 \\");
        assert_eq!(
            lines[n - 1].trim(),
            "--hash=sha256:31cf317dbe98827505f428d6bba2616695f1bb981ae93ce4951249663a233720"
        );
    }
}
