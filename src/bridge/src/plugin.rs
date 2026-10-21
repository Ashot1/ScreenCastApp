//! Plugin ABI2 on the memory3 handle (ADR-021, ADR-022). The only bridge state is the table of
//! operation handles that lets native cancel a blocking discovery, like the model operations.
//! Discovery is the only network or process use here; the Store lock is released around it.
use crate::integration::capacity;
use crate::source::{error, store};
use crate::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AmbientPluginFile {
    pub kind: u32,
    pub reserved: u32,
    pub path: AmbientText,
    pub bytes: AmbientText,
}
const _: () = assert!(std::mem::size_of::<AmbientPluginFile>() == 40);
const REVIEW_CAPACITY: usize = 256 * 1024;
const LIST_CAPACITY: usize = 1024 * 1024;
const CALLS_CAPACITY: usize = 768 * 1024;
const DISCOVER_CAPACITY: usize = 512 * 1024;
const INVALID: u32 = 1;
const BUSY: u32 = 4;
const LIMIT: u32 = 6;
const HANDLE: u32 = 7;
const PANIC: u32 = 8;

/// One cancellable blocking operation. The cancel flag is sticky: a cancelled operation cancels
/// every later use, so each discovery takes a fresh one.
struct Operation {
    cancelled: AtomicBool,
    busy: AtomicBool,
}
static OPERATIONS: Mutex<(u64, BTreeMap<u64, Arc<Operation>>)> = Mutex::new((1, BTreeMap::new()));
fn operation(op: u64) -> Result<Arc<Operation>, u32> {
    OPERATIONS
        .lock()
        .map_err(|_| PANIC)?
        .1
        .get(&op)
        .cloned()
        .ok_or(HANDLE)
}
/// Busy from the start of a blocking call until its actual return, processes stopped included.
struct Running(Arc<Operation>);
impl Running {
    /// Marks the operation busy for one blocking call; a second concurrent call gets 4.
    fn acquire(op: u64) -> Result<Self, u32> {
        let o = operation(op)?;
        o.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BUSY)?;
        Ok(Self(o))
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}
/// # Safety
/// out is aligned writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_operation_begin(out: *mut u64) -> u32 {
    boundary(|| {
        output(out).map_err(|_| INVALID)?;
        let mut reg = OPERATIONS.lock().map_err(|_| PANIC)?;
        if reg.1.len() >= 64 {
            return Err(LIMIT);
        }
        let id = reg.0;
        reg.0 = id.checked_add(1).ok_or(LIMIT)?;
        reg.1.insert(
            id,
            Arc::new(Operation {
                cancelled: AtomicBool::new(false),
                busy: AtomicBool::new(false),
            }),
        );
        unsafe { out.write(id) };
        Ok(())
    })
}
/// Requests a stop; the running call returns after its processes are stopped.
#[unsafe(no_mangle)]
pub extern "C" fn ambient_plugin_v2_operation_cancel(op: u64) -> u32 {
    boundary(|| {
        operation(op)?.cancelled.store(true, Ordering::Release);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_plugin_v2_operation_close(op: u64) -> u32 {
    boundary(|| {
        let mut reg = OPERATIONS.lock().map_err(|_| PANIC)?;
        let o = reg.1.get(&op).ok_or(HANDLE)?;
        if o.busy.load(Ordering::Acquire) {
            return Err(BUSY);
        }
        reg.1.remove(&op);
        Ok(())
    })
}

unsafe fn json_output(s: &str, b: *mut u8, cap: u32, used: *mut u32) -> Result<(), u32> {
    if s.len() > cap as usize {
        return Err(LIMIT);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), b, s.len());
        used.write(s.len() as u32);
    }
    Ok(())
}
unsafe fn text<'a>(t: AmbientText, max: usize) -> Result<&'a str, u32> {
    unsafe { input(t, max) }.map_err(|_| INVALID)
}
/// Package files: 1..64 records, each path 1..1024 UTF-8 bytes, bytes ≤256 KiB (`{NULL, 0}` is an
/// empty file), ≤1 MiB in total. Core validates kinds against paths and parses everything.
unsafe fn files<'a>(p: *const AmbientPluginFile, count: u32) -> Result<Vec<PluginFile<'a>>, u32> {
    if count == 0 {
        return Err(INVALID);
    }
    let raw = unsafe { array(p, count, MAX_PLUGIN_FILES as u32) }.map_err(|_| LIMIT)?;
    let mut total = 0usize;
    raw.iter()
        .map(|f| {
            let kind = match (f.kind, f.reserved) {
                (1, 0) => PluginFileKind::Manifest,
                (2, 0) => PluginFileKind::Mcp,
                (3, 0) => PluginFileKind::Skill,
                _ => return Err(INVALID),
            };
            let path = unsafe { text(f.path, 1024) }?;
            let bytes: &[u8] = if f.bytes.bytes.is_null() && f.bytes.length == 0 {
                &[]
            } else {
                if f.bytes.bytes.is_null() {
                    return Err(INVALID);
                }
                if f.bytes.length as usize > MAX_PLUGIN_FILE {
                    return Err(LIMIT);
                }
                unsafe { std::slice::from_raw_parts(f.bytes.bytes, f.bytes.length as usize) }
            };
            total += bytes.len();
            if total > MAX_PLUGIN_FILES_TOTAL {
                return Err(LIMIT);
            }
            Ok(PluginFile { kind, path, bytes })
        })
        .collect()
}

/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_review(
    files: *const AmbientPluginFile,
    count: u32,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, REVIEW_CAPACITY)?;
        let files = unsafe { self::files(files, count) }?;
        let review = review_plugin(&files).map_err(error)?;
        let json = review.json().map_err(error)?;
        unsafe { json_output(&json, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_install(
    h: u64,
    files: *const AmbientPluginFile,
    count: u32,
    hash: AmbientText,
    at_utc: i64,
    plugin: *mut u64,
) -> u32 {
    boundary(|| {
        output(plugin).map_err(|_| INVALID)?;
        let files = unsafe { self::files(files, count) }?;
        let hash = unsafe { text(hash, 64) }?;
        let id = store(h, |s| s.install_plugin(&files, hash, at_utc))?;
        unsafe { plugin.write(id) };
        Ok(())
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_list(
    h: u64,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, LIST_CAPACITY)?;
        let json = store(h, |s| s.plugins_json())?;
        unsafe { json_output(&json, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_calls(
    h: u64,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, CALLS_CAPACITY)?;
        let json = store(h, |s| s.plugin_calls_json())?;
        unsafe { json_output(&json, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities; `key` is `{NULL, 0}` for a
/// server without a secret.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ambient_plugin_v2_discover(
    h: u64,
    op: u64,
    root: AmbientText,
    plugin: u64,
    server: AmbientText,
    generation: u64,
    key: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, DISCOVER_CAPACITY)?;
        let root = unsafe { crate::model::root(root) }?;
        let server = unsafe { text(server, 32) }?;
        let key = if key.bytes.is_null() && key.length == 0 {
            None
        } else {
            let k = unsafe { text(key, 16384) }?;
            if !k.bytes().all(|b| (33..=126).contains(&b)) {
                return Err(INVALID);
            }
            Some(k)
        };
        let running = Running::acquire(op)?;
        // Core picks the target of exactly this generation; providers never read the package.
        let target = store(h, |s| s.plugin_server(plugin, server, generation))?;
        // The Store lock is not held across the bounded network or process operation.
        let found =
            ambient_providers::discover(root, plugin, server, &target, key, &running.0.cancelled);
        drop(running);
        let json = discovery_json(&found, &target).map_err(error)?;
        unsafe { json_output(&json, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_approve(
    h: u64,
    plugin: u64,
    revision: u64,
    server: AmbientText,
    definition: AmbientText,
    hash: AmbientText,
    new_revision: *mut u64,
) -> u32 {
    boundary(|| {
        output(new_revision).map_err(|_| INVALID)?;
        let server = unsafe { text(server, 32) }?;
        let definition = unsafe { text(definition, MAX_TOOL_DEFINITION) }?;
        let hash = unsafe { text(hash, 64) }?;
        let r = store(h, |s| {
            s.approve_plugin_tool(plugin, revision, server, definition, hash)
        })?;
        unsafe { new_revision.write(r) };
        Ok(())
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_unapprove(
    h: u64,
    plugin: u64,
    revision: u64,
    server: AmbientText,
    tool: AmbientText,
    new_revision: *mut u64,
) -> u32 {
    boundary(|| {
        output(new_revision).map_err(|_| INVALID)?;
        let server = unsafe { text(server, 32) }?;
        let tool = unsafe { text(tool, 128) }?;
        let r = store(h, |s| {
            s.unapprove_plugin_tool(plugin, revision, server, tool)
        })?;
        unsafe { new_revision.write(r) };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_plugin_v2_set_enabled(h: u64, plugin: u64, enabled: u32) -> u32 {
    boundary(|| {
        if enabled > 1 {
            return Err(INVALID);
        }
        store(h, |s| s.set_plugin_enabled(plugin, enabled == 1))
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_revoke(
    h: u64,
    plugin: u64,
    revision: u64,
    server: AmbientText,
    new_revision: *mut u64,
) -> u32 {
    boundary(|| {
        output(new_revision).map_err(|_| INVALID)?;
        let server = unsafe { text(server, 32) }?;
        let r = store(h, |s| s.revoke_plugin_credential(plugin, revision, server))?;
        unsafe { new_revision.write(r) };
        Ok(())
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_begin_removal(
    h: u64,
    plugin: u64,
    revision: u64,
    new_revision: *mut u64,
) -> u32 {
    boundary(|| {
        output(new_revision).map_err(|_| INVALID)?;
        let r = store(h, |s| s.begin_plugin_removal(plugin, revision))?;
        unsafe { new_revision.write(r) };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_plugin_v2_finish_removal(h: u64, plugin: u64, revision: u64) -> u32 {
    boundary(|| store(h, |s| s.finish_plugin_removal(plugin, revision)))
}

const BUILTIN_CAPACITY: usize = 256 * 1024;
const STATUS_CAPACITY: usize = 1024;
const INSTALL_CAPACITY: usize = 64 * 1024;
const OAUTH_CAPACITY: usize = 64 * 1024;
unsafe fn managed(id: AmbientText) -> Result<ManagedServerId, u32> {
    ManagedServerId::parse(unsafe { text(id, 64) }?).ok_or(INVALID)
}
unsafe fn json_value(
    v: &impl serde::Serialize,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> Result<(), u32> {
    let json = serde_json::to_string(v).map_err(|_| INVALID)?;
    unsafe { json_output(&json, b, cap, used) }
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_builtin(
    id: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, BUILTIN_CAPACITY)?;
        let id = unsafe { managed(id) }?;
        // The bytes cross as text: the built-in files are UTF-8 JSON, so they stay exact.
        let files = id
            .builtin_package()
            .iter()
            .map(|f| {
                let text = std::str::from_utf8(f.bytes).map_err(|_| INVALID)?;
                Ok(serde_json::json!({"kind": f.kind as u32, "path": f.path, "text": text}))
            })
            .collect::<Result<Vec<_>, u32>>()?;
        unsafe { json_value(&serde_json::json!({ "files": files }), b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_managed_status(
    root: AmbientText,
    id: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, STATUS_CAPACITY)?;
        let root = unsafe { crate::model::root(root) }?;
        let id = unsafe { managed(id) }?;
        let status = ambient_providers::managed_status(root, id);
        unsafe { json_value(&serde_json::json!({ "status": status }), b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_managed_install(
    op: u64,
    root: AmbientText,
    id: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, INSTALL_CAPACITY)?;
        let root = unsafe { crate::model::root(root) }?;
        // An unknown id is refused before anything runs.
        let id = unsafe { managed(id) }?;
        let running = Running::acquire(op)?;
        let r = ambient_providers::managed_install(root, id, &running.0.cancelled);
        drop(running);
        unsafe { json_value(&r, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_oauth_begin(
    id: AmbientText,
    redirect: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, OAUTH_CAPACITY)?;
        let id = unsafe { managed(id) }?;
        let redirect = unsafe { text(redirect, 64) }?;
        let begin = ambient_providers::oauth_begin(id, redirect).ok_or(INVALID)?;
        unsafe { json_value(&begin, b, cap, used) }
    })
}
/// Printable ASCII only: OAuth values never carry spaces or controls.
unsafe fn graphic<'a>(t: AmbientText, max: usize) -> Result<&'a str, u32> {
    let s = unsafe { text(t, max) }?;
    if s.is_empty() || !s.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(INVALID);
    }
    Ok(s)
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ambient_plugin_v2_oauth_exchange(
    id: AmbientText,
    redirect: AmbientText,
    query: AmbientText,
    state: AmbientText,
    verifier: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, OAUTH_CAPACITY)?;
        let id = unsafe { managed(id) }?;
        let redirect = unsafe { text(redirect, 64) }?;
        let query = unsafe { graphic(query, 8192) }?;
        let state = unsafe { graphic(state, 256) }?;
        let verifier = unsafe { graphic(verifier, 256) }?;
        let outcome = ambient_providers::oauth_exchange(id, redirect, query, state, verifier);
        unsafe { json_value(&outcome, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_oauth_refresh(
    id: AmbientText,
    refresh: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, OAUTH_CAPACITY)?;
        let id = unsafe { managed(id) }?;
        let refresh = unsafe { graphic(refresh, 16384) }?;
        let outcome = ambient_providers::oauth_refresh(id, refresh);
        unsafe { json_value(&outcome, b, cap, used) }
    })
}
/// # Safety
/// Caller obeys ambient_plugin.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_plugin_v2_oauth_revoke(
    id: AmbientText,
    refresh: AmbientText,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, STATUS_CAPACITY)?;
        let id = unsafe { managed(id) }?;
        let refresh = unsafe { graphic(refresh, 16384) }?;
        let outcome = ambient_providers::oauth_revoke(id, refresh);
        unsafe { json_value(&outcome, b, cap, used) }
    })
}
