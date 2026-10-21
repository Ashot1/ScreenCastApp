//! Platforms without the Unix process-group host. Remote servers work as everywhere; a local
//! server is unavailable until the Windows adapter (Job Objects, ADR-022 «Windows позже»).
use ambient_core::{Discovery, PluginCallOutcome, Target, ToolCallPermit, ToolCallResult};
use std::{path::Path, sync::atomic::AtomicBool};

pub struct LocalHost;
impl LocalHost {
    pub fn new(_root: &Path) -> Self {
        Self
    }
    pub fn call_tool(&mut self, permit: &ToolCallPermit, key: Option<&str>) -> ToolCallResult {
        match &permit.target {
            Target::Remote { url } => crate::mcp::call(url, crate::mcp::McpCall::of(permit, key)),
            Target::Local { .. } | Target::Managed(_) => {
                crate::mcp::result(Err(PluginCallOutcome::Unavailable), None)
            }
        }
    }
    pub fn finish(self) -> Vec<(u64, String)> {
        Vec::new()
    }
}
pub fn discover(
    _root: &Path,
    _plugin: u64,
    _server: &str,
    target: &Target,
    key: Option<&str>,
    flag: &AtomicBool,
) -> Discovery {
    Discovery {
        listed: match target {
            Target::Remote { url } => crate::mcp::list_remote(url, key, flag),
            Target::Local { .. } | Target::Managed(_) => Err(PluginCallOutcome::Unavailable),
        },
        stderr: None,
        stop_confirmed: true,
    }
}
