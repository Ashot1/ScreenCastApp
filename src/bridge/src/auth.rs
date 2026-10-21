use ambient_providers::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct AmbientAuthSpan {
    pub offset: u32,
    pub length: u32,
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AmbientAuthRecord {
    pub registration: u32,
    pub has_hint: u32,
    pub has_credentials: u32,
    pub scope_count: u32,
    pub expires_at: i64,
    pub bytes_used: u32,
    pub reserved: u32,
    pub host: AmbientAuthSpan,
    pub client_id: AmbientAuthSpan,
    pub issuer: AmbientAuthSpan,
    pub subject: AmbientAuthSpan,
    pub hint: AmbientAuthSpan,
    pub nonce: AmbientAuthSpan,
    pub access: AmbientAuthSpan,
    pub refresh: AmbientAuthSpan,
    pub url: AmbientAuthSpan,
    pub scopes: [AmbientAuthSpan; 64],
}
impl Default for AmbientAuthRecord {
    fn default() -> Self {
        Self {
            registration: 0,
            has_hint: 0,
            has_credentials: 0,
            scope_count: 0,
            expires_at: 0,
            bytes_used: 0,
            reserved: 0,
            host: AmbientAuthSpan::default(),
            client_id: AmbientAuthSpan::default(),
            issuer: AmbientAuthSpan::default(),
            subject: AmbientAuthSpan::default(),
            hint: AmbientAuthSpan::default(),
            nonce: AmbientAuthSpan::default(),
            access: AmbientAuthSpan::default(),
            refresh: AmbientAuthSpan::default(),
            url: AmbientAuthSpan::default(),
            scopes: [AmbientAuthSpan::default(); 64],
        }
    }
}
struct Slot {
    cancelled: Arc<AtomicBool>,
    busy: AtomicBool,
    closed: AtomicBool,
    op: Mutex<AuthOperation>,
}
static AUTH: Mutex<(u64, BTreeMap<u64, Arc<Slot>>)> = Mutex::new((1, BTreeMap::new()));
const BUFFER: u32 = 11;
const HANDLE: u32 = 12;
const BUSY: u32 = 13;
fn boundary(op: impl FnOnce() -> Result<(), u32>) -> u32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(op)) {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => e,
        Err(_) => 10,
    }
}
fn get(handle: u64) -> Result<Arc<Slot>, u32> {
    AUTH.lock()
        .map_err(|_| 10u32)?
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
fn with_op<T>(
    handle: u64,
    call: impl FnOnce(&mut AuthOperation) -> Result<T, AuthError>,
) -> Result<T, u32> {
    let slot = get(handle)?;
    slot.busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| BUSY)?;
    let running = Running(slot);
    if running.0.closed.load(Ordering::Acquire) {
        return Err(HANDLE);
    }
    let mut op = running.0.op.lock().map_err(|_| 10u32)?;
    call(&mut op).map_err(|e| e as u32)
}
unsafe fn text<'a>(p: *const u8, n: u32, limit: usize) -> Result<&'a str, u32> {
    if n as usize > limit || (n > 0 && p.is_null()) {
        return Err(1);
    }
    if n == 0 {
        return Ok("");
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(p, n as usize) }).map_err(|_| 1)
}
fn read_span(s: AmbientAuthSpan, bytes: &str, limit: usize) -> Result<&str, u32> {
    if s.length as usize > limit || (s.length == 0 && s.offset != 0) {
        return Err(1);
    }
    let end = (s.offset as usize)
        .checked_add(s.length as usize)
        .ok_or(1u32)?;
    bytes.get(s.offset as usize..end).ok_or(1)
}
fn decode(r: &AmbientAuthRecord, b: &str) -> Result<Snapshot, u32> {
    if r.reserved != 0
        || r.has_hint > 1
        || r.has_credentials > 1
        || r.scope_count > 64
        || r.bytes_used as usize != b.len()
        || r.url.length != 0
        || r.url.offset != 0
    {
        return Err(1);
    }
    let string = |s, l| read_span(s, b, l).map(str::to_owned);
    let absent = |s: AmbientAuthSpan| s.length == 0 && s.offset == 0;
    if (r.registration < 2 && !absent(r.client_id))
        || (r.registration != 3 && (!absent(r.issuer) || !absent(r.subject)))
        || (r.has_hint == 0 && (!absent(r.hint) || !absent(r.nonce)))
        || (r.has_credentials == 0
            && (!absent(r.access) || !absent(r.refresh) || r.scope_count != 0 || r.expires_at != 0))
        || r.scopes[r.scope_count as usize..]
            .iter()
            .any(|s| !absent(*s))
    {
        return Err(1);
    }
    let registration = Registration {
        host: string(r.host, 45)?,
        state: r.registration,
        client_id: if r.registration >= 2 {
            Some(string(r.client_id, 512)?)
        } else {
            None
        },
        identity: if r.registration == 3 {
            Some(Identity {
                issuer: string(r.issuer, 2048)?,
                subject: string(r.subject, 1024)?,
            })
        } else {
            None
        },
        hint: if r.has_hint == 1 {
            Some(Hint {
                token: string(r.hint, TOKEN_LIMIT)?,
                nonce: string(r.nonce, 1024)?,
            })
        } else {
            None
        },
    };
    let credentials = if r.has_credentials == 1 {
        Some(Credentials {
            access: string(r.access, TOKEN_LIMIT)?,
            refresh: string(r.refresh, TOKEN_LIMIT)?,
            scopes: r.scopes[..r.scope_count as usize]
                .iter()
                .map(|s| string(*s, SCOPE_LIMIT))
                .collect::<Result<_, _>>()?,
            expires_at: r.expires_at,
        })
    } else {
        None
    };
    let s = Snapshot {
        registration,
        credentials,
    };
    s.validate().map_err(|e| e as u32)?;
    Ok(s)
}
fn span(b: &mut Vec<u8>, s: &str) -> AmbientAuthSpan {
    let out = AmbientAuthSpan {
        offset: b.len() as u32,
        length: s.len() as u32,
    };
    b.extend_from_slice(s.as_bytes());
    out
}
fn encode(s: Snapshot) -> (AmbientAuthRecord, Vec<u8>) {
    let mut r = AmbientAuthRecord::default();
    let mut b = Vec::new();
    let sreg = s.registration;
    r.registration = sreg.state;
    r.host = span(&mut b, &sreg.host);
    if let Some(id) = sreg.client_id {
        r.client_id = span(&mut b, &id)
    }
    if let Some(i) = sreg.identity {
        r.issuer = span(&mut b, &i.issuer);
        r.subject = span(&mut b, &i.subject)
    }
    if let Some(h) = sreg.hint {
        r.has_hint = 1;
        r.hint = span(&mut b, &h.token);
        r.nonce = span(&mut b, &h.nonce)
    }
    if let Some(c) = s.credentials {
        r.has_credentials = 1;
        r.access = span(&mut b, &c.access);
        r.refresh = span(&mut b, &c.refresh);
        r.expires_at = c.expires_at;
        r.scope_count = c.scopes.len() as u32;
        for (i, s) in c.scopes.iter().enumerate() {
            r.scopes[i] = span(&mut b, s)
        }
    }
    r.bytes_used = b.len() as u32;
    (r, b)
}
fn capacity(r: *mut AmbientAuthRecord, b: *mut u8, n: u32) -> Result<(), u32> {
    if r.is_null() || !r.is_aligned() || b.is_null() {
        Err(1)
    } else if (n as usize) < RESULT_LIMIT {
        Err(BUFFER)
    } else {
        Ok(())
    }
}
unsafe fn write(
    r: *mut AmbientAuthRecord,
    b: *mut u8,
    out: (AmbientAuthRecord, Vec<u8>),
) -> Result<(), u32> {
    if out.1.len() > RESULT_LIMIT {
        return Err(9);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(out.1.as_ptr(), b, out.1.len());
        r.write(out.0)
    };
    Ok(())
}
/// # Safety
/// Caller provides live, aligned, non-overlapping inputs and output capacities
/// for the entire call, as specified in `include/ambient_auth.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_auth_v1_begin(
    refresh: u32,
    r: *const AmbientAuthRecord,
    b: *const u8,
    n: u32,
    redirect: *const u8,
    redirect_length: u32,
    out: *mut u64,
) -> u32 {
    boundary(|| {
        if refresh > 1 || r.is_null() || !r.is_aligned() || out.is_null() || !out.is_aligned() {
            return Err(1);
        }
        let s = decode(unsafe { &*r }, unsafe { text(b, n, RESULT_LIMIT)? })?;
        let redirect = unsafe { text(redirect, redirect_length, 2048)? };
        let cancelled = Arc::new(AtomicBool::new(false));
        let op = AuthOperation::new(
            s,
            if redirect.is_empty() {
                None
            } else {
                Some(redirect.into())
            },
            refresh == 1,
            cancelled.clone(),
        )
        .map_err(|e| e as u32)?;
        let mut reg = AUTH.lock().map_err(|_| 10u32)?;
        if !reg.1.is_empty() {
            return Err(BUSY);
        }
        let id = reg.0;
        reg.0 = reg.0.checked_add(1).ok_or(9u32)?;
        reg.1.insert(
            id,
            Arc::new(Slot {
                cancelled,
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                op: Mutex::new(op),
            }),
        );
        unsafe { out.write(id) };
        Ok(())
    })
}
/// # Safety
/// Caller provides live, aligned, non-overlapping inputs and output capacities
/// for the entire call, as specified in `include/ambient_auth.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_auth_v1_prepare(
    h: u64,
    r: *mut AmbientAuthRecord,
    b: *mut u8,
    n: u32,
) -> u32 {
    boundary(|| {
        capacity(r, b, n)?;
        let url = with_op(h, AuthOperation::prepare)?;
        let mut record = AmbientAuthRecord::default();
        let mut bytes = Vec::new();
        if let Some(url) = url {
            record.url = span(&mut bytes, &url)
        }
        record.bytes_used = bytes.len() as u32;
        unsafe { write(r, b, (record, bytes)) }
    })
}
/// # Safety
/// Caller provides live, aligned, non-overlapping inputs and output capacities
/// for the entire call, as specified in `include/ambient_auth.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_auth_v1_callback(
    h: u64,
    url: *const u8,
    length: u32,
    r: *mut AmbientAuthRecord,
    b: *mut u8,
    n: u32,
) -> u32 {
    boundary(|| {
        capacity(r, b, n)?;
        let url = unsafe { text(url, length, 8192)? };
        let id = with_op(h, |op| op.accept_callback(url))?;
        let mut record = AmbientAuthRecord::default();
        let mut bytes = Vec::new();
        record.client_id = span(&mut bytes, &id);
        record.bytes_used = bytes.len() as u32;
        unsafe { write(r, b, (record, bytes)) }
    })
}
/// # Safety
/// Caller provides live, aligned, non-overlapping inputs and output capacities
/// for the entire call, as specified in `include/ambient_auth.h`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ambient_auth_v1_dispatch(
    h: u64,
    id: *const u8,
    length: u32,
    r: *mut AmbientAuthRecord,
    b: *mut u8,
    n: u32,
) -> u32 {
    boundary(|| {
        capacity(r, b, n)?;
        let id = unsafe { text(id, length, 512)? };
        let out = with_op(h, |op| op.dispatch(id))?;
        unsafe { write(r, b, encode(out)) }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_auth_v1_cancel(h: u64) -> u32 {
    boundary(|| {
        get(h)?.cancelled.store(true, Ordering::Release);
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn ambient_auth_v1_close(h: u64) -> u32 {
    boundary(|| {
        let slot = get(h)?;
        slot.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BUSY)?;
        slot.closed.store(true, Ordering::Release);
        AUTH.lock().map_err(|_| 10u32)?.1.remove(&h);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_buffers_are_checked_before_io_and_cancelled_handles_never_reopen() {
        assert_eq!(std::mem::size_of::<AmbientAuthRecord>(), 616);
        assert_eq!(std::mem::size_of::<AmbientAuthSpan>(), 8);
        let (record, bytes) = encode(Snapshot {
            registration: Registration {
                host: "urn:uuid:12345678-1234-4234-9234-123456789abc".into(),
                state: 0,
                client_id: None,
                identity: None,
                hint: None,
            },
            credentials: None,
        });
        let redirect = b"http://127.0.0.1:12345/auth/callback";
        let mut h = 0;
        assert_eq!(
            unsafe {
                ambient_auth_v1_begin(
                    0,
                    &record,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    redirect.as_ptr(),
                    redirect.len() as u32,
                    &mut h,
                )
            },
            0
        );
        let mut out = AmbientAuthRecord::default();
        let mut buffer = vec![0u8; RESULT_LIMIT];
        assert_eq!(
            unsafe { ambient_auth_v1_prepare(h, &mut out, buffer.as_mut_ptr(), 1) },
            11
        );
        assert_eq!(ambient_auth_v1_cancel(h), 0);
        assert_eq!(
            unsafe {
                ambient_auth_v1_prepare(h, &mut out, buffer.as_mut_ptr(), RESULT_LIMIT as u32)
            },
            3
        );
        assert_eq!(
            unsafe {
                ambient_auth_v1_dispatch(h, b"issued".as_ptr(), 6, &mut out, buffer.as_mut_ptr(), 1)
            },
            11
        );
        assert_eq!(ambient_auth_v1_close(h), 0);
        assert_eq!(ambient_auth_v1_cancel(h), 12);
        assert_eq!(ambient_auth_v1_close(h), 12);
        let mut bad = record;
        bad.has_hint = 2;
        assert_eq!(
            unsafe {
                ambient_auth_v1_begin(
                    0,
                    &bad,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    redirect.as_ptr(),
                    redirect.len() as u32,
                    &mut h,
                )
            },
            1
        );
    }
}
