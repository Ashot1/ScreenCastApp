//! Platforms without the Unix process host: the managed server is not installable until the
//! Windows adapter (ADR-022 «Windows позже»); status and install say so.
use crate::managed_types::{InstallResult, InstallStatus, ManagedStatus};
use ambient_core::ManagedServerId;
use std::{path::Path, sync::atomic::AtomicBool};

pub fn managed_status(_root: &Path, _id: ManagedServerId) -> ManagedStatus {
    ManagedStatus::NotInstalled
}
pub fn managed_install(_root: &Path, _id: ManagedServerId, _flag: &AtomicBool) -> InstallResult {
    InstallResult {
        status: InstallStatus::Failed,
        stderr_tail: None,
    }
}
