//! Concrete Codex source on the existing Store; borrowed raw data is bounded before access.
use crate::source::store;
use crate::*;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientCodexSettings {
    pub id: u64,
    pub revision: u64,
    pub path: AmbientSpan,
    pub bytes_used: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientCodexFile {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientCodexPlan {
    pub start: u64,
    pub end: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientCodexEvent {
    pub ordinal: u64,
    pub observed_at_utc: i64,
    pub kind: u32,
    pub completeness: u32,
    pub has_time: u32,
    pub reserved: u32,
    pub text: AmbientSpan,
    pub turn_id: AmbientSpan,
    pub item_id: AmbientSpan,
    pub object_id: AmbientSpan,
    pub version: AmbientSpan,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientCodexWindow {
    pub cursor: u64,
    pub next_before: u64,
    pub observed_at_utc: i64,
    pub reasons: u32,
    pub physical_lines: u32,
    pub count: u32,
    pub bytes_used: u32,
    pub thread_id: AmbientSpan,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AmbientCodexMetadata {
    pub binding_id: u64,
    pub evidence_id: u64,
    pub selection_id: u64,
    pub settings_revision: u64,
    pub accepted_ordinal: u64,
    pub kind: u32,
    pub reserved: u32,
    pub turn_id: AmbientSpan,
    pub item_id: AmbientSpan,
}
unsafe fn raw<'a>(p: *const u8, n: u32, max: usize) -> Result<&'a [u8], u32> {
    if n as usize > max {
        return Err(6);
    }
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() {
        return Err(1);
    }
    Ok(unsafe { std::slice::from_raw_parts(p, n as usize) })
}
fn capacities<T>(o: *mut T, b: *mut u8, cap: u32, min: u32) -> Result<(), u32> {
    output(o)?;
    output(b)?;
    if cap < min {
        return Err(6);
    }
    Ok(())
}
unsafe fn settings(
    h: u64,
    id: u64,
    index: Option<u32>,
    o: *mut AmbientCodexSettings,
    b: *mut u8,
    cap: u32,
) -> Result<(), u32> {
    capacities(o, b, cap, 4096)?;
    let s = store(h, |s| match index {
        None => s.codex_selection(id),
        Some(i) => s
            .codex_selections()?
            .get(i as usize)
            .cloned()
            .ok_or(StoreError::NotFound),
    })?;
    let mut bytes = Vec::new();
    let value = AmbientCodexSettings {
        id: s.id,
        revision: s.revision,
        path: span(&mut bytes, &s.path),
        bytes_used: s.path.len() as u32,
        reserved: 0,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), b, bytes.len());
        o.write(value);
    }
    Ok(())
}
/// # Safety
/// Caller buffers satisfy ambient_codex.h ownership, capacities and alignment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_settings_get(
    h: u64,
    id: u64,
    o: *mut AmbientCodexSettings,
    b: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| unsafe { settings(h, id, None, o, b, cap) })
}
/// # Safety
/// Caller buffers satisfy ambient_codex.h ownership, capacities and alignment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_settings_at(
    h: u64,
    index: u32,
    o: *mut AmbientCodexSettings,
    b: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| {
        if index >= 64 {
            return Err(14);
        }
        unsafe { settings(h, 0, Some(index), o, b, cap) }
    })
}
/// # Safety
/// Caller buffers satisfy ambient_codex.h ownership, capacities and alignment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_settings_save(
    h: u64,
    id: u64,
    revision: u64,
    path: AmbientText,
    out_id: *mut u64,
    out_revision: *mut u64,
) -> u32 {
    boundary(|| {
        output(out_id)?;
        output(out_revision)?;
        let path = unsafe { input(path, 4096) }.map_err(|_| 1u32)?;
        let s = store(h, |s| s.save_codex_selection(id, revision, path))?;
        unsafe {
            out_id.write(s.id);
            out_revision.write(s.revision);
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_codex_v1_settings_delete(h: u64, id: u64, revision: u64) -> u32 {
    boundary(|| store(h, |s| s.delete_codex_selection(id, revision)))
}
/// # Safety
/// out is live, aligned and disjoint; input IDs are scalar values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_begin(
    h: u64,
    task: u64,
    revision: u64,
    selection: u64,
    settings_revision: u64,
    cursor: u64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let task = TaskVersion {
            task_id: task_id(task)?,
            revision,
        };
        let slot = get_store(h)?;
        // Publication of the accepted atomic shares the short cancellation lock; no I/O holds it.
        let mut signal = slot.codex_cancel.lock().map_err(|_| 8u32)?;
        let id = store(h, |s| {
            s.begin_codex_read(task, selection, settings_revision, cursor)
        })?;
        let flag = store(h, |s| s.codex_read_cancellation(id))?;
        *signal = Some((id, flag));
        unsafe {
            out.write(id);
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_codex_v1_permit(h: u64, id: u64) -> u32 {
    boundary(|| store(h, |s| s.permit_codex_read(id).map(|_| ())))
}
/// # Safety
/// Header is bounded before access; out is live/aligned/disjoint as documented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_plan(
    h: u64,
    id: u64,
    file: AmbientCodexFile,
    p: *const u8,
    n: u32,
    out: *mut AmbientCodexPlan,
) -> u32 {
    boundary(|| {
        output(out)?;
        let bytes = unsafe { raw(p, n, MAX_CODEX_LINE + 1) }?;
        let plan = store(h, |s| {
            s.plan_codex_read(
                id,
                CodexFileIdentity {
                    device: file.device,
                    inode: file.inode,
                    size: file.size,
                },
                bytes,
            )
        })
        .map_err(|e| if e == 1 { 16 } else { e })?;
        unsafe {
            out.write(AmbientCodexPlan {
                start: plan.start,
                end: plan.end,
            });
        }
        Ok(())
    })
}
/// # Safety
/// Native closed all operation FDs; input bytes remain live until return and are bounded first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_finish(
    h: u64,
    id: u64,
    p: *const u8,
    n: u32,
    at: i64,
) -> u32 {
    boundary(|| {
        let bytes = unsafe { raw(p, n, MAX_CODEX_WINDOW + 1) }?;
        store(h, |s| s.finish_codex_read(id, bytes, at).map(|_| ()))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_codex_v1_cancel(h: u64, id: u64) -> u32 {
    boundary(|| {
        let slot = get_store(h)?;
        if slot.closed.load(Ordering::Acquire) {
            return Err(7);
        }
        if let Some((_, flag)) = slot
            .codex_cancel
            .lock()
            .map_err(|_| 8u32)?
            .as_ref()
            .filter(|(active, _)| *active == id)
        {
            flag.store(true, Ordering::Release);
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_codex_v1_abort(h: u64, id: u64) -> u32 {
    boundary(|| store(h, |s| s.abort_codex_read(id)))
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_codex_v1_release(h: u64, id: u64) -> u32 {
    boundary(|| {
        let slot = get_store(h)?;
        store(h, |s| s.release_codex_read(id))?;
        let mut signal = slot.codex_cancel.lock().map_err(|_| 8u32)?;
        if signal.as_ref().is_some_and(|(active, _)| *active == id) {
            *signal = None;
        }
        Ok(())
    })
}
/// # Safety
/// out/records/bytes satisfy documented fixed window capacities and pointer ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_window(
    h: u64,
    id: u64,
    out: *mut AmbientCodexWindow,
    records: *mut AmbientCodexEvent,
    n: u32,
    b: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| {
        capacities(out, b, cap, 262144)?;
        output(records)?;
        if n < 128 {
            return Err(6);
        }
        let o = store(h, |s| s.codex_observation(id))?;
        let mut bytes = Vec::new();
        let thread_id = span(&mut bytes, &o.thread_id);
        let rows: Vec<_> = o
            .window
            .events
            .iter()
            .map(|e| AmbientCodexEvent {
                ordinal: e.ordinal,
                observed_at_utc: e.evidence.observed_at_utc.unwrap_or(0),
                kind: e.identity.kind() as u32,
                completeness: e.evidence.completeness as u32,
                has_time: u32::from(e.evidence.observed_at_utc.is_some()),
                reserved: 0,
                text: span(&mut bytes, &e.evidence.text),
                turn_id: span(&mut bytes, e.identity.turn_id()),
                item_id: e
                    .identity
                    .item_id()
                    .map_or_else(AmbientSpan::default, |s| span(&mut bytes, s)),
                object_id: span(&mut bytes, &e.key.object_id),
                version: span(&mut bytes, &e.version),
            })
            .collect();
        if bytes.len()
            + rows.len() * std::mem::size_of::<AmbientCodexEvent>()
            + std::mem::size_of::<AmbientCodexWindow>()
            > MAX_PAGE_BYTES
        {
            return Err(6);
        }
        let value = AmbientCodexWindow {
            cursor: o.cursor,
            next_before: o.window.next_before,
            observed_at_utc: o.observed_at_utc,
            reasons: o.window.reasons,
            physical_lines: o.window.physical_lines,
            count: rows.len() as u32,
            bytes_used: bytes.len() as u32,
            thread_id,
        };
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), b, bytes.len());
            std::ptr::copy_nonoverlapping(rows.as_ptr(), records, rows.len());
            out.write(value);
        }
        Ok(())
    })
}
/// # Safety
/// bytes has capacity>=16384 and length is live/aligned/disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_preview(
    h: u64,
    id: u64,
    index: u32,
    b: *mut u8,
    cap: u32,
    length: *mut u32,
) -> u32 {
    boundary(|| {
        capacities(length, b, cap, 16384)?;
        let e = store(h, |s| s.codex_event(id, index as usize))?;
        if e.variable_bytes() > 16384 || e.evidence.text.is_empty() {
            return Err(6);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(e.evidence.text.as_ptr(), b, e.evidence.text.len());
            length.write(e.evidence.text.len() as u32);
        }
        Ok(())
    })
}
/// # Safety
/// out is live/aligned/disjoint; only a Core-owned window index is accepted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_bind(
    h: u64,
    id: u64,
    index: u32,
    at: i64,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        output(out)?;
        let e = store(h, |s| s.bind_codex_event(id, index as usize, at))
            .map_err(|e| if e == 1 { 17 } else { e })?;
        unsafe {
            out.write(e.0 as u64);
        }
        Ok(())
    })
}
/// # Safety
/// Standard memory3 page buffers/ownership apply; capacities<=50 records/256KiB.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_codex_v1_saved(
    h: u64,
    task: u64,
    after: u64,
    page: *mut AmbientPage,
    records: *mut AmbientCodexMetadata,
    n: u32,
    b: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| {
        unsafe { page_buffers(page, records, n, b, cap) }?;
        let task = task_id(task)?;
        let after = i64::try_from(after).map_err(|_| 1u32)?;
        let p = store(h, |s| s.codex_metadata(task, after, n as usize))?;
        unsafe {
            transfer(p, page, records, n, b, cap, |m, bytes| {
                AmbientCodexMetadata {
                    binding_id: m.binding_id as u64,
                    evidence_id: m.evidence_id.0 as u64,
                    selection_id: m.selection_id,
                    settings_revision: m.settings_revision,
                    accepted_ordinal: m.accepted_ordinal,
                    kind: m.identity.kind() as u32,
                    reserved: 0,
                    turn_id: span(bytes, m.identity.turn_id()),
                    item_id: m
                        .identity
                        .item_id()
                        .map_or_else(AmbientSpan::default, |s| span(bytes, s)),
                }
            })
        }
    })
}
