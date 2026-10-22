//! Background observation ABI v1 (ADR-025 «ABI и хранение»). Frames and outputs are typed, the
//! policy is JSON; `begin` hands out a model ABI v6 operation for dispatch, cancel and close.
use crate::model::{INVALID, LIMIT, PANIC, STORAGE, boundary, error, memory, register, reserve_id};
use crate::{AmbientSpan, AmbientText};
use ambient_core::*;
use std::sync::atomic::Ordering;

/// All text of one frame; the core cuts it to its own bounds after normalization.
const MAX_STRINGS: usize = 8 * 1024 * 1024;
const MAX_LINES: u32 = 4096;
const MAX_WORDS: u32 = 16_384;
const MAX_POLICY: usize = 1024;

/// Vision rectangle normalized to the display, origin bottom-left.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientObservedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientObservedWord {
    pub text: AmbientSpan,
    pub rect: AmbientObservedRect,
}
/// A line owns the next `word_count` words of the frame's word array, in order.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientObservedLine {
    pub text: AmbientSpan,
    pub word_count: u32,
}
/// Every span indexes `strings`; empty spans are allowed.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AmbientObservedFrame {
    pub strings: AmbientText,
    pub display_key: AmbientSpan,
    pub bundle_id: AmbientSpan,
    pub app_name: AmbientSpan,
    pub title: AmbientSpan,
    pub lines: *const AmbientObservedLine,
    pub words: *const AmbientObservedWord,
    pub jpeg: AmbientText,
    pub line_count: u32,
    pub word_count: u32,
    pub idle_seconds: u32,
    pub reserved: u32,
}
/// capture 1: act after `seconds`; 2: hold (do not capture, keep calling tick).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct AmbientObserveNext {
    pub capture: u32,
    pub seconds: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct AmbientObserveFrameResult {
    pub frame_id: u64,
    pub highlight_version: u64,
    pub disposition: u32,
    pub next: AmbientObserveNext,
    pub reserved: u32,
}
/// block 0: `operation` is ready; 1 not due; 2 busy; 3 cloud unavailable (operation 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct AmbientObserveBegin {
    pub operation: u64,
    pub block: u32,
    pub reserved: u32,
}
// The layouts of ambient_observe.h.
const _: () = {
    use std::mem::{align_of, offset_of, size_of};
    assert!(size_of::<AmbientObservedRect>() == 32 && align_of::<AmbientObservedRect>() == 8);
    assert!(size_of::<AmbientObservedWord>() == 40 && align_of::<AmbientObservedWord>() == 8);
    assert!(size_of::<AmbientObservedLine>() == 12 && align_of::<AmbientObservedLine>() == 4);
    assert!(size_of::<AmbientObservedFrame>() == 96 && align_of::<AmbientObservedFrame>() == 8);
    assert!(offset_of!(AmbientObservedFrame, lines) == 48);
    assert!(offset_of!(AmbientObservedFrame, jpeg) == 64);
    assert!(offset_of!(AmbientObservedFrame, line_count) == 80);
    assert!(size_of::<AmbientObserveNext>() == 8);
    assert!(size_of::<AmbientObserveFrameResult>() == 32);
    assert!(offset_of!(AmbientObserveFrameResult, next) == 20);
    assert!(size_of::<AmbientObserveBegin>() == 16);
};

fn time(utc: i64, monotonic_ms: u64) -> ObservationTime {
    ObservationTime { utc, monotonic_ms }
}
fn next(c: Capture) -> AmbientObserveNext {
    match c {
        Capture::After { seconds } => AmbientObserveNext {
            capture: 1,
            seconds,
        },
        Capture::Hold => AmbientObserveNext {
            capture: 2,
            seconds: 0,
        },
    }
}
/// Caller bytes: null only at length 0.
unsafe fn bytes<'a>(t: AmbientText, max: usize) -> Result<&'a [u8], u32> {
    if t.length as usize > max || (t.length > 0 && t.bytes.is_null()) {
        return Err(INVALID);
    }
    if t.length == 0 {
        return Ok(&[]);
    }
    Ok(unsafe { std::slice::from_raw_parts(t.bytes, t.length as usize) })
}
/// Copies a frame out of caller memory. Counts and spans are checked before any allocation that
/// they size; the words must split into the lines exactly, so no word is shared or left over.
/// Content limits (key, JPEG, rects, text cuts) stay with the core.
unsafe fn frame(f: &AmbientObservedFrame) -> Result<ObservedFrame, u32> {
    if f.reserved != 0 {
        return Err(INVALID);
    }
    let strings = unsafe { bytes(f.strings, MAX_STRINGS) }?;
    let text = |s: AmbientSpan| -> Result<String, u32> {
        let end = s.offset.checked_add(s.length).ok_or(INVALID)?;
        let part = strings
            .get(s.offset as usize..end as usize)
            .ok_or(INVALID)?;
        std::str::from_utf8(part)
            .map(str::to_owned)
            .map_err(|_| INVALID)
    };
    let lines = unsafe { crate::array(f.lines, f.line_count, MAX_LINES) }.map_err(|_| INVALID)?;
    let mut words =
        unsafe { crate::array(f.words, f.word_count, MAX_WORDS) }.map_err(|_| INVALID)?;
    let lines = lines
        .iter()
        .map(|l| {
            let (own, rest) = words
                .split_at_checked(l.word_count as usize)
                .ok_or(INVALID)?;
            words = rest;
            Ok(ObservedLine {
                text: text(l.text)?,
                words: own
                    .iter()
                    .map(|w| {
                        Ok(ObservedWord {
                            text: text(w.text)?,
                            rect: NormalizedRect {
                                x: w.rect.x,
                                y: w.rect.y,
                                width: w.rect.width,
                                height: w.rect.height,
                            },
                        })
                    })
                    .collect::<Result<_, u32>>()?,
            })
        })
        .collect::<Result<Vec<_>, u32>>()?;
    if !words.is_empty() {
        return Err(INVALID);
    }
    Ok(ObservedFrame {
        display_key: text(f.display_key)?,
        bundle_id: text(f.bundle_id)?,
        app_name: text(f.app_name)?,
        title: text(f.title)?,
        idle_seconds: f.idle_seconds,
        lines,
        image: unsafe { bytes(f.jpeg, MAX_FRAME_IMAGE) }?.to_vec(),
    })
}
/// Writes Core JSON; a small capacity fails with LIMIT and reports the needed length only.
unsafe fn json(value: &str, out: *mut u8, capacity: u32, length: *mut u32) -> Result<(), u32> {
    crate::output(length).map_err(|_| INVALID)?;
    if capacity > 0 && out.is_null() {
        return Err(INVALID);
    }
    let needed = u32::try_from(value.len()).map_err(|_| LIMIT)?;
    if needed > capacity {
        unsafe { length.write(needed) };
        return Err(LIMIT);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), out, value.len());
        length.write(needed);
    }
    Ok(())
}

/// # Safety
/// policy is readable caller memory for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_observe_v1_set_policy(handle: u64, policy: AmbientText) -> u32 {
    boundary(|| {
        let text = unsafe { crate::input(policy, MAX_POLICY) }.map_err(|_| INVALID)?;
        memory(handle, |s| {
            s.set_observation_policy_json(text).map_err(error)
        })
    })
}
/// # Safety
/// frame and everything it points to are readable for the call; out is aligned and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_observe_v1_submit_frame(
    handle: u64,
    at_utc: i64,
    monotonic_ms: u64,
    frame: *const AmbientObservedFrame,
    out: *mut AmbientObserveFrameResult,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let f = unsafe { crate::borrowed(frame) }.map_err(|_| INVALID)?;
        let f = unsafe { self::frame(f) }?;
        let r = memory(handle, |s| {
            s.submit_frame(f, time(at_utc, monotonic_ms)).map_err(error)
        })?;
        unsafe {
            out.write(AmbientObserveFrameResult {
                frame_id: r.frame.map_or(0, FrameId::get),
                highlight_version: r.highlight_version,
                disposition: r.disposition as u32,
                next: next(r.next),
                reserved: 0,
            })
        };
        Ok(())
    })
}
/// # Safety
/// out is aligned writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_observe_v1_tick(
    handle: u64,
    at_utc: i64,
    monotonic_ms: u64,
    idle_seconds: u32,
    app_switched: u32,
    out: *mut AmbientObserveNext,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let switched = crate::flag(app_switched)?;
        let c = memory(handle, |s| {
            s.observation_tick(idle_seconds, switched, time(at_utc, monotonic_ms))
                .map_err(error)
        })?;
        unsafe { out.write(next(c)) };
        Ok(())
    })
}
/// # Safety
/// out is aligned writable caller memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_observe_v1_begin(
    handle: u64,
    at_utc: i64,
    monotonic_ms: u64,
    out: *mut AmbientObserveBegin,
) -> u32 {
    boundary(|| {
        crate::output(out).map_err(|_| INVALID)?;
        let store = crate::get_store(handle).map_err(|_| crate::model::HANDLE)?;
        let now = time(at_utc, monotonic_ms);
        let id = reserve_id()?;
        let begun = {
            let mut s = store.store.lock().map_err(|_| PANIC)?;
            if store.closed.load(Ordering::Acquire) {
                return Err(crate::model::HANDLE);
            }
            s.begin_observation(now).map_err(error)?
        };
        let result = match begun {
            ObservationBegin::Ready(admission) => {
                register(id, store, admission, at_utc, Some(monotonic_ms))?;
                AmbientObserveBegin {
                    operation: id,
                    block: 0,
                    reserved: 0,
                }
            }
            ObservationBegin::Blocked(block) => AmbientObserveBegin {
                operation: 0,
                block: match block {
                    ObservationBlock::NotStarted
                    | ObservationBlock::Paused
                    | ObservationBlock::NotDue
                    | ObservationBlock::HourlyLimit => 1,
                    ObservationBlock::Busy => 2,
                    ObservationBlock::CloudUnavailable => 3,
                },
                reserved: 0,
            },
        };
        unsafe { out.write(result) };
        Ok(())
    })
}
/// # Safety
/// bytes has capacity writable bytes (null only with capacity 0); length is aligned writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_observe_v1_highlight(
    handle: u64,
    bytes: *mut u8,
    capacity: u32,
    length: *mut u32,
) -> u32 {
    boundary(|| {
        let value = memory(handle, |s| {
            serde_json::to_string(&s.highlight()).map_err(|_| STORAGE)
        })?;
        unsafe { json(&value, bytes, capacity, length) }
    })
}
/// # Safety
/// bytes has capacity writable bytes (null only with capacity 0); length is aligned writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_observe_v1_journal(
    handle: u64,
    at_utc: i64,
    bytes: *mut u8,
    capacity: u32,
    length: *mut u32,
) -> u32 {
    boundary(|| {
        let value = memory(handle, |s| {
            let journal = s.observation_journal(at_utc).map_err(error)?;
            serde_json::to_string(&journal).map_err(|_| STORAGE)
        })?;
        unsafe { json(&value, bytes, capacity, length) }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_observe_v1_delete_log(handle: u64) -> u32 {
    boundary(|| memory(handle, |s| s.delete_observation_log().map_err(error)))
}

#[cfg(test)]
#[path = "observe_tests.rs"]
mod tests;
