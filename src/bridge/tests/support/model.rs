// Separately enabled test build only. Never linked into the production application.
use super::*;
/// # Safety
/// Same ownership/disjoint buffers as ambient_model_v6_dispatch; URL is test-only loopback UTF-8.
/// Plugin servers are reached at the loopback URLs stored by a test-support Core.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ambient_model_test_dispatch(
    handle: u64,
    token: *const u8,
    token_length: u32,
    keys: *const AmbientPluginKey,
    key_count: u32,
    root: crate::AmbientText,
    url: *const u8,
    url_length: u32,
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
        let url = unsafe {
            crate::input(
                crate::AmbientText {
                    bytes: url,
                    length: url_length,
                },
                4096,
            )
        }
        .map_err(|_| INVALID)?;
        let keys = unsafe { super::keys(keys, key_count) }?;
        let root = unsafe { super::root(root) }?;
        let mut record = AmbientModelCompletion::default();
        let status = dispatch_with(
            handle,
            text,
            &keys,
            root,
            &mut record,
            |permit| ambient_providers::run_model_loopback(permit, text, url),
            |step| ambient_providers::run_model_step_loopback(step, text, url),
            |host, permit, key| host.call_tool(permit, key),
        );
        if status != 0 {
            return Err(status);
        }
        unsafe { out.write(record) };
        Ok(())
    })
}
