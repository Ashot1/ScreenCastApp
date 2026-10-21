//! Integration ABI1 on the same Store. Store locks never span network or runtime cleanup.
use crate::source::{error, store};
use crate::*;
use ambient_providers::{IntegrationTransport, IntegrationTransportError};
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AmbientGitHubRepository {
    pub owner: AmbientText,
    pub repository: AmbientText,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientIntegrationProfileRef {
    pub id: u64,
    pub settings_revision: u64,
    pub credential_generation: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AmbientIntegrationRead {
    pub task_id: u64,
    pub task_revision: u64,
    pub profile: AmbientIntegrationProfileRef,
    pub target: AmbientText,
    pub session_expires_at: i64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientIntegrationBatch {
    pub task_id: u64,
    pub task_revision: u64,
    pub count: u32,
    pub reserved: u32,
    pub evidence_ids: [u64; 32],
}
pub const AMBIENT_INTEGRATION_LOCAL_PROJECTS: u32 = 1;
pub const AMBIENT_INTEGRATION_CODEX: u32 = 2;
pub const AMBIENT_INTEGRATION_GITHUB: u32 = 3;
pub const AMBIENT_INTEGRATION_JIRA: u32 = 4;
/// Kind values are fixed in the header; only implemented kinds are accepted.
fn kind(raw: u32) -> Result<IntegrationKind, u32> {
    match raw {
        AMBIENT_INTEGRATION_GITHUB => Ok(IntegrationKind::GitHub),
        AMBIENT_INTEGRATION_JIRA => Ok(IntegrationKind::Jira),
        _ => Err(INVALID_ARGUMENT),
    }
}
/// Switches cover every built-in kind, including the file-reading local projects and Codex.
fn switch_kind(raw: u32) -> Result<BuiltInIntegration, u32> {
    match raw {
        AMBIENT_INTEGRATION_LOCAL_PROJECTS => Ok(BuiltInIntegration::LocalProjects),
        AMBIENT_INTEGRATION_CODEX => Ok(BuiltInIntegration::Codex),
        _ => kind(raw).map(BuiltInIntegration::from),
    }
}
fn reference(p: IntegrationProfile) -> AmbientIntegrationProfileRef {
    AmbientIntegrationProfileRef {
        id: p.id,
        settings_revision: p.settings_revision,
        credential_generation: p.credential_generation,
    }
}
fn now() -> Result<i64, u32> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .ok_or(INVALID_ARGUMENT)
}
pub(crate) fn capacity(b: *mut u8, cap: u32, used: *mut u32, min: usize) -> Result<(), u32> {
    output(b)?;
    output(used)?;
    if (cap as usize) < min {
        return Err(error(StoreError::ResourceLimit));
    }
    Ok(())
}
unsafe fn json_output(s: &str, b: *mut u8, used: *mut u32) {
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), b, s.len());
        used.write(s.len() as u32);
    }
}
unsafe fn indices(p: *const u32, count: u32) -> Result<Vec<usize>, u32> {
    if count == 0 || count > 32 || p.is_null() || !p.is_aligned() {
        return Err(INVALID_ARGUMENT);
    }
    Ok(unsafe { std::slice::from_raw_parts(p, count as usize) }
        .iter()
        .map(|v| *v as usize)
        .collect())
}
/// `{NULL, 0}` is an explicitly absent secret part; `input` rejects empty text.
unsafe fn secret_part<'a>(v: AmbientText) -> Result<Option<&'a str>, u32> {
    if v.bytes.is_null() && v.length == 0 {
        return Ok(None);
    }
    unsafe { input(v, 16384) }.map(Some)
}
/// # Safety
/// Caller obeys ambient_integration.h buffer ownership and capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_profiles(
    h: u64,
    raw: u32,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, 1024 * 1024)?;
        let kind = kind(raw)?;
        let s = store(h, |s| s.integration_settings_json(kind))?;
        unsafe {
            json_output(&s, b, used);
        }
        Ok(())
    })
}
/// # Safety
/// Output points to aligned writable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_enabled(h: u64, raw: u32, out: *mut u32) -> u32 {
    boundary(|| {
        output(out)?;
        let kind = switch_kind(raw)?;
        let enabled = store(h, |s| s.integration_enabled(kind))?;
        unsafe { out.write(u32::from(enabled)) };
        Ok(())
    })
}
/// Enabled is the closed boolean tag 0 or 1; the same value changes nothing.
#[unsafe(no_mangle)]
pub extern "C" fn ambient_integration_v1_set_enabled(h: u64, raw: u32, enabled: u32) -> u32 {
    boundary(|| {
        let kind = switch_kind(raw)?;
        if enabled > 1 {
            return Err(INVALID_ARGUMENT);
        }
        store(h, |s| s.set_integration_enabled(kind, enabled == 1))
    })
}
/// # Safety
/// Scope is caller-owned UTF8; excluded is the closed boolean tag 0 or 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_scope_excluded(
    h: u64,
    raw: u32,
    scope: AmbientText,
    excluded: u32,
) -> u32 {
    boundary(|| {
        let kind = kind(raw)?;
        let scope = unsafe { input(scope, 128) }?;
        if excluded > 1 {
            return Err(INVALID_ARGUMENT);
        }
        store(h, |s| {
            s.set_integration_scope_excluded(kind, scope, excluded == 1)
        })
    })
}
/// # Safety
/// Inputs and output obey ambient_integration.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_jira_profile_save(
    h: u64,
    id: u64,
    revision: u64,
    name: AmbientText,
    cloud_id: AmbientText,
    out: *mut AmbientIntegrationProfileRef,
) -> u32 {
    boundary(|| {
        output(out)?;
        let name = unsafe { input(name, 256) }?;
        let cloud_id = unsafe { input(cloud_id, 128) }?;
        let p = store(h, |s| {
            s.save_integration_profile(
                id,
                revision,
                name,
                Some(cloud_id),
                &IntegrationConfig::Jira(JiraConfig::default()),
            )
        })?;
        unsafe {
            out.write(reference(p));
        }
        Ok(())
    })
}
/// # Safety
/// Name and 0..32 repositories are caller-owned UTF8; output obeys ambient_integration.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_github_profile_save(
    h: u64,
    id: u64,
    revision: u64,
    name: AmbientText,
    repositories: *const AmbientGitHubRepository,
    count: u32,
    out: *mut AmbientIntegrationProfileRef,
) -> u32 {
    boundary(|| {
        output(out)?;
        if count > 32 || (count > 0 && (repositories.is_null() || !repositories.is_aligned())) {
            return Err(INVALID_ARGUMENT);
        }
        let name = unsafe { input(name, 256) }?;
        let raw = if count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(repositories, count as usize) }
        };
        let repositories = raw
            .iter()
            .map(|r| {
                Ok(GitHubRepository {
                    owner: unsafe { input(r.owner, 100) }?.into(),
                    repository: unsafe { input(r.repository, 100) }?.into(),
                })
            })
            .collect::<Result<Vec<_>, u32>>()?;
        let p = store(h, |s| {
            s.save_integration_profile(
                id,
                revision,
                name,
                None,
                &IntegrationConfig::GitHub(repositories),
            )
        })?;
        unsafe {
            out.write(reference(p));
        }
        Ok(())
    })
}
/// # Safety
/// Output obeys ambient_integration.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_credential_begin(
    h: u64,
    id: u64,
    revision: u64,
    out: *mut AmbientIntegrationProfileRef,
) -> u32 {
    boundary(|| {
        output(out)?;
        let p = store(h, |s| s.replace_integration_credential(id, revision))?;
        unsafe {
            out.write(reference(p));
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_integration_v1_profile_delete_finish(
    h: u64,
    p: AmbientIntegrationProfileRef,
) -> u32 {
    boundary(|| {
        store(h, |s| {
            s.finish_integration_profile_deletion(
                p.id,
                p.settings_revision,
                p.credential_generation,
            )
        })
    })
}
/// # Safety
/// Input read and output ID obey ambient_integration.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_begin(
    h: u64,
    read: *const AmbientIntegrationRead,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let r = unsafe { borrowed(read) }?;
        let target = unsafe { input(r.target, 512) }?;
        let slot = get_store(h)?;
        // The cancel entry is registered before the caller can learn the operation ID.
        let mut signals = slot.integration_cancel.lock().map_err(|_| PANIC)?;
        let (id, kind, flag) = store(h, |s| {
            let id = s.begin_integration_read(
                TaskVersion {
                    task_id: task_id(r.task_id).map_err(|_| StoreError::InvalidInput)?,
                    revision: r.task_revision,
                },
                IntegrationProfileVersion {
                    id: r.profile.id,
                    settings_revision: r.profile.settings_revision,
                    credential_generation: r.profile.credential_generation,
                },
                target,
                now().map_err(|_| StoreError::InvalidInput)?,
                r.session_expires_at,
            )?;
            Ok((
                id,
                s.integration_read_kind(id)?,
                s.integration_read_cancellation(id)?,
            ))
        })?;
        signals.insert(kind, (id, flag));
        unsafe {
            out.write(id);
        }
        Ok(())
    })
}
fn dispatch(
    h: u64,
    id: u64,
    first: &str,
    second: Option<&str>,
    route: Option<&str>,
) -> Result<u32, u32> {
    let slot = get_store(h)?;
    let (kind, flag) = {
        let s = slot.store.lock().map_err(|_| PANIC)?;
        if slot.closed.load(Ordering::Acquire) {
            return Err(INVALID_HANDLE);
        }
        (
            s.integration_read_kind(id).map_err(error)?,
            s.integration_read_cancellation(id).map_err(error)?,
        )
    };
    // Constructing the transport checks the secret form for the kind; it never touches the network.
    let transport = (|| {
        #[cfg(feature = "model-test-support")]
        if let Some(route) = route {
            return IntegrationTransport::test(kind, first, second, flag.clone(), route);
        }
        #[cfg(not(feature = "model-test-support"))]
        if route.is_some() {
            return Err(IntegrationTransportError::Invalid);
        }
        IntegrationTransport::new(kind, first, second, flag.clone())
    })();
    {
        let mut s = slot.store.lock().map_err(|_| PANIC)?;
        if slot.closed.load(Ordering::Acquire) {
            return Err(INVALID_HANDLE);
        }
        if matches!(transport, Err(IntegrationTransportError::Invalid)) {
            s.abort_unused_integration_read(id).map_err(error)?;
            return Err(INVALID_ARGUMENT);
        }
        if let Err(e) = s.start_integration_transport(id) {
            s.abort_unused_integration_read(id).map_err(error)?;
            return Err(error(e));
        }
    }
    let mut transport = transport;
    if let Ok(transport) = &mut transport {
        // Core refuses the next step when the operation ends, fails or reaches its maximum.
        loop {
            let step = {
                let mut s = slot.store.lock().map_err(|_| PANIC)?;
                if slot.closed.load(Ordering::Acquire) {
                    s.cancel_integration_read(id);
                }
                s.permit_integration_step(id, now()?)
            };
            let Ok(step) = step else {
                break;
            };
            let response = transport.get(step);
            let mut s = slot.store.lock().map_err(|_| PANIC)?;
            match response {
                Ok(response) => {
                    if s.accept_integration_response(id, response, now()?).is_err() {
                        // An unclassified local acceptance failure reports an invalid representation.
                        if s.integration_read_status(id).unwrap_or(500) == 500 {
                            let _ = s.fail_integration_read(id, 422);
                        }
                        break;
                    }
                }
                Err(e) => {
                    let code = match e {
                        IntegrationTransportError::Cancelled => 499,
                        IntegrationTransportError::Timeout => 408,
                        IntegrationTransportError::Limit => 413,
                        _ => 500,
                    };
                    let _ = s.fail_integration_read(id, code);
                    break;
                }
            }
        }
    } else {
        let mut s = slot.store.lock().map_err(|_| PANIC)?;
        let _ = s.fail_integration_read(id, 500);
    }
    // Drop waits for the current-thread runtime's actual cleanup before Core releases busy.
    drop(transport);
    let mut s = slot.store.lock().map_err(|_| PANIC)?;
    s.finish_integration_transport(id).map_err(error)?;
    s.integration_read_status(id).map_err(error)
}
/// # Safety
/// Secret parts and output obey ambient_integration.h. Secrets are borrowed only during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_dispatch(
    h: u64,
    id: u64,
    first: AmbientText,
    second: AmbientText,
    outcome: *mut u32,
) -> u32 {
    boundary(|| {
        output(outcome)?;
        let first = unsafe { input(first, 16384) }?;
        let second = unsafe { secret_part(second) }?;
        let result = dispatch(h, id, first, second, None)?;
        unsafe {
            outcome.write(result);
        }
        Ok(())
    })
}
#[cfg(feature = "model-test-support")]
/// # Safety
/// Same as dispatch; route is a test-only loopback base URL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_test_dispatch(
    h: u64,
    id: u64,
    first: AmbientText,
    second: AmbientText,
    route: AmbientText,
    outcome: *mut u32,
) -> u32 {
    boundary(|| {
        output(outcome)?;
        let first = unsafe { input(first, 16384) }?;
        let second = unsafe { secret_part(second) }?;
        let route = unsafe { input(route, 512) }?;
        let result = dispatch(h, id, first, second, Some(route))?;
        unsafe {
            outcome.write(result);
        }
        Ok(())
    })
}
/// Cancellation reads only the cancel map, so it never waits for the Store lock or network.
#[unsafe(no_mangle)]
pub extern "C" fn ambient_integration_v1_cancel(h: u64, id: u64) -> u32 {
    boundary(|| {
        let slot = get_store(h)?;
        let signals = slot.integration_cancel.lock().map_err(|_| PANIC)?;
        let (_, flag) = signals
            .values()
            .find(|(i, _)| *i == id)
            .ok_or(error(StoreError::StaleRevision))?;
        flag.store(true, Ordering::Release);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_integration_v1_abort(h: u64, id: u64) -> u32 {
    boundary(|| store(h, |s| s.abort_unused_integration_read(id)))
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_integration_v1_release(h: u64, id: u64) -> u32 {
    boundary(|| {
        let slot = get_store(h)?;
        let mut signals = slot.integration_cancel.lock().map_err(|_| PANIC)?;
        store(h, |s| s.release_integration_read(id))?;
        signals.retain(|_, (i, _)| *i != id);
        Ok(())
    })
}
/// # Safety
/// Output obeys ambient_integration.h (256KiB minimum).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_window(
    h: u64,
    id: u64,
    after: u32,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, MAX_PAGE_BYTES)?;
        let time = now()?;
        let s = store(h, |s| {
            s.validate_integration_session_time(id, time)?;
            s.integration_window_page(id, after as usize)
        })?;
        unsafe {
            json_output(&s, b, used);
        }
        Ok(())
    })
}
/// # Safety
/// Selection and output obey ambient_integration.h (512KiB minimum).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_preview(
    h: u64,
    id: u64,
    selected: *const u32,
    count: u32,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, MAX_INTEGRATION_WINDOW)?;
        let indices = unsafe { indices(selected, count) }?;
        let time = now()?;
        let s = store(h, |s| {
            s.validate_integration_session_time(id, time)?;
            s.integration_preview(id, &indices)
        })?;
        unsafe {
            json_output(&s, b, used);
        }
        Ok(())
    })
}
/// # Safety
/// Selection and fixed batch output obey ambient_integration.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_bind(
    h: u64,
    id: u64,
    selected: *const u32,
    count: u32,
    at: i64,
    out: *mut AmbientIntegrationBatch,
) -> u32 {
    boundary(|| {
        output(out)?;
        let indices = unsafe { indices(selected, count) }?;
        let time = now()?;
        let batch = store(h, |s| {
            s.validate_integration_session_time(id, time)?;
            s.bind_integration_objects(id, &indices, at)
        })?;
        let mut result = AmbientIntegrationBatch {
            task_id: batch.task.task_id.0 as u64,
            task_revision: batch.task.revision,
            count: batch.evidence_ids.len() as u32,
            ..Default::default()
        };
        for (i, e) in batch.evidence_ids.iter().enumerate() {
            result.evidence_ids[i] = e.0 as u64;
        }
        unsafe {
            out.write(result);
        }
        Ok(())
    })
}
/// # Safety
/// Saved metadata output obeys ambient_integration.h (256KiB minimum).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_integration_v1_saved_metadata(
    h: u64,
    raw: u32,
    task: u64,
    after: i64,
    b: *mut u8,
    cap: u32,
    used: *mut u32,
) -> u32 {
    boundary(|| {
        capacity(b, cap, used, MAX_PAGE_BYTES)?;
        let kind = kind(raw)?;
        let task = task_id(task)?;
        let s = store(h, |s| s.integration_saved_metadata(kind, task, after))?;
        unsafe {
            json_output(&s, b, used);
        }
        Ok(())
    })
}
#[cfg(test)]
#[path = "integration_tests.rs"]
mod tests;
