//! Typed, bounded local-source boundary; native owns descriptors and actual I/O lifetime.
use crate::*;
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientSourceSettings {
    pub revision: u64,
    pub path: AmbientSpan,
    pub exclusion_count: u32,
    pub bytes_used: u32,
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AmbientSourceFile {
    pub path: AmbientText,
    pub bytes: *const u8,
    pub length: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientSourceStats {
    pub bytes: u32,
    pub files: u32,
    pub directories: u32,
    pub entries: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientSourceBranch {
    pub full_ref: AmbientSpan,
    pub oid: AmbientSpan,
    pub has_oid: u32,
    pub head: u32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientSourceObservation {
    pub started_at_utc: i64,
    pub ended_at_utc: i64,
    pub completeness: u32,
    pub reasons: u32,
    pub branch_count: u32,
    pub bytes_used: u32,
}
pub(super) fn error(e: StoreError) -> u32 {
    match e {
        StoreError::Paused => 2,
        StoreError::Cancelled => 3,
        StoreError::Busy => 4,
        StoreError::Storage | StoreError::UnsupportedSchema => 5,
        StoreError::ResourceLimit | StoreError::TextTooLong => 6,
        StoreError::ReportRequired => 9,
        StoreError::CapabilityRequired => 10,
        StoreError::StaleRevision => 11,
        StoreError::SessionRequired => 12,
        StoreError::TaskCompleted => 13,
        StoreError::NotFound => 14,
        StoreError::Deleted => 15,
        StoreError::InvalidInput => 1,
    }
}
pub(super) fn store<T>(
    h: u64,
    op: impl FnOnce(&mut Store) -> Result<T, StoreError>,
) -> Result<T, u32> {
    let slot = get_store(h)?;
    let mut s = slot.store.lock().map_err(|_| 8u32)?;
    if slot.closed.load(Ordering::Acquire) {
        return Err(7);
    }
    op(&mut s).map_err(error)
}
unsafe fn text<'a>(t: AmbientText, max: usize) -> Result<&'a str, u32> {
    unsafe { input(t, max) }.map_err(|_| 1)
}
fn out<T>(p: *mut T) -> Result<(), u32> {
    output(p).map_err(|_| 1)
}
unsafe fn buffers<T>(
    outp: *mut T,
    records: *mut AmbientSpan,
    count: u32,
    bytes: *mut u8,
    cap: u32,
) -> Result<(), u32> {
    out(outp)?;
    out(records)?;
    out(bytes)?;
    if count < 16 || cap < 16384 {
        return Err(6);
    }
    Ok(())
}
unsafe fn settings_output(
    r: LocalRootSettings,
    o: *mut AmbientSourceSettings,
    ex: *mut AmbientSpan,
    bytes: *mut u8,
) -> Result<(), u32> {
    let mut payload = Vec::new();
    let path = span(&mut payload, &r.path);
    let spans: Vec<_> = r.exclusions.iter().map(|s| span(&mut payload, s)).collect();
    let value = AmbientSourceSettings {
        revision: r.revision,
        path,
        exclusion_count: spans.len() as u32,
        bytes_used: payload.len() as u32,
    };
    unsafe {
        std::ptr::copy_nonoverlapping(payload.as_ptr(), bytes, payload.len());
        std::ptr::copy_nonoverlapping(spans.as_ptr(), ex, spans.len());
        o.write(value);
    }
    Ok(())
}
/// # Safety
/// All caller-owned pointers are live, aligned, and disjoint as documented in ambient_source.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_settings_get(
    h: u64,
    path: AmbientText,
    o: *mut AmbientSourceSettings,
    ex: *mut AmbientSpan,
    n: u32,
    b: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| {
        unsafe { buffers(o, ex, n, b, cap) }?;
        let p = unsafe { text(path, 4096) }?;
        let r = store(h, |s| s.local_root(p))?;
        unsafe { settings_output(r, o, ex, b) }
    })
}
/// # Safety
/// All caller-owned pointers are live, aligned, and disjoint as documented in ambient_source.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_settings_at(
    h: u64,
    index: u32,
    o: *mut AmbientSourceSettings,
    ex: *mut AmbientSpan,
    n: u32,
    b: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| {
        unsafe { buffers(o, ex, n, b, cap) }?;
        if index >= 64 {
            return Err(14);
        }
        let r = store(h, |s| {
            s.local_roots()?
                .get(index as usize)
                .cloned()
                .ok_or(StoreError::NotFound)
        })?;
        unsafe { settings_output(r, o, ex, b) }
    })
}
/// # Safety
/// All caller-owned pointers are live, aligned, and disjoint as documented in ambient_source.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_settings_save(
    h: u64,
    path: AmbientText,
    expected: u64,
    ex: *const AmbientText,
    n: u32,
    o: *mut u64,
) -> u32 {
    boundary(|| {
        out(o)?;
        let p = unsafe { text(path, 4096) }?;
        let a = unsafe { array(ex, n, 16) }.map_err(|_| 1u32)?;
        let exclusions = a
            .iter()
            .map(|t| unsafe { text(*t, 1024) }.map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;
        let r = store(h, |s| s.save_local_root(p, expected, &exclusions))?;
        unsafe { o.write(r.revision) };
        Ok(())
    })
}
/// # Safety
/// path is a live UTF-8 caller span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_settings_delete(
    h: u64,
    path: AmbientText,
    expected: u64,
) -> u32 {
    boundary(|| {
        let p = unsafe { text(path, 4096) }?;
        store(h, |s| s.delete_local_root(p, expected))
    })
}
/// # Safety
/// path and out are live and disjoint caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_begin(
    h: u64,
    task: u64,
    revision: u64,
    path: AmbientText,
    root_revision: u64,
    o: *mut u64,
) -> u32 {
    boundary(|| {
        out(o)?;
        let task = task_id(task)?;
        let p = unsafe { text(path, 4096) }?;
        let slot = get_store(h)?;
        let (id, flag) = store(h, |s| {
            let id = s.begin_local_read(
                TaskVersion {
                    task_id: task,
                    revision,
                },
                p,
                root_revision,
            )?;
            Ok((id, s.local_read_cancellation(id)?))
        })?;
        // Publish the same Core signal without holding either lock across the other owner.
        *slot.source_cancel.lock().map_err(|_| 8u32)? = Some((id, flag));
        if slot.closed.load(Ordering::Acquire) {
            return Err(7);
        }
        unsafe { o.write(id) };
        Ok(())
    })
}
/// # Safety
/// path and out are live and disjoint caller memory. Empty path denotes root metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_permit(
    h: u64,
    id: u64,
    path: AmbientText,
    directory: u32,
    o: *mut u32,
) -> u32 {
    boundary(|| {
        out(o)?;
        if directory > 1 {
            return Err(1);
        }
        let p = if path.length == 0 {
            ""
        } else {
            unsafe { text(path, 2048) }?
        };
        let yes = store(h, |s| s.permit_local_read(id, p, directory == 1))?;
        unsafe { o.write(u32::from(yes)) };
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_source_v1_cancel(h: u64, id: u64) -> u32 {
    boundary(|| {
        if id == 0 {
            return Err(1);
        }
        let slot = get_store(h)?;
        if slot.closed.load(Ordering::Acquire) {
            return Err(7);
        }
        // Cancellation cannot wait behind SQLite or parsing on the Store mutex.
        let flag = slot
            .source_cancel
            .lock()
            .map_err(|_| 8u32)?
            .as_ref()
            .filter(|(active, _)| *active == id)
            .map(|(_, flag)| flag.clone());
        if let Some(flag) = flag {
            flag.store(true, Ordering::Release);
        }
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_source_v1_abort(h: u64, id: u64) -> u32 {
    boundary(|| store(h, |s| s.abort_local_read(id)))
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_source_v1_release(h: u64, id: u64) -> u32 {
    boundary(|| {
        let slot = get_store(h)?;
        store(h, |s| s.release_local_read(id))?;
        let mut signal = slot.source_cancel.lock().map_err(|_| 8u32)?;
        if signal.as_ref().is_some_and(|(active, _)| *active == id) {
            *signal = None;
        }
        Ok(())
    })
}
fn reasons(mask: u32) -> Result<Vec<LocalReadReason>, u32> {
    if mask > 255 {
        return Err(1);
    }
    Ok([
        LocalReadReason::Excluded,
        LocalReadReason::Limit,
        LocalReadReason::Unavailable,
        LocalReadReason::InvalidName,
        LocalReadReason::Malformed,
        LocalReadReason::Unsupported,
        LocalReadReason::Changed,
        LocalReadReason::Symbolic,
    ]
    .into_iter()
    .filter(|r| mask & (1 << (*r as u32 - 1)) != 0)
    .collect())
}
/// # Safety
/// Inputs remain live and disjoint until return; native has already drained I/O and closed operation FDs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_finish(
    h: u64,
    id: u64,
    files: *const AmbientSourceFile,
    n: u32,
    stats: AmbientSourceStats,
    start: i64,
    end: i64,
    mask: u32,
) -> u32 {
    boundary(|| {
        let rs = reasons(mask)?;
        let inputs = unsafe { array(files, n, 256) }.map_err(|_| 1u32)?;
        let mut total = 0usize;
        let mut fs = Vec::new();
        for f in inputs {
            let p = unsafe { text(f.path, 2048) }?;
            let cap = match p {
                ".git/config" => 65536,
                ".git/packed-refs" => 524288,
                _ => 4096,
            };
            total = total.checked_add(f.length as usize).ok_or(6u32)?;
            if f.length as usize > cap || total > MAX_SOURCE_BYTES {
                return Err(6);
            }
            if f.length > 0 && f.bytes.is_null() {
                return Err(1);
            }
            let bytes = if f.length == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(f.bytes, f.length as usize) }.to_vec()
            };
            fs.push(LocalGitFile {
                path: p.into(),
                bytes,
            });
        }
        store(h, |s| {
            s.finish_local_read(
                id,
                &fs,
                LocalReadStats {
                    bytes: stats.bytes as usize,
                    files: stats.files as usize,
                    directories: stats.directories as usize,
                    entries: stats.entries as usize,
                },
                start,
                end,
                &rs,
            )
            .map(|_| ())
        })
    })
}
/// # Safety
/// Output capacity is >=256 records and >=1MiB bytes; pointers are live, aligned, disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_observation(
    h: u64,
    id: u64,
    o: *mut AmbientSourceObservation,
    records: *mut AmbientSourceBranch,
    n: u32,
    bytes: *mut u8,
    cap: u32,
) -> u32 {
    boundary(|| {
        out(o)?;
        out(records)?;
        out(bytes)?;
        if n < 256 || cap < 1048576 {
            return Err(6);
        }
        let r = store(h, |s| s.local_observation(id))?;
        let mut payload = Vec::new();
        let rows: Vec<_> = r
            .branches
            .iter()
            .map(|b| AmbientSourceBranch {
                full_ref: span(&mut payload, &b.full_ref),
                oid: b
                    .oid
                    .as_ref()
                    .map_or_else(AmbientSpan::default, |s| span(&mut payload, s)),
                has_oid: u32::from(b.oid.is_some()),
                head: b.head as u32,
            })
            .collect();
        let value = AmbientSourceObservation {
            started_at_utc: r.started_at_utc,
            ended_at_utc: r.ended_at_utc,
            completeness: r.completeness as u32,
            reasons: r.reasons.iter().fold(0, |m, r| m | (1 << (*r as u32 - 1))),
            branch_count: rows.len() as u32,
            bytes_used: payload.len() as u32,
        };
        unsafe {
            std::ptr::copy_nonoverlapping(payload.as_ptr(), bytes, payload.len());
            std::ptr::copy_nonoverlapping(rows.as_ptr(), records, rows.len());
            o.write(value);
        }
        Ok(())
    })
}
/// # Safety
/// bytes>=16384 and length are live, aligned, disjoint output buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_preview(
    h: u64,
    id: u64,
    index: u32,
    bytes: *mut u8,
    cap: u32,
    len: *mut u32,
) -> u32 {
    boundary(|| {
        out(bytes)?;
        out(len)?;
        if cap < 16384 {
            return Err(6);
        }
        let (_, _, e) = store(h, |s| s.local_branch_evidence(id, index as usize))?;
        unsafe {
            std::ptr::copy_nonoverlapping(e.text.as_ptr(), bytes, e.text.len());
            len.write(e.text.len() as u32)
        };
        Ok(())
    })
}
/// # Safety
/// out is live aligned writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_source_v1_bind(
    h: u64,
    id: u64,
    index: u32,
    at: i64,
    o: *mut u64,
) -> u32 {
    boundary(|| {
        out(o)?;
        let e = store(h, |s| s.bind_local_branch(id, index as usize, at))?;
        unsafe { o.write(e.0 as u64) };
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_cancel_does_not_wait_for_the_store_mutex() {
        let path = std::env::temp_dir().join(format!(
            "ambient-source-cancel-{}.sqlite3",
            std::process::id()
        ));
        let path_string = path.to_string_lossy();
        let mut memory = 0;
        assert_eq!(
            unsafe { ambient_v3_open(path_string.as_ptr(), path_string.len() as u32, &mut memory) },
            0
        );
        let (task, root) = store(memory, |s| {
            s.set_model_session(42, true, true)?;
            let admission = s.prepare_check(true, ModelProfile::High)?;
            let flag = admission.cancellation();
            let dispatch = s.dispatch_check(admission, 1)?;
            s.finish_check(
                dispatch.id(),
                ModelCheckResult {
                    outcome: CheckOutcome::Confirmed,
                    actual: ActualConfig {
                        model: ActualModel::Luna,
                        effort: ActualEffort::High,
                        tier: ActualTier::Fast,
                    },
                    usage: None,
                },
            )?;
            s.release_check(&flag);
            Ok((s.create_task("task")?, s.save_local_root("/root", 0, &[])?))
        })
        .unwrap_or_else(|_| panic!("fixture"));
        let mut operation = 0;
        let root_text = AmbientText {
            bytes: root.path.as_ptr(),
            length: root.path.len() as u32,
        };
        assert_eq!(
            unsafe {
                ambient_source_v1_begin(
                    memory,
                    task.id.0 as u64,
                    task.revision,
                    root_text,
                    root.revision,
                    &mut operation,
                )
            },
            0
        );
        let slot = get_store(memory).unwrap_or_else(|_| panic!("fixture"));
        let guard = slot.store.lock().unwrap_or_else(|_| panic!("fixture"));
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::thread::spawn(move || {
            let status = ambient_source_v1_cancel(memory, operation);
            let _ = tx.send(status);
        });
        let fast = rx.recv_timeout(std::time::Duration::from_millis(500));
        drop(guard);
        cancel.join().unwrap_or_else(|_| panic!("thread"));
        assert!(
            fast.is_ok(),
            "cancel waited for Store instead of setting the shared flag"
        );
        assert_eq!(ambient_source_v1_abort(memory, operation), 0);
        assert_eq!(ambient_source_v1_release(memory, operation), 0);
        assert_eq!(ambient_v3_close(memory), 0);
        let _ = std::fs::remove_file(path);
    }
}
