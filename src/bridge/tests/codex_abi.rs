use ambient_bridge::*;
#[test]
fn codex_layout_settings_and_closed_gate_use_existing_memory_handle()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(std::mem::size_of::<AmbientCodexSettings>(), 32);
    assert_eq!(std::mem::size_of::<AmbientCodexFile>(), 24);
    assert_eq!(std::mem::size_of::<AmbientCodexPlan>(), 16);
    assert_eq!(std::mem::size_of::<AmbientCodexEvent>(), 72);
    assert_eq!(std::mem::size_of::<AmbientCodexWindow>(), 48);
    assert_eq!(std::mem::size_of::<AmbientCodexMetadata>(), 64);
    let path =
        std::env::temp_dir().join(format!("ambient-codex-abi-{}.sqlite3", std::process::id()));
    let p = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    unsafe {
        assert_eq!(ambient_v3_open(p.as_ptr(), p.len() as u32, &mut h), 0);
    }
    let p = b"/private/tmp/selected.jsonl";
    let mut id = 0;
    let mut revision = 0;
    unsafe {
        assert_eq!(
            ambient_codex_v1_settings_save(
                h,
                0,
                0,
                AmbientText {
                    bytes: p.as_ptr(),
                    length: p.len() as u32
                },
                &mut id,
                &mut revision
            ),
            0
        );
    }
    let mut out = AmbientCodexSettings::default();
    let mut bytes = [0u8; 4096];
    unsafe {
        assert_eq!(
            ambient_codex_v1_settings_get(h, id, &mut out, bytes.as_mut_ptr(), 4096),
            0
        );
    }
    assert_eq!(out.id, id);
    assert_eq!(out.revision, revision);
    assert_eq!(&bytes[..out.bytes_used as usize], p);
    let mut op = 99;
    unsafe {
        assert_ne!(ambient_codex_v1_begin(h, 1, 1, id, revision, 0, &mut op), 0);
    }
    assert_eq!(op, 99);
    assert_eq!(ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
