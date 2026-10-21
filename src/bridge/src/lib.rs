//! Synchronous typed native ABI v3. Every output buffer belongs to the caller.
mod abi_types;
mod auth;
mod codex;
mod integration;
mod model;
mod plugin;
#[cfg(all(test, feature = "model-test-support"))]
#[path = "plugin_tests.rs"]
mod plugin_tests;
mod source;
pub use abi_types::*;
use ambient_core::*;
pub use auth::*;
pub use codex::*;
pub use integration::*;
pub use model::*;
pub use plugin::*;
pub use source::*;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
pub const OK: u32 = 0;
pub const INVALID_ARGUMENT: u32 = 1;
pub const INVALID_UTF8: u32 = 2;
pub const TOO_LONG: u32 = 3;
pub const BUFFER_TOO_SMALL: u32 = 4;
pub const STORAGE_ERROR: u32 = 5;
pub const UNSUPPORTED_SCHEMA: u32 = 6;
pub const INVALID_HANDLE: u32 = 7;
pub const PANIC: u32 = 8;
pub const RESOURCE_LIMIT: u32 = 9;
pub const NOT_FOUND: u32 = 10;
pub const DELETED: u32 = 11;
pub const STALE_REVISION: u32 = 12;
pub const TASK_COMPLETED: u32 = 13;
struct StoreSlot {
    store: Mutex<Store>,
    source_cancel: Mutex<Option<(u64, Arc<AtomicBool>)>>,
    codex_cancel: Mutex<Option<(u64, Arc<AtomicBool>)>>,
    /// At most one operation per integration kind; cancel finds it by ID without the Store lock.
    integration_cancel: Mutex<BTreeMap<IntegrationKind, (u64, Arc<AtomicBool>)>>,
    closed: AtomicBool,
}
struct Registry {
    next_handle: u64,
    stores: BTreeMap<u64, Arc<StoreSlot>>,
}
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    next_handle: 1,
    stores: BTreeMap::new(),
});
fn boundary(op: impl FnOnce() -> Result<(), u32>) -> u32 {
    match catch_unwind(AssertUnwindSafe(op)) {
        Ok(Ok(())) => OK,
        Ok(Err(s)) => s,
        Err(_) => PANIC,
    }
}
fn store_error(e: StoreError) -> u32 {
    match e {
        StoreError::InvalidInput
        | StoreError::Paused
        | StoreError::Cancelled
        | StoreError::ReportRequired
        | StoreError::CapabilityRequired
        | StoreError::SessionRequired => INVALID_ARGUMENT,
        StoreError::Busy => RESOURCE_LIMIT,
        StoreError::TextTooLong => TOO_LONG,
        StoreError::Storage => STORAGE_ERROR,
        StoreError::UnsupportedSchema => UNSUPPORTED_SCHEMA,
        StoreError::ResourceLimit => RESOURCE_LIMIT,
        StoreError::NotFound => NOT_FOUND,
        StoreError::Deleted => DELETED,
        StoreError::StaleRevision => STALE_REVISION,
        StoreError::TaskCompleted => TASK_COMPLETED,
    }
}
fn with_store<T>(
    handle: u64,
    op: impl FnOnce(&mut Store) -> Result<T, StoreError>,
) -> Result<T, u32> {
    let slot = get_store(handle)?;
    let mut store = slot.store.lock().map_err(|_| PANIC)?;
    if slot.closed.load(Ordering::Acquire) {
        return Err(INVALID_HANDLE);
    }
    op(&mut store).map_err(store_error)
}
fn get_store(handle: u64) -> Result<Arc<StoreSlot>, u32> {
    REGISTRY
        .lock()
        .map_err(|_| PANIC)?
        .stores
        .get(&handle)
        .cloned()
        .ok_or(INVALID_HANDLE)
}
fn output<T>(p: *mut T) -> Result<(), u32> {
    if p.is_null() || !p.is_aligned() {
        Err(INVALID_ARGUMENT)
    } else {
        Ok(())
    }
}
fn task_id(id: u64) -> Result<TaskId, u32> {
    i64::try_from(id)
        .ok()
        .filter(|i| *i > 0)
        .map(TaskId)
        .ok_or(INVALID_ARGUMENT)
}
fn evidence_id(id: u64) -> Result<EvidenceId, u32> {
    task_id(id).map(|i| EvidenceId(i.0))
}
fn cursor(id: u64) -> Result<i64, u32> {
    i64::try_from(id).map_err(|_| INVALID_ARGUMENT)
}
unsafe fn input<'a>(v: AmbientText, limit: usize) -> Result<&'a str, u32> {
    if v.length as usize > limit {
        return Err(TOO_LONG);
    }
    if v.length == 0 || v.bytes.is_null() {
        return Err(INVALID_ARGUMENT);
    }
    let bytes = unsafe { std::slice::from_raw_parts(v.bytes, v.length as usize) };
    std::str::from_utf8(bytes).map_err(|_| INVALID_UTF8)
}
unsafe fn borrowed<'a, T>(p: *const T) -> Result<&'a T, u32> {
    if p.is_null() || !p.is_aligned() {
        return Err(INVALID_ARGUMENT);
    }
    Ok(unsafe { &*p })
}
unsafe fn versions(p: *const AmbientTaskVersion, count: u32) -> Result<Vec<TaskVersion>, u32> {
    if count as i64 > MAX_LIVE_TASKS {
        return Err(TOO_LONG);
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    if p.is_null() || !p.is_aligned() {
        return Err(INVALID_ARGUMENT);
    }
    let raw = unsafe { std::slice::from_raw_parts(p, count as usize) };
    raw.iter()
        .map(|v| {
            Ok(TaskVersion {
                task_id: task_id(v.task_id)?,
                revision: v.revision,
            })
        })
        .collect()
}
fn span(bytes: &mut Vec<u8>, text: &str) -> AmbientSpan {
    let out = AmbientSpan {
        offset: bytes.len() as u32,
        length: text.len() as u32,
    };
    bytes.extend_from_slice(text.as_bytes());
    out
}
fn encode_task(t: Task, bytes: &mut Vec<u8>) -> AmbientTask {
    AmbientTask {
        id: t.id.0 as u64,
        revision: t.revision,
        text: span(bytes, &t.text),
        lifecycle: t.lifecycle as u32,
        initiative_suppressed: u32::from(t.initiative_suppressed),
        plan_completeness: t.plan_completeness as u32,
    }
}
fn encode_evidence(e: Evidence, bytes: &mut Vec<u8>) -> AmbientEvidence {
    let mut out = AmbientEvidence {
        id: e.id.0 as u64,
        task_id: e.task_id.0 as u64,
        observed_at_utc: e.observed_at_utc.unwrap_or(0),
        has_observed_at: u32::from(e.observed_at_utc.is_some()),
        completeness: e.completeness as u32,
        kind: e.kind as u32,
        text: span(bytes, &e.text),
        ..Default::default()
    };
    if let Origin::External { key, version } = e.origin {
        out.origin = 1;
        out.namespace = span(bytes, &key.namespace);
        out.scope = span(bytes, &key.scope);
        out.object_id = span(bytes, &key.object_id);
        out.has_version = u32::from(version.is_some());
        if let Some(v) = version {
            out.version = span(bytes, &v);
        }
    }
    out
}
fn encode_source(s: SourceBinding, bytes: &mut Vec<u8>) -> AmbientSource {
    AmbientSource {
        id: s.id as u64,
        task_id: s.task_id.0 as u64,
        evidence_id: s.evidence_id.0 as u64,
        namespace: span(bytes, &s.key.namespace),
        scope: span(bytes, &s.key.scope),
        object_id: span(bytes, &s.key.object_id),
        has_version: u32::from(s.version.is_some()),
        version: s
            .version
            .map_or_else(AmbientSpan::default, |v| span(bytes, &v)),
    }
}
unsafe fn page_buffers<T>(
    page: *mut AmbientPage,
    records: *mut T,
    record_capacity: u32,
    bytes: *mut u8,
    byte_capacity: u32,
) -> Result<(), u32> {
    output(page)?;
    if record_capacity > 50 || byte_capacity as usize > MAX_PAGE_BYTES {
        return Err(TOO_LONG);
    }
    if (record_capacity > 0 && (records.is_null() || !records.is_aligned()))
        || (byte_capacity > 0 && bytes.is_null())
    {
        return Err(INVALID_ARGUMENT);
    }
    Ok(())
}
unsafe fn transfer<T, R: Copy>(
    p: Page<T>,
    page: *mut AmbientPage,
    records: *mut R,
    record_capacity: u32,
    bytes: *mut u8,
    byte_capacity: u32,
    encode: impl Fn(T, &mut Vec<u8>) -> R,
) -> Result<(), u32> {
    let mut payload = Vec::new();
    let rows: Vec<R> = p
        .items
        .into_iter()
        .map(|v| encode(v, &mut payload))
        .collect();
    if std::mem::size_of::<AmbientPage>() + rows.len() * std::mem::size_of::<R>() + payload.len()
        > MAX_PAGE_BYTES
    {
        return Err(RESOURCE_LIMIT);
    }
    let meta = AmbientPage {
        next_after: p.next_after.unwrap_or(0) as u64,
        count: rows.len() as u32,
        has_more: u32::from(p.next_after.is_some()),
        bytes_used: payload.len() as u32,
        task_revision: p.task_revision.unwrap_or(0),
        plan_completeness: p.plan_completeness.map_or(0, |v| v as u32),
    };
    unsafe { page.write(meta) };
    if rows.len() > record_capacity as usize || payload.len() > byte_capacity as usize {
        return Err(BUFFER_TOO_SMALL);
    }
    if !rows.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(rows.as_ptr(), records, rows.len()) };
    }
    if !payload.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), bytes, payload.len()) };
    }
    Ok(())
}
/// # Safety
/// Path is readable for its length; output is aligned, writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_open(path: *const u8, path_length: u32, out: *mut u64) -> u32 {
    boundary(|| {
        output(out)?;
        let p = unsafe {
            input(
                AmbientText {
                    bytes: path,
                    length: path_length,
                },
                16384,
            )
        }?;
        if p.contains('\0') || !Path::new(p).is_absolute() {
            return Err(INVALID_ARGUMENT);
        }
        let mut r = REGISTRY.lock().map_err(|_| PANIC)?;
        let next = r.next_handle.checked_add(1).ok_or(RESOURCE_LIMIT)?;
        let s = Store::open(p).map_err(store_error)?;
        let id = r.next_handle;
        r.stores.insert(
            id,
            Arc::new(StoreSlot {
                store: Mutex::new(s),
                source_cancel: Mutex::new(None),
                codex_cancel: Mutex::new(None),
                integration_cancel: Mutex::new(BTreeMap::new()),
                closed: AtomicBool::new(false),
            }),
        );
        r.next_handle = next;
        unsafe { out.write(id) };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_close(handle: u64) -> u32 {
    boundary(|| {
        let slot = REGISTRY
            .lock()
            .map_err(|_| PANIC)?
            .stores
            .remove(&handle)
            .ok_or(INVALID_HANDLE)?;
        let store = slot.store.lock().map_err(|_| PANIC)?;
        slot.closed.store(true, Ordering::Release);
        store.cancel_all_reads();
        store.cancel_check();
        // Model operations retain a lease until their accounting and actual cleanup finish.
        Ok(())
    })
}
/// # Safety
/// Output points to aligned writable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_manual_pause(handle: u64, out: *mut u32) -> u32 {
    boundary(|| {
        output(out)?;
        let p = with_store(handle, |s| s.manual_pause())?;
        unsafe { out.write(u32::from(p)) };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_set_manual_pause(handle: u64, paused: u32) -> u32 {
    boundary(|| {
        if paused > 1 {
            return Err(INVALID_ARGUMENT);
        }
        with_store(handle, |s| s.set_manual_pause(paused == 1))
    })
}
/// # Safety
/// Text is readable for its length; output is aligned, writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_create_task(
    handle: u64,
    text: AmbientText,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let t = unsafe { input(text, MAX_TEXT_BYTES) }?;
        let task = with_store(handle, |s| s.create_task(t))?;
        unsafe { out.write(task.id.0 as u64) };
        Ok(())
    })
}
/// # Safety
/// Text is readable for its declared byte length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_edit_task(
    handle: u64,
    id: u64,
    revision: u64,
    text: AmbientText,
) -> u32 {
    boundary(|| {
        let id = task_id(id)?;
        let t = unsafe { input(text, MAX_TEXT_BYTES) }?;
        with_store(handle, |s| s.edit_task(id, revision, t).map(|_| ()))
    })
}
/// # Safety
/// Version array is aligned and readable for count entries (null allowed for zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_delete_task(
    handle: u64,
    id: u64,
    revision: u64,
    others: *const AmbientTaskVersion,
    count: u32,
) -> u32 {
    boundary(|| {
        let id = task_id(id)?;
        let versions = unsafe { versions(others, count) }?;
        with_store(handle, |s| s.delete_task(id, revision, &versions))
    })
}
/// # Safety
/// Record and byte outputs are writable and disjoint. Null bytes only with zero capacity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_task(
    handle: u64,
    id: u64,
    out: *mut AmbientTask,
    bytes: *mut u8,
    capacity: u32,
) -> u32 {
    boundary(|| {
        output(out)?;
        if capacity as usize > MAX_TEXT_BYTES {
            return Err(TOO_LONG);
        }
        if capacity > 0 && bytes.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let id = task_id(id)?;
        let task = with_store(handle, |s| s.task(id))?;
        let mut payload = Vec::new();
        let record = encode_task(task, &mut payload);
        unsafe { out.write(record) };
        if payload.len() > capacity as usize {
            return Err(BUFFER_TOO_SMALL);
        }
        unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), bytes, payload.len()) };
        Ok(())
    })
}
macro_rules! page_function {
    ($name:ident,$record:ty,$method:ident,$encode:expr) => {
        /// # Safety
        /// Outputs are aligned, writable for their capacities, mutually disjoint and disjoint from input.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            handle: u64,
            after: u64,
            limit: u32,
            page: *mut AmbientPage,
            records: *mut $record,
            record_capacity: u32,
            bytes: *mut u8,
            byte_capacity: u32,
        ) -> u32 {
            boundary(|| {
                unsafe { page_buffers(page, records, record_capacity, bytes, byte_capacity) }?;
                let after = cursor(after)?;
                let p = with_store(handle, |s| s.$method(after, limit as usize))?;
                unsafe {
                    transfer(
                        p,
                        page,
                        records,
                        record_capacity,
                        bytes,
                        byte_capacity,
                        $encode,
                    )
                }
            })
        }
    };
}
page_function!(ambient_v3_tasks, AmbientTask, tasks, encode_task);
macro_rules! task_page_function {
    ($name:ident,$record:ty,$method:ident,$encode:expr) => {
        /// # Safety
        /// Outputs are aligned, writable for their capacities, mutually disjoint and disjoint from input.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            handle: u64,
            task: u64,
            after: u64,
            limit: u32,
            page: *mut AmbientPage,
            records: *mut $record,
            record_capacity: u32,
            bytes: *mut u8,
            byte_capacity: u32,
        ) -> u32 {
            boundary(|| {
                unsafe { page_buffers(page, records, record_capacity, bytes, byte_capacity) }?;
                let task = task_id(task)?;
                let after = cursor(after)?;
                let p = with_store(handle, |s| s.$method(task, after, limit as usize))?;
                unsafe {
                    transfer(
                        p,
                        page,
                        records,
                        record_capacity,
                        bytes,
                        byte_capacity,
                        $encode,
                    )
                }
            })
        }
    };
}
task_page_function!(
    ambient_v3_evidence,
    AmbientEvidence,
    evidence,
    encode_evidence
);
task_page_function!(ambient_v3_sources, AmbientSource, sources, encode_source);
task_page_function!(
    ambient_v3_links,
    AmbientLink,
    links,
    |l: TaskLink, bytes| AmbientLink {
        id: l.id as u64,
        other_task_id: l.other_task_id.0 as u64,
        action_at_utc: l.action_at_utc,
        related: u32::from(l.related),
        other_task_revision: l.other_task_revision,
        other_task_text: span(bytes, &l.other_task_text),
        other_task_lifecycle: l.other_task_lifecycle as u32,
        other_task_initiative_suppressed: u32::from(l.other_task_initiative_suppressed),
        other_task_plan_completeness: l.other_task_plan_completeness as u32
    }
);
task_page_function!(
    ambient_v3_decisions,
    AmbientDecision,
    decisions,
    |d: BindingDecision, _| AmbientDecision {
        id: d.id as u64,
        evidence_id: d.evidence_id.0 as u64,
        action_at_utc: d.action_at_utc,
        accepted: u32::from(d.accepted)
    }
);
/// # Safety
/// Outputs are aligned, writable for their capacities and mutually disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_deletion_impact(
    handle: u64,
    task: u64,
    evidence: u64,
    after: u64,
    limit: u32,
    page: *mut AmbientPage,
    records: *mut AmbientTask,
    record_capacity: u32,
    bytes: *mut u8,
    byte_capacity: u32,
) -> u32 {
    boundary(|| {
        unsafe { page_buffers(page, records, record_capacity, bytes, byte_capacity) }?;
        let task = task_id(task)?;
        let evidence = if evidence == 0 {
            None
        } else {
            Some(evidence_id(evidence)?)
        };
        let after = cursor(after)?;
        let p = with_store(handle, |s| {
            s.deletion_impact(task, evidence, after, limit as usize)
        })?;
        unsafe {
            transfer(
                p,
                page,
                records,
                record_capacity,
                bytes,
                byte_capacity,
                encode_task,
            )
        }
    })
}
/// # Safety
/// Text is readable; output is aligned/writable and disjoint from input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_add_note(
    handle: u64,
    task: u64,
    revision: u64,
    text: AmbientText,
    at: i64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = task_id(task)?;
        let t = unsafe { input(text, MAX_TEXT_BYTES) }?;
        let e = with_store(handle, |s| s.add_note(task, revision, t, at))?;
        unsafe { out.write(e.id.0 as u64) };
        Ok(())
    })
}
/// # Safety
/// Text is readable for its declared length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_edit_evidence(
    handle: u64,
    id: u64,
    task: u64,
    revision: u64,
    text: AmbientText,
    at: i64,
) -> u32 {
    boundary(|| {
        let id = evidence_id(id)?;
        let task = task_id(task)?;
        let t = unsafe { input(text, MAX_TEXT_BYTES) }?;
        with_store(handle, |s| s.edit_evidence(id, task, revision, t, at))
    })
}
/// # Safety
/// Versions are aligned/readable for count entries (null allowed for zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_delete_evidence(
    handle: u64,
    id: u64,
    task: u64,
    revision: u64,
    others: *const AmbientTaskVersion,
    count: u32,
) -> u32 {
    boundary(|| {
        let id = evidence_id(id)?;
        let task = task_id(task)?;
        let v = unsafe { versions(others, count) }?;
        with_store(handle, |s| s.delete_evidence(id, task, revision, &v))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_move_evidence(
    handle: u64,
    id: u64,
    from: u64,
    from_revision: u64,
    to: u64,
    to_revision: u64,
    at: i64,
) -> u32 {
    boundary(|| {
        let id = evidence_id(id)?;
        let from = task_id(from)?;
        let to = task_id(to)?;
        with_store(handle, |s| {
            s.move_evidence(id, from, from_revision, to, to_revision, at)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_set_link(
    handle: u64,
    a: u64,
    a_revision: u64,
    b: u64,
    b_revision: u64,
    related: u32,
    at: i64,
) -> u32 {
    boundary(|| {
        if related > 1 {
            return Err(INVALID_ARGUMENT);
        }
        let a = task_id(a)?;
        let b = task_id(b)?;
        with_store(handle, |s| {
            s.set_link(a, a_revision, b, b_revision, related == 1, at)
        })
    })
}
unsafe fn source_input(
    key: *const AmbientSourceKey,
    e: *const AmbientSourceEvidence,
) -> Result<(SourceKey, Option<String>, SourceEvidence), u32> {
    let k = unsafe { borrowed(key) }?;
    let e = unsafe { borrowed(e) }?;
    if k.has_version > 1 || e.has_observed_at > 1 {
        return Err(INVALID_ARGUMENT);
    }
    let key = SourceKey {
        namespace: unsafe { input(k.namespace, 64) }?.into(),
        scope: unsafe { input(k.scope, 256) }?.into(),
        object_id: unsafe { input(k.object_id, 2048) }?.into(),
    };
    let version = if k.has_version == 1 {
        Some(unsafe { input(k.version, 256) }?.into())
    } else {
        if k.version.length != 0 {
            return Err(INVALID_ARGUMENT);
        }
        None
    };
    let completeness = match e.completeness {
        0 => Completeness::Unknown,
        1 => Completeness::Partial,
        2 => Completeness::Complete,
        _ => return Err(INVALID_ARGUMENT),
    };
    let kind = match e.kind {
        0 => AssertionKind::SourceFact,
        1 => AssertionKind::Assumption,
        2 => AssertionKind::UserConfirmation,
        _ => return Err(INVALID_ARGUMENT),
    };
    let input = SourceEvidence {
        text: unsafe { input(e.text, MAX_TEXT_BYTES) }?.into(),
        observed_at_utc: if e.has_observed_at == 1 {
            Some(e.observed_at_utc)
        } else {
            None
        },
        completeness,
        kind,
    };
    Ok((key, version, input))
}
/// # Safety
/// Input records and their byte fields are aligned/readable; output is writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_ingest_source(
    handle: u64,
    task: u64,
    revision: u64,
    key: *const AmbientSourceKey,
    evidence: *const AmbientSourceEvidence,
    out: *mut AmbientIngest,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = task_id(task)?;
        let (key, version, input) = unsafe { source_input(key, evidence) }?;
        let result = with_store(handle, |s| {
            s.ingest_source(task, revision, &key, version.as_deref(), &input)
        })?;
        let record = match result {
            IngestOutcome::Applied(id) => AmbientIngest {
                evidence_id: id.0 as u64,
                outcome: 0,
            },
            IngestOutcome::Forgotten => AmbientIngest {
                evidence_id: 0,
                outcome: 1,
            },
            IngestOutcome::UserProtected => AmbientIngest {
                evidence_id: 0,
                outcome: 2,
            },
        };
        unsafe { out.write(record) };
        Ok(())
    })
}
/// # Safety
/// Input records/byte fields are readable; output is aligned/writable and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_bind_source(
    handle: u64,
    task: u64,
    revision: u64,
    key: *const AmbientSourceKey,
    evidence: *const AmbientSourceEvidence,
    at: i64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = task_id(task)?;
        let (key, version, input) = unsafe { source_input(key, evidence) }?;
        let id = with_store(handle, |s| {
            s.bind_source(task, revision, &key, version.as_deref(), &input, at)
        })?;
        unsafe { out.write(id.0 as u64) };
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn panic_is_contained() {
        assert_eq!(
            super::boundary(|| panic!("sanitized fixture")),
            super::PANIC
        );
    }
}

fn flag(v: u32) -> Result<bool, u32> {
    match v {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(INVALID_ARGUMENT),
    }
}
fn completeness(v: u32) -> Result<Completeness, u32> {
    match v {
        0 => Ok(Completeness::Unknown),
        1 => Ok(Completeness::Partial),
        2 => Ok(Completeness::Complete),
        _ => Err(INVALID_ARGUMENT),
    }
}
unsafe fn optional_input<'a>(
    has: u32,
    text: AmbientText,
    limit: usize,
) -> Result<Option<&'a str>, u32> {
    if flag(has)? {
        Ok(Some(unsafe { input(text, limit) }?))
    } else if text.length != 0 {
        Err(INVALID_ARGUMENT)
    } else {
        Ok(None)
    }
}
unsafe fn step_value(p: *const AmbientStepInput) -> Result<StepInput, u32> {
    let v = unsafe { borrowed(p) }?;
    Ok(StepInput {
        description: unsafe { input(v.description, MAX_TEXT_BYTES) }?.into(),
        prompt: unsafe { optional_input(v.has_prompt, v.prompt, MAX_TEXT_BYTES) }?
            .map(str::to_owned),
        reference: unsafe { optional_input(v.has_reference, v.reference, 2048) }?
            .map(str::to_owned),
    })
}
unsafe fn array<'a, T>(p: *const T, count: u32, max: u32) -> Result<&'a [T], u32> {
    if count > max {
        return Err(TOO_LONG);
    }
    if count == 0 {
        return Ok(&[]);
    }
    if p.is_null() || !p.is_aligned() {
        return Err(INVALID_ARGUMENT);
    }
    Ok(unsafe { std::slice::from_raw_parts(p, count as usize) })
}
unsafe fn kept_tasks(p: *const u64, count: u32) -> Result<Vec<TaskId>, u32> {
    unsafe { array(p, count, 1023) }?
        .iter()
        .map(|v| task_id(*v))
        .collect()
}
fn encode_step(s: PlanStep, b: &mut Vec<u8>) -> AmbientStep {
    AmbientStep {
        id: s.id.0 as u64,
        task_id: s.task_id.0 as u64,
        description: span(b, &s.description),
        has_prompt: u32::from(s.prompt.is_some()),
        prompt: s.prompt.map_or_else(AmbientSpan::default, |v| span(b, &v)),
        has_reference: u32::from(s.reference.is_some()),
        reference: s
            .reference
            .map_or_else(AmbientSpan::default, |v| span(b, &v)),
        has_completed: u32::from(s.completed_at_utc.is_some()),
        completed_at_utc: s.completed_at_utc.unwrap_or(0),
        ready: u32::from(s.ready),
    }
}
task_page_function!(ambient_v3_steps, AmbientStep, steps, encode_step);
task_page_function!(
    ambient_v3_dependencies,
    AmbientDependency,
    dependencies,
    |d: PlanDependency, _| AmbientDependency {
        id: d.id.0 as u64,
        task_id: d.task_id.0 as u64,
        predecessor: d.predecessor.0 as u64,
        dependent: d.dependent.0 as u64
    }
);
task_page_function!(
    ambient_v3_references,
    AmbientReference,
    references,
    |r: UsefulReference, b| AmbientReference {
        id: r.id.0 as u64,
        task_id: r.task_id.0 as u64,
        text: span(b, &r.text)
    }
);
/// # Safety
/// Caller-owned input and output buffers follow ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_checks(
    handle: u64,
    task: u64,
    step: u64,
    after: u64,
    limit: u32,
    page: *mut AmbientPage,
    records: *mut AmbientCheck,
    record_capacity: u32,
    bytes: *mut u8,
    byte_capacity: u32,
) -> u32 {
    boundary(|| {
        unsafe { page_buffers(page, records, record_capacity, bytes, byte_capacity) }?;
        let task = task_id(task)?;
        let step = StepId(task_id(step)?.0);
        let after = cursor(after)?;
        let p = with_store(handle, |s| s.checks(task, step, after, limit as usize))?;
        unsafe {
            transfer(
                p,
                page,
                records,
                record_capacity,
                bytes,
                byte_capacity,
                |k, b| AmbientCheck {
                    id: k.id.0 as u64,
                    task_id: k.task_id.0 as u64,
                    step_id: k.step_id.0 as u64,
                    description: span(b, &k.description),
                    result: k.result as u32,
                    has_confirmation: u32::from(k.confirmed_at_utc.is_some()),
                    confirmed_at_utc: k.confirmed_at_utc.unwrap_or(0),
                },
            )
        }
    })
}
/// # Safety
/// Caller-owned input and output buffers follow ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_add_step(
    handle: u64,
    task: u64,
    revision: u64,
    input: *const AmbientStepInput,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = task_id(task)?;
        let input = unsafe { step_value(input) }?;
        let id = with_store(handle, |s| s.add_step(task, revision, &input))?;
        unsafe { out.write(id.0 as u64) };
        Ok(())
    })
}
/// # Safety
/// Caller-owned input buffers follow ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_edit_step(
    handle: u64,
    task: u64,
    revision: u64,
    id: u64,
    input: *const AmbientStepInput,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let id = StepId(task_id(id)?.0);
        let input = unsafe { step_value(input) }?;
        with_store(handle, |s| s.edit_step(task, revision, id, &input))
    })
}
macro_rules! identity_mutation {
    ($name:ident,$id:ident,$method:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(handle: u64, task: u64, revision: u64, id: u64) -> u32 {
            boundary(|| {
                let task = task_id(task)?;
                let id = $id(task_id(id)?.0);
                with_store(handle, |s| s.$method(task, revision, id))
            })
        }
    };
}
identity_mutation!(ambient_v3_delete_step, StepId, delete_step);
identity_mutation!(ambient_v3_delete_check, CheckId, delete_check);
identity_mutation!(
    ambient_v3_delete_dependency,
    DependencyId,
    delete_dependency
);
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_set_plan_completeness(
    handle: u64,
    task: u64,
    revision: u64,
    value: u32,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let value = completeness(value)?;
        with_store(handle, |s| s.set_plan_completeness(task, revision, value))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_set_initiative_suppressed(
    handle: u64,
    task: u64,
    revision: u64,
    value: u32,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let value = flag(value)?;
        with_store(handle, |s| {
            s.set_initiative_suppressed(task, revision, value)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_reopen_task(handle: u64, task: u64, revision: u64) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        with_store(handle, |s| s.reopen_task(task, revision))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_set_step_completed(
    handle: u64,
    task: u64,
    revision: u64,
    id: u64,
    has_completed: u32,
    at: i64,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let id = StepId(task_id(id)?.0);
        let at = flag(has_completed)?.then_some(at);
        with_store(handle, |s| s.set_step_completed(task, revision, id, at))
    })
}
/// # Safety
/// Caller-owned text and output follow ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_add_check(
    handle: u64,
    task: u64,
    revision: u64,
    step: u64,
    description: AmbientText,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = task_id(task)?;
        let step = StepId(task_id(step)?.0);
        let text = unsafe { input(description, MAX_TEXT_BYTES) }?;
        let id = with_store(handle, |s| s.add_check(task, revision, step, text))?;
        unsafe { out.write(id.0 as u64) };
        Ok(())
    })
}
/// # Safety
/// Caller-owned text follows ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_edit_check(
    handle: u64,
    task: u64,
    revision: u64,
    id: u64,
    description: AmbientText,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let id = CheckId(task_id(id)?.0);
        let text = unsafe { input(description, MAX_TEXT_BYTES) }?;
        with_store(handle, |s| s.edit_check(task, revision, id, text))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_v3_set_check_result(
    handle: u64,
    task: u64,
    revision: u64,
    id: u64,
    result: u32,
    has_confirmation: u32,
    at: i64,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let id = CheckId(task_id(id)?.0);
        let result = match result {
            0 => CheckResult::Unknown,
            1 => CheckResult::Passed,
            2 => CheckResult::Failed,
            _ => return Err(INVALID_ARGUMENT),
        };
        let at = flag(has_confirmation)?.then_some(at);
        with_store(handle, |s| {
            s.set_check_result(task, revision, id, result, at)
        })
    })
}
/// # Safety
/// Output is aligned and writable; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_add_dependency(
    handle: u64,
    task: u64,
    revision: u64,
    predecessor: u64,
    dependent: u64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = task_id(task)?;
        let a = StepId(task_id(predecessor)?.0);
        let b = StepId(task_id(dependent)?.0);
        let id = with_store(handle, |s| s.add_dependency(task, revision, a, b))?;
        unsafe { out.write(id.0 as u64) };
        Ok(())
    })
}
/// # Safety
/// Caller-owned input and output buffers follow ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_completion_impact(
    handle: u64,
    task: u64,
    keep: *const u64,
    keep_count: u32,
    after: u64,
    limit: u32,
    page: *mut AmbientPage,
    records: *mut AmbientTask,
    record_capacity: u32,
    bytes: *mut u8,
    byte_capacity: u32,
) -> u32 {
    boundary(|| {
        unsafe { page_buffers(page, records, record_capacity, bytes, byte_capacity) }?;
        let task = task_id(task)?;
        let keep = unsafe { kept_tasks(keep, keep_count) }?;
        let after = cursor(after)?;
        let p = with_store(handle, |s| {
            s.completion_impact(task, &keep, after, limit as usize)
        })?;
        unsafe {
            transfer(
                p,
                page,
                records,
                record_capacity,
                bytes,
                byte_capacity,
                encode_task,
            )
        }
    })
}
/// # Safety
/// Caller-owned input arrays/text follow ambient.h; no pointers are retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_v3_complete_task(
    handle: u64,
    task: u64,
    revision: u64,
    summary: AmbientText,
    keep: *const u64,
    keep_count: u32,
    references: *const AmbientText,
    reference_count: u32,
    others: *const AmbientTaskVersion,
    other_count: u32,
) -> u32 {
    boundary(|| {
        let task = task_id(task)?;
        let summary = unsafe { input(summary, MAX_TEXT_BYTES) }?;
        let keep = unsafe { kept_tasks(keep, keep_count) }?;
        let versions = unsafe { versions(others, other_count) }?;
        let refs = unsafe { array(references, reference_count, 32) }?
            .iter()
            .map(|t| unsafe { input(*t, 2048) }.map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;
        with_store(handle, |s| {
            s.complete_task(task, revision, summary, &keep, &refs, &versions)
        })
    })
}
