use ambient_bridge::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Native {
    path: std::path::PathBuf,
    handle: u64,
}
impl Native {
    fn open() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ambient-abi2-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = path.to_string_lossy();
        let mut handle = 0;
        assert_eq!(
            unsafe { ambient_v3_open(bytes.as_ptr(), bytes.len() as u32, &mut handle) },
            OK
        );
        Self { path, handle }
    }
    fn create(&self, text: &str) -> u64 {
        let mut id = 0;
        assert_eq!(
            unsafe {
                ambient_v3_create_task(
                    self.handle,
                    AmbientText {
                        bytes: text.as_ptr(),
                        length: text.len() as u32,
                    },
                    &mut id,
                )
            },
            OK
        );
        id
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = ambient_v3_close(self.handle);
        let _ = std::fs::remove_file(&self.path);
    }
}
fn text(s: &str) -> AmbientText {
    AmbientText {
        bytes: s.as_ptr(),
        length: s.len() as u32,
    }
}
#[test]
fn typed_native_pages_revision_errors_buffers_and_disposal() {
    let n = Native::open();
    let id = n.create("🌿abc");
    let other = n.create("Other");
    assert_ne!(id, other);
    let mut page = AmbientPage::default();
    let mut records = [AmbientTask::default(); 50];
    let mut bytes = vec![0u8; 262144];
    assert_eq!(
        unsafe {
            ambient_v3_tasks(
                n.handle,
                0,
                50,
                &mut page,
                records.as_mut_ptr(),
                50,
                bytes.as_mut_ptr(),
                262144,
            )
        },
        OK
    );
    assert_eq!(page.count, 2);
    assert_eq!(records[0].id, id);
    assert_eq!(&bytes[..7], "🌿abc".as_bytes());
    let mut record = AmbientTask::default();
    let mut small = [127; 3];
    assert_eq!(
        unsafe { ambient_v3_task(n.handle, id, &mut record, small.as_mut_ptr(), 3) },
        BUFFER_TOO_SMALL
    );
    assert_eq!(record.text.length, 7);
    assert_eq!(small, [127; 3]);
    assert_eq!(
        unsafe { ambient_v3_edit_task(n.handle, id, 1, text("Edited")) },
        OK
    );
    assert_eq!(
        unsafe { ambient_v3_edit_task(n.handle, id, 1, text("Late")) },
        STALE_REVISION
    );
    assert_eq!(
        unsafe {
            ambient_v3_edit_task(
                n.handle,
                id,
                2,
                AmbientText {
                    bytes: [255].as_ptr(),
                    length: 1,
                },
            )
        },
        INVALID_UTF8
    );
    assert_eq!(
        unsafe {
            ambient_v3_edit_task(
                n.handle,
                id,
                2,
                AmbientText {
                    bytes: b"x".as_ptr(),
                    length: 16385,
                },
            )
        },
        TOO_LONG
    );
    assert_eq!(
        unsafe {
            ambient_v3_tasks(
                n.handle,
                0,
                51,
                &mut page,
                records.as_mut_ptr(),
                50,
                bytes.as_mut_ptr(),
                262144,
            )
        },
        INVALID_ARGUMENT
    );
    assert_eq!(ambient_v3_set_manual_pause(n.handle, 2), INVALID_ARGUMENT);
    assert_eq!(ambient_v3_close(n.handle), OK);
    assert_eq!(ambient_v3_close(n.handle), INVALID_HANDLE);
}
#[test]
fn external_input_is_typed_bounded_and_forgotten_is_explicit() {
    let n = Native::open();
    let id = n.create("A");
    let mut key = AmbientSourceKey {
        namespace: text("fixture"),
        scope: text("account"),
        object_id: text("exact-key"),
        has_version: 0,
        version: AmbientText::default(),
    };
    let input = AmbientSourceEvidence {
        text: text("source assertion"),
        observed_at_utc: 0,
        has_observed_at: 0,
        completeness: 0,
        kind: 0,
    };
    let mut out = AmbientIngest::default();
    assert_eq!(
        unsafe { ambient_v3_ingest_source(n.handle, id, 1, &key, &input, &mut out) },
        OK
    );
    assert_eq!(out.outcome, 0);
    let eid = out.evidence_id;
    assert_eq!(
        unsafe { ambient_v3_delete_evidence(n.handle, eid, id, 2, std::ptr::null(), 0) },
        OK
    );
    key.has_version = 1;
    key.version = text("new-version");
    assert_eq!(
        unsafe { ambient_v3_ingest_source(n.handle, id, 3, &key, &input, &mut out) },
        OK
    );
    assert_eq!(out.outcome, 1);
    assert_eq!(out.evidence_id, 0);
    let bad = AmbientSourceEvidence { kind: 99, ..input };
    assert_eq!(
        unsafe { ambient_v3_ingest_source(n.handle, id, 3, &key, &bad, &mut out) },
        INVALID_ARGUMENT
    );
    key.namespace = text("bad\0key");
    assert_eq!(
        unsafe { ambient_v3_ingest_source(n.handle, id, 3, &key, &input, &mut out) },
        INVALID_ARGUMENT
    );
}
#[test]
fn plan_v3_optional_text_and_completion_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let path =
            std::env::temp_dir().join(format!("ambient-abi-plan-{}.sqlite3", std::process::id()));
        let p = path.to_string_lossy();
        let mut h = 0;
        assert_eq!(ambient_v3_open(p.as_ptr(), p.len() as u32, &mut h), OK);
        let text = |s: &str| AmbientText {
            bytes: s.as_ptr(),
            length: s.len() as u32,
        };
        let mut task = 0;
        assert_eq!(ambient_v3_create_task(h, text("Affair"), &mut task), OK);
        let exact = " e\u{301}\0🌿\n";
        let reference = "ref:🌿";
        let mut id = 0;
        let input = AmbientStepInput {
            description: text("Step"),
            has_prompt: 1,
            prompt: text(exact),
            has_reference: 1,
            reference: text(reference),
        };
        assert_eq!(ambient_v3_add_step(h, task, 1, &input, &mut id), OK);
        let mut page = AmbientPage::default();
        let mut records = [AmbientStep::default(); 50];
        let mut bytes = vec![0; 262144];
        assert_eq!(
            ambient_v3_steps(
                h,
                task,
                0,
                50,
                &mut page,
                records.as_mut_ptr(),
                50,
                bytes.as_mut_ptr(),
                262144
            ),
            OK
        );
        assert_eq!(page.task_revision, 2);
        assert_eq!(page.plan_completeness, 1);
        let r = records[0];
        assert_eq!(r.has_prompt, 1);
        assert_eq!(r.ready, 0);
        assert_eq!(
            &bytes[r.prompt.offset as usize..(r.prompt.offset + r.prompt.length) as usize],
            exact.as_bytes()
        );
        let bad = AmbientStepInput {
            has_prompt: 2,
            ..input
        };
        assert_eq!(ambient_v3_edit_step(h, task, 2, id, &bad), INVALID_ARGUMENT);
        assert_eq!(ambient_v3_set_plan_completeness(h, task, 2, 2), OK);
        assert_eq!(ambient_v3_set_step_completed(h, task, 3, id, 1, 42), OK);
        let refs = [text(reference)];
        assert_eq!(
            ambient_v3_complete_task(
                h,
                task,
                4,
                text("Summary"),
                std::ptr::null(),
                0,
                refs.as_ptr(),
                1,
                std::ptr::null(),
                0
            ),
            OK
        );
        let mut untouched = 999;
        assert_eq!(
            ambient_v3_add_note(h, task, 5, text("late"), 44, &mut untouched),
            TASK_COMPLETED
        );
        assert_eq!(untouched, 999);
        assert_eq!(ambient_v3_close(h), OK);
        std::fs::remove_file(path)?;
    }
    Ok(())
}
#[test]
fn local_source_settings_are_typed_bounded_and_gate_errors_leave_outputs_unchanged() {
    let n = Native::open();
    let mut revision = 999;
    assert_eq!(
        unsafe {
            ambient_source_v1_settings_save(
                n.handle,
                text("/private/tmp/project"),
                0,
                std::ptr::null(),
                0,
                &mut revision,
            )
        },
        0
    );
    assert!(revision > 0 && revision != 999);
    let mut out = AmbientSourceSettings {
        revision: 999,
        ..Default::default()
    };
    let mut spans = [AmbientSpan::default(); 16];
    let mut bytes = vec![0u8; 16384];
    assert_eq!(
        unsafe {
            ambient_source_v1_settings_get(
                n.handle,
                text("/private/tmp/project"),
                &mut out,
                spans.as_mut_ptr(),
                16,
                bytes.as_mut_ptr(),
                16383,
            )
        },
        6
    );
    assert_eq!(out.revision, 999);
    let invalid = [text("../outside")];
    let mut unchanged = 999;
    assert_eq!(
        unsafe {
            ambient_source_v1_settings_save(
                n.handle,
                text("/private/tmp/project"),
                revision,
                invalid.as_ptr(),
                1,
                &mut unchanged,
            )
        },
        1
    );
    assert_eq!(unchanged, 999);
    assert_eq!(
        unsafe {
            ambient_source_v1_settings_get(
                n.handle,
                text("/private/tmp/project"),
                &mut out,
                spans.as_mut_ptr(),
                16,
                bytes.as_mut_ptr(),
                16384,
            )
        },
        0
    );
    assert_eq!(out.revision, revision);
    let task = n.create("task");
    let mut operation = 999;
    assert_eq!(
        unsafe {
            ambient_source_v1_begin(
                n.handle,
                task,
                1,
                text("/private/tmp/project"),
                revision,
                &mut operation,
            )
        },
        12
    );
    assert_eq!(operation, 999);
    let deleted = unsafe {
        ambient_source_v1_settings_delete(n.handle, text("/private/tmp/project"), revision)
    };
    assert_eq!(deleted, 0);
    assert_eq!(
        unsafe {
            ambient_source_v1_settings_save(
                n.handle,
                text("/private/tmp/project"),
                0,
                std::ptr::null(),
                0,
                &mut unchanged,
            )
        },
        0
    );
    assert!(unchanged > revision);
    assert_eq!(std::mem::size_of::<AmbientSourceSettings>(), 24);
    assert_eq!(std::mem::size_of::<AmbientSourceBranch>(), 24);
    assert_eq!(std::mem::size_of::<AmbientSourceObservation>(), 32);
}
