use ambient_core::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
pub(crate) const INVALID: u32 = 1;
const PAUSED: u32 = 2;
const CANCELLED: u32 = 3;
const BUSY: u32 = 4;
pub(crate) const STORAGE: u32 = 5;
pub(crate) const LIMIT: u32 = 6;
pub(crate) const HANDLE: u32 = 7;
pub(crate) const PANIC: u32 = 8;
const REPORT: u32 = 9;
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientModelAttempt {
    pub id: u64,
    pub at_utc: i64,
    pub stage: u32,
    pub outcome: u32,
    pub expected_model: u32,
    pub expected_effort: u32,
    pub expected_tier: u32,
    pub actual_model: u32,
    pub actual_effort: u32,
    pub actual_tier: u32,
    pub has_usage: u32,
    pub purpose: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientModelSummary {
    pub attempts: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub unknown: u64,
    pub latest: AmbientModelAttempt,
    pub has_latest: u32,
    pub reserved: u32,
}
struct Slot {
    store: Arc<crate::StoreSlot>,
    cancelled: Arc<AtomicBool>,
    admission: Mutex<Option<CheckAdmission>>,
    busy: AtomicBool,
    closed: AtomicBool,
    at_utc: i64,
    /// Only for an observation attempt, which must not touch the manual assistance preview.
    observation: Option<ObservationClock>,
}
/// The caller's monotonic clock at begin; the finish inside dispatch continues both of the
/// caller's clocks (with `Slot::at_utc`) by the elapsed time.
struct ObservationClock {
    monotonic_ms: u64,
    started: std::time::Instant,
}
static MODEL: Mutex<(u64, BTreeMap<u64, Arc<Slot>>)> = Mutex::new((1, BTreeMap::new()));
pub(crate) fn boundary(call: impl FnOnce() -> Result<(), u32>) -> u32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => e,
        Err(_) => PANIC,
    }
}
pub(crate) fn error(e: StoreError) -> u32 {
    match e {
        StoreError::Paused => PAUSED,
        StoreError::Cancelled => CANCELLED,
        StoreError::Busy => BUSY,
        StoreError::ReportRequired => REPORT,
        StoreError::ResourceLimit | StoreError::TextTooLong => LIMIT,
        StoreError::CapabilityRequired => 10,
        StoreError::StaleRevision | StoreError::Deleted | StoreError::NotFound => 11,
        StoreError::SessionRequired => 12,
        StoreError::TaskCompleted => 13,
        StoreError::InvalidInput => INVALID,
        _ => STORAGE,
    }
}
fn get(handle: u64) -> Result<Arc<Slot>, u32> {
    MODEL
        .lock()
        .map_err(|_| PANIC)?
        .1
        .get(&handle)
        .cloned()
        .ok_or(HANDLE)
}
struct Running(Arc<Slot>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}
fn running(h: u64) -> Result<Running, u32> {
    let s = get(h)?;
    s.busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| BUSY)?;
    let r = Running(s);
    if r.0.closed.load(Ordering::Acquire) {
        return Err(HANDLE);
    }
    Ok(r)
}
fn encode(
    id: u64,
    at_utc: i64,
    stage: u32,
    purpose: ModelPurpose,
    profile: ModelProfile,
    r: ModelCheckResult,
) -> AmbientModelAttempt {
    AmbientModelAttempt {
        id,
        at_utc,
        stage,
        outcome: r.outcome as u32,
        expected_model: 1,
        expected_effort: profile as u32,
        expected_tier: 1,
        actual_model: r.actual.model as u32,
        actual_effort: r.actual.effort as u32,
        actual_tier: r.actual.tier as u32,
        has_usage: u32::from(r.usage.is_some()),
        input_tokens: r.usage.map_or(0, CheckUsage::input),
        output_tokens: r.usage.map_or(0, CheckUsage::output),
        total_tokens: r.usage.map_or(0, CheckUsage::total),
        purpose: purpose as u32,
    }
}
fn completion(
    mut attempt: CheckAttempt,
    mut published: Option<WorkspaceVersion>,
    cancelled: &AtomicBool,
) -> AmbientModelCompletion {
    // Cancellation may win after accounting; publication is not a persisted permission to act.
    if cancelled.load(Ordering::Acquire) {
        attempt.result.outcome = CheckOutcome::Cancelled;
        published = None;
    }
    AmbientModelCompletion {
        attempt: encode(
            attempt.id.get(),
            attempt.at_utc,
            attempt.stage,
            attempt.purpose,
            attempt.profile,
            attempt.result,
        ),
        published_task: published.map_or(0, |v| v.task.task_id.0 as u64),
        workspace_revision: published.map_or(0, |v| v.workspace_revision),
    }
}
/// # Safety
/// out is aligned writable caller memory, disjoint from every other input/output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_begin(
    memory: u64,
    user_reported: u32,
    profile: u32,
    at_utc: i64,
    request: u64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        if user_reported > 1 {
            return Err(INVALID);
        }
        let profile = match profile {
            1 => ModelProfile::Low,
            2 => ModelProfile::High,
            _ => return Err(INVALID),
        };
        if request > 0 && profile != ModelProfile::High {
            return Err(INVALID);
        }
        let store = crate::get_store(memory).map_err(|_| HANDLE)?;
        let id = reserve_id()?;
        let admission = {
            let mut s = store.store.lock().map_err(|_| PANIC)?;
            if store.closed.load(Ordering::Acquire) {
                return Err(HANDLE);
            }
            if request == 0 {
                s.prepare_check(user_reported == 1, profile)
                    .map_err(error)?
            } else {
                s.begin_assistance(request).map_err(error)?
            }
        };
        register(id, store, admission, at_utc, None)?;
        unsafe { out.write(id) };
        Ok(())
    })
}
/// An operation ID under the shared limit of 64 open operations; an unused ID is never reused.
pub(crate) fn reserve_id() -> Result<u64, u32> {
    let mut reg = MODEL.lock().map_err(|_| PANIC)?;
    if reg.1.len() >= 64 {
        return Err(LIMIT);
    }
    let id = reg.0;
    reg.0 = id.checked_add(1).ok_or(LIMIT)?;
    Ok(id)
}
/// Serves an admission under `id` through dispatch/cancel/close; an observation brings its clocks.
pub(crate) fn register(
    id: u64,
    store: Arc<crate::StoreSlot>,
    admission: CheckAdmission,
    at_utc: i64,
    observation_ms: Option<u64>,
) -> Result<(), u32> {
    let cancelled = admission.cancellation();
    let slot = Arc::new(Slot {
        store: store.clone(),
        cancelled: cancelled.clone(),
        admission: Mutex::new(Some(admission)),
        busy: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        at_utc,
        observation: observation_ms.map(|monotonic_ms| ObservationClock {
            monotonic_ms,
            started: std::time::Instant::now(),
        }),
    });
    if let Ok(mut reg) = MODEL.lock() {
        reg.1.insert(id, slot);
        Ok(())
    } else {
        store
            .store
            .lock()
            .map_err(|_| PANIC)?
            .release_check(&cancelled);
        Err(PANIC)
    }
}
/// One borrowed plugin key per (plugin, server); the secret stays in this call's arguments.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AmbientPluginKey {
    pub plugin: u64,
    pub server: crate::AmbientText,
    pub generation: u64,
    pub key: crate::AmbientText,
}
const _: () = assert!(std::mem::size_of::<AmbientPluginKey>() == 48);
type Keys<'a> = Vec<(PluginKeyRef, &'a str)>;
/// Validates the key array before any Core or network use: ≤64 entries, positive IDs, a valid
/// server key, printable ASCII secrets of 1..16384 bytes and no duplicate (plugin, server).
unsafe fn keys<'a>(p: *const AmbientPluginKey, count: u32) -> Result<Keys<'a>, u32> {
    if count as usize > MAX_PLUGIN_KEYS {
        return Err(INVALID);
    }
    let raw = unsafe { crate::array(p, count, MAX_PLUGIN_KEYS as u32) }.map_err(|_| INVALID)?;
    let mut out: Keys<'a> = Vec::new();
    for k in raw {
        let server = unsafe { crate::input(k.server, 32) }.map_err(|_| INVALID)?;
        let key = unsafe { crate::input(k.key, 16384) }.map_err(|_| INVALID)?;
        if k.plugin == 0
            || k.plugin > i64::MAX as u64
            || k.generation == 0
            || k.generation > i64::MAX as u64
            || !server
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !key.bytes().all(|b| (33..=126).contains(&b))
            || out
                .iter()
                .any(|(r, _)| r.plugin == k.plugin && r.server == server)
        {
            return Err(INVALID);
        }
        out.push((
            PluginKeyRef {
                plugin: k.plugin,
                server: server.into(),
                generation: k.generation,
            },
            key,
        ));
    }
    Ok(out)
}
fn now_utc(fallback: i64) -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(fallback)
}
/// The root folder of local plugin servers: an absolute path of 1..1024 bytes without NUL.
pub(crate) unsafe fn root<'a>(t: crate::AmbientText) -> Result<&'a std::path::Path, u32> {
    let text = unsafe { crate::input(t, 1024) }.map_err(|_| INVALID)?;
    if !text.starts_with('/') || text.contains('\0') {
        return Err(INVALID);
    }
    Ok(std::path::Path::new(text))
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_with(
    handle: u64,
    token: &str,
    keys: &Keys<'_>,
    root: &std::path::Path,
    out: &mut AmbientModelCompletion,
    check: impl FnOnce(CheckDispatch) -> ModelResponse,
    observe: impl FnOnce(CheckDispatch) -> ModelResponse,
    model: impl Fn(&ModelStep<'_>) -> ModelResponse,
    mut tool: impl FnMut(
        &mut ambient_providers::LocalHost,
        &ToolCallPermit,
        Option<&str>,
    ) -> ToolCallResult,
) -> u32 {
    boundary(|| {
        if token.is_empty()
            || token.len() > 16384
            || !token.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(INVALID);
        }
        let running = running(handle)?;
        let slot = &running.0;
        let admission = slot
            .admission
            .lock()
            .map_err(|_| PANIC)?
            .take()
            .ok_or(INVALID)?;
        // Admission refuses a closed memory handle; accounting of a sent POST or call never does,
        // because the model lease keeps the Store for late usage.
        let account = || slot.store.store.lock().map_err(|_| PANIC);
        let admit = || -> Result<std::sync::MutexGuard<'_, Store>, u32> {
            let s = account()?;
            if slot.store.closed.load(Ordering::Acquire) {
                return Err(HANDLE);
            }
            Ok(s)
        };
        if let Some(clock) = &slot.observation {
            // One POST without tools or a loop: the answer only proposes writes to Ambient's
            // memory. The core discards a late answer itself; the summary skips observation rows,
            // so the finish result is the receipt and the stored outcome is reported unchanged.
            let dispatch = admit()?
                .dispatch_check(admission, slot.at_utc)
                .map_err(error)?;
            let id = dispatch.id();
            let response = observe(dispatch);
            let elapsed = clock.started.elapsed();
            let now = ObservationTime {
                utc: slot.at_utc.saturating_add(elapsed.as_secs() as i64),
                monotonic_ms: clock
                    .monotonic_ms
                    .saturating_add(elapsed.as_millis() as u64),
            };
            let finish = account()?
                .finish_observation(id, response.check, response.observation, now)
                .map_err(error)?;
            *out = AmbientModelCompletion {
                attempt: encode(
                    id.get(),
                    slot.at_utc,
                    2,
                    ModelPurpose::Observation,
                    ModelProfile::High,
                    finish.result,
                ),
                published_task: 0,
                workspace_revision: 0,
            };
            return Ok(());
        }
        let receipt = if admission.purpose() == ModelPurpose::Connectivity {
            let dispatch = admit()?
                .dispatch_check(admission, slot.at_utc)
                .map_err(error)?;
            let id = dispatch.id();
            let mut result = check(dispatch);
            let mut s = account()?;
            if slot.cancelled.load(Ordering::Acquire) {
                result.check.outcome = CheckOutcome::Cancelled;
            }
            s.finish_check(id, result.check).map_err(error)?;
            None
        } else {
            let refs = keys.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>();
            let mut l = admit()?.begin_model_loop(admission, &refs).map_err(error)?;
            // Declared after `running`: on every exit path its Drop stops the local servers
            // before busy is released.
            let mut host = ambient_providers::LocalHost::new(root);
            let mut posts = 0;
            let mut receipt = None;
            // Core decides every step and refuses the next one at its limits; the Store lock is
            // never held across I/O. No step constant exists here.
            'requests: loop {
                let step = {
                    let mut s = match admit() {
                        Ok(s) => s,
                        Err(e) if posts == 0 => return Err(e),
                        Err(_) => break,
                    };
                    match s.dispatch_model_step(&mut l, now_utc(slot.at_utc)) {
                        Ok(step) => step,
                        Err(e) if posts == 0 => return Err(error(e)),
                        Err(_) => break,
                    }
                };
                posts += 1;
                let id = step.id;
                let mut response = model(&step);
                drop(step);
                let next = {
                    let mut s = account()?;
                    if slot.cancelled.load(Ordering::Acquire) {
                        response.check.outcome = CheckOutcome::Cancelled;
                    }
                    s.finish_model_step(&mut l, id, response).map_err(error)?
                };
                if let StepNext::Done(published) = next {
                    receipt = published;
                    break;
                }
                loop {
                    let permit = match admit()
                        .map(|mut s| s.permit_tool_call(&mut l, now_utc(slot.at_utc)))
                    {
                        Ok(Ok(Some(p))) => p,
                        Ok(Ok(None)) => break,
                        _ => break 'requests,
                    };
                    let secret = permit
                        .key
                        .as_ref()
                        .and_then(|k| keys.iter().find(|(r, _)| r == k).map(|(_, secret)| *secret));
                    let result = tool(&mut host, &permit, secret);
                    if account()?
                        .accept_tool_result(&mut l, permit, result)
                        .is_err()
                    {
                        break 'requests;
                    }
                }
            }
            let unconfirmed = host.finish();
            account()?
                .record_unconfirmed_stops(&unconfirmed)
                .map_err(error)?;
            receipt
        };
        let s = account()?;
        let committed = s.check_summary().map_err(error)?.latest.ok_or(PANIC)?;
        *out = completion(committed, receipt, &slot.cancelled);
        Ok(())
    })
}
/// # Safety
/// token points to token_length readable bytes; keys points to count aligned records whose
/// texts stay readable for the call; root is readable; out is aligned writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_dispatch(
    handle: u64,
    token: *const u8,
    token_length: u32,
    keys: *const AmbientPluginKey,
    key_count: u32,
    root: crate::AmbientText,
    out: *mut AmbientModelCompletion,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let text = unsafe {
            crate::input(
                crate::AmbientText {
                    bytes: token,
                    length: token_length,
                },
                16384,
            )
        }
        .map_err(|_| INVALID)?;
        let keys = unsafe { self::keys(keys, key_count) }?;
        let root = unsafe { self::root(root) }?;
        let mut record = AmbientModelCompletion::default();
        let status = dispatch_with(
            handle,
            text,
            &keys,
            root,
            &mut record,
            |permit| ambient_providers::run_model(permit, text),
            |permit| ambient_providers::run_observation(permit, text),
            |step| ambient_providers::run_model_step(step, text),
            |host, permit, key| host.call_tool(permit, key),
        );
        if status != 0 {
            return Err(status);
        }
        unsafe { out.write(record) };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_cancel(handle: u64) -> u32 {
    boundary(|| {
        let slot = get(handle)?;
        slot.cancelled.store(true, Ordering::Release);
        if slot.observation.is_none() {
            slot.store
                .store
                .lock()
                .map_err(|_| PANIC)?
                .clear_assistance();
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_close(handle: u64) -> u32 {
    boundary(|| {
        let running = running(handle)?;
        let slot = &running.0;
        // No dispatch is running: disposal preserves committed results, but abandons an unused admission.
        let unused = slot.admission.lock().map_err(|_| PANIC)?.take().is_some();
        if unused {
            slot.cancelled.store(true, Ordering::Release);
        }
        slot.closed.store(true, Ordering::Release);
        MODEL
            .lock()
            .map_err(|_| PANIC)?
            .1
            .remove(&handle)
            .ok_or(HANDLE)?;
        let mut store = slot.store.store.lock().map_err(|_| PANIC)?;
        if unused && slot.observation.is_none() {
            store.clear_assistance();
        }
        store.release_check(&slot.cancelled);
        Ok(())
    })
}
/// # Safety
/// out is aligned writable caller memory, disjoint from every other input/output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_summary(
    memory: u64,
    out: *mut AmbientModelSummary,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let store = crate::get_store(memory).map_err(|_| HANDLE)?;
        let s = store.store.lock().map_err(|_| PANIC)?;
        if store.closed.load(Ordering::Acquire) {
            return Err(HANDLE);
        }
        let r = s.check_summary().map_err(error)?;
        let latest = r.latest.map(|v| {
            encode(
                v.id.get(),
                v.at_utc,
                v.stage,
                v.purpose,
                v.profile,
                v.result,
            )
        });
        unsafe {
            out.write(AmbientModelSummary {
                attempts: r.attempts,
                input_tokens: r.input,
                output_tokens: r.output,
                total_tokens: r.total,
                unknown: r.unknown,
                latest: latest.unwrap_or_default(),
                has_latest: u32::from(latest.is_some()),
                reserved: 0,
            })
        };
        Ok(())
    })
}
#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientModelSpan {
    pub offset: u32,
    pub length: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientModelCompletion {
    pub attempt: AmbientModelAttempt,
    pub published_task: u64,
    pub workspace_revision: u64,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientAssistancePreview {
    pub request_id: u64,
    pub task_id: u64,
    pub revision: u64,
    pub workspace_revision: u64,
    pub context: AmbientModelSpan,
    pub mode: u32,
    pub reserved: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientAssistanceRequest {
    pub task_id: u64,
    pub revision: u64,
    pub workspace_revision: u64,
    pub proposal_id: u64,
    pub content_revision: u64,
    pub mode: u32,
    pub reserved: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientProposalTarget {
    pub proposal_id: u64,
    pub content_revision: u64,
}
fn target(t: AmbientProposalTarget) -> Result<ProposalTarget, u32> {
    if t.content_revision == 0 || t.content_revision > i64::MAX as u64 {
        return Err(INVALID);
    }
    Ok(ProposalTarget {
        id: ProposalId::from_raw(t.proposal_id).map_err(error)?,
        content_revision: t.content_revision,
    })
}
pub(crate) fn memory<T>(
    handle: u64,
    call: impl FnOnce(&mut Store) -> Result<T, u32>,
) -> Result<T, u32> {
    let slot = crate::get_store(handle).map_err(|_| HANDLE)?;
    let mut s = slot.store.lock().map_err(|_| PANIC)?;
    if slot.closed.load(Ordering::Acquire) {
        return Err(HANDLE);
    }
    call(&mut s)
}
fn buffer(p: *mut u8, capacity: u32, min: usize) -> Result<(), u32> {
    if p.is_null() || (capacity as usize) < min {
        Err(INVALID)
    } else {
        Ok(())
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_session(
    handle: u64,
    epoch: u64,
    valid: u32,
    reported: u32,
) -> u32 {
    boundary(|| {
        if valid > 1 || reported > 1 {
            return Err(INVALID);
        }
        memory(handle, |s| {
            s.set_model_session(epoch, valid == 1, reported == 1)
                .map_err(error)
        })
    })
}
/// # Safety
/// out is aligned writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_available(handle: u64, out: *mut u32) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let v = memory(handle, |s| s.assistance_available().map_err(error))?;
        unsafe { out.write(u32::from(v)) };
        Ok(())
    })
}

/// # Safety
/// Caller buffers/records must be live, aligned, disjoint and have the documented capacities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_prepare(
    handle: u64,
    input: AmbientAssistanceRequest,
    notes: *const u64,
    count: u32,
    text: *const u8,
    length: u32,
    out: *mut AmbientAssistancePreview,
    bytes: *mut u8,
    capacity: u32,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        buffer(bytes, capacity, MAX_ASSISTANCE_CONTEXT)?;
        if input.reserved != 0 || length as usize > MAX_REPLY_TEXT {
            return Err(INVALID);
        }
        let mode = match input.mode {
            1 => AssistanceMode::Continue,
            2 => AssistanceMode::NewContext,
            _ => return Err(INVALID),
        };
        let notes = unsafe { crate::array(notes, count, 32) }
            .map_err(|_| INVALID)?
            .iter()
            .map(|id| crate::evidence_id(*id).map_err(|_| INVALID))
            .collect::<Result<Vec<_>, _>>()?;
        let reply = if input.proposal_id == 0 {
            if input.content_revision != 0 || length != 0 {
                return Err(INVALID);
            }
            None
        } else {
            let text = unsafe {
                crate::input(
                    crate::AmbientText {
                        bytes: text,
                        length,
                    },
                    MAX_REPLY_TEXT,
                )
            }
            .map_err(|_| INVALID)?;
            Some(ProposalReply {
                target: target(AmbientProposalTarget {
                    proposal_id: input.proposal_id,
                    content_revision: input.content_revision,
                })?,
                text: text.to_owned(),
            })
        };
        let request = AssistanceRequest {
            task: TaskVersion {
                task_id: crate::task_id(input.task_id).map_err(|_| INVALID)?,
                revision: input.revision,
            },
            workspace_revision: input.workspace_revision,
            mode,
            selected_evidence: notes,
            reply,
        };
        let p = memory(handle, |s| {
            let p = s.prepare_assistance(request).map_err(error)?;
            if ambient_providers::validate_assistance_body(&p.context).is_err() {
                s.clear_assistance();
                return Err(LIMIT);
            }
            Ok(p)
        })?;
        let record = AmbientAssistancePreview {
            request_id: p.request_id,
            task_id: p.task.task_id.0 as u64,
            revision: p.task.revision,
            workspace_revision: p.workspace_revision,
            context: AmbientModelSpan {
                offset: 0,
                length: p.context.len() as u32,
            },
            mode: match p.mode {
                AssistanceMode::Continue => 1,
                AssistanceMode::NewContext => 2,
            },
            reserved: 0,
        };
        unsafe {
            std::ptr::copy_nonoverlapping(p.context.as_ptr(), bytes, p.context.len());
            out.write(record)
        };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_clear(handle: u64) -> u32 {
    boundary(|| {
        memory(handle, |s| {
            s.clear_assistance();
            Ok(())
        })
    })
}
/// # Safety
/// out is live aligned writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_workspace_revision(
    handle: u64,
    task: u64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let id = crate::task_id(task).map_err(|_| INVALID)?;
        let value = memory(handle, |s| {
            s.assistant_workspace_revision(id).map_err(error)
        })?;
        unsafe { out.write(value) };
        Ok(())
    })
}
/// # Safety
/// bytes/length are live, writable, disjoint caller-owned output buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_feed(
    handle: u64,
    bytes: *mut u8,
    capacity: u32,
    length: *mut u32,
) -> u32 {
    boundary(|| {
        buffer(bytes, capacity, MAX_PROPOSAL_FEED)?;
        crate::output(length).map_err(|_| INVALID)?;
        let value = memory(handle, |s| s.proposal_feed_json().map_err(error))?;
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), bytes, value.len());
            length.write(value.len() as u32)
        };
        Ok(())
    })
}
/// # Safety
/// bytes/length are live, writable, disjoint caller-owned output buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_details(
    handle: u64,
    t: AmbientProposalTarget,
    bytes: *mut u8,
    capacity: u32,
    length: *mut u32,
) -> u32 {
    boundary(|| {
        buffer(bytes, capacity, MAX_ASSISTANCE_CONTEXT)?;
        crate::output(length).map_err(|_| INVALID)?;
        let value = memory(handle, |s| {
            s.proposal_details_json(target(t)?).map_err(error)
        })?;
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), bytes, value.len());
            length.write(value.len() as u32)
        };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_visibility(
    handle: u64,
    t: AmbientProposalTarget,
    visible: u32,
) -> u32 {
    boundary(|| {
        if visible > 1 {
            return Err(INVALID);
        }
        memory(handle, |s| {
            s.set_proposal_visibility(target(t)?, visible == 1)
                .map_err(error)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_clear_discussion(
    handle: u64,
    task: u64,
    revision: u64,
    workspace_revision: u64,
) -> u32 {
    boundary(|| {
        let task = TaskVersion {
            task_id: crate::task_id(task).map_err(|_| INVALID)?,
            revision,
        };
        memory(handle, |s| {
            s.clear_discussion(task, workspace_revision).map_err(error)
        })
    })
}
/// # Safety
/// bytes/length are live, writable, disjoint caller-owned output buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_copy(
    handle: u64,
    t: AmbientProposalTarget,
    bytes: *mut u8,
    capacity: u32,
    length: *mut u32,
) -> u32 {
    boundary(|| {
        buffer(bytes, capacity, MAX_REPLY_TEXT)?;
        crate::output(length).map_err(|_| INVALID)?;
        let value = memory(handle, |s| s.copy_proposal(target(t)?).map_err(error))?;
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), bytes, value.len());
            length.write(value.len() as u32)
        };
        Ok(())
    })
}
#[cfg(feature = "model-test-support")]
#[path = "../tests/support/model.rs"]
pub(crate) mod support;

/// # Safety
/// out is aligned live writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_model_v6_validate(
    handle: u64,
    t: AmbientProposalTarget,
    out: *mut u32,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let kind = memory(handle, |s| {
            s.validate_proposal_action(target(t)?).map_err(error)
        })?;
        unsafe { out.write(kind as u32) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ambient_model_v6_discard_preview(handle: u64, request: u64) -> u32 {
    boundary(|| {
        memory(handle, |s| {
            s.discard_assistance_preview(request).map_err(error)
        })
    })
}
