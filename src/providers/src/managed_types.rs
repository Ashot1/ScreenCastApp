//! The answers of the managed server's status and install, shared by every platform's module.
use serde::Serialize;

/// What the install button shows. «Installing» is UI state, not a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedStatus {
    NotInstalled,
    Installed,
    /// Another release's version is on disk; this release's is not.
    Outdated,
    /// Not installed (or outdated) and `uv` is in none of its fixed places.
    UvMissing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Installed,
    HashMismatch,
    UvMissing,
    Failed,
    Cancelled,
    Timeout,
}
/// The answer of one install; `stderr_tail` is the failed step's last ≤8 KiB, shown only there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstallResult {
    pub status: InstallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
}
