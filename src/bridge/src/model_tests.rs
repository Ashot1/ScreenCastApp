use super::*;
use std::{
    sync::{Barrier, atomic::AtomicU64},
    thread,
};
static NEXT: AtomicU64 = AtomicU64::new(1);
/// The connectivity path: no plugin loop and no tool transport.
fn check_only(
    h: u64,
    out: &mut AmbientModelCompletion,
    check: impl FnOnce(CheckDispatch) -> ModelResponse,
) -> u32 {
    dispatch_with(
        h,
        "test",
        &Vec::new(),
        std::path::Path::new("/nonexistent-root"),
        out,
        check,
        |_| panic!("connectivity has no plugin loop"),
        |_, _, _| panic!("connectivity calls no tools"),
    )
}
fn memory() -> Result<(u64, std::path::PathBuf), Box<dyn std::error::Error>> {
    let p = std::env::temp_dir().join(format!(
        "ambient-model-ffi-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let b = p.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    assert_eq!(
        unsafe { crate::ambient_v3_open(b.as_ptr(), b.len() as u32, &mut h) },
        0
    );
    Ok((h, p))
}
#[test]
fn model_buffers_pause_and_cancel_before_dispatch_do_not_create_attempts()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    let mut h = 0;
    assert_eq!(
        unsafe { ambient_model_v6_begin(m, 0, 1, 1, 0, &mut h) },
        REPORT
    );
    assert_eq!(crate::ambient_v3_set_manual_pause(m, 1), 0);
    assert_eq!(
        unsafe { ambient_model_v6_begin(m, 1, 1, 1, 0, &mut h) },
        PAUSED
    );
    assert_eq!(crate::ambient_v3_set_manual_pause(m, 0), 0);
    assert_eq!(unsafe { ambient_model_v6_begin(m, 1, 1, 1, 0, &mut h) }, 0);
    assert_eq!(
        unsafe {
            ambient_model_v6_dispatch(
                h,
                b"test".as_ptr(),
                4,
                std::ptr::null(),
                0,
                crate::AmbientText {
                    bytes: b"/tmp".as_ptr(),
                    length: 4,
                },
                std::ptr::null_mut(),
            )
        },
        INVALID
    );
    assert_eq!(ambient_model_v6_cancel(h), 0);
    let mut r = AmbientModelCompletion::default();
    assert_eq!(
        check_only(h, &mut r, |_| panic!("cancel must deny transport")),
        CANCELLED
    );
    assert_eq!(ambient_model_v6_close(h), 0);
    let mut summary = AmbientModelSummary::default();
    assert_eq!(unsafe { ambient_model_v6_summary(m, &mut summary) }, 0);
    assert_eq!(summary.attempts, 0);
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
#[test]
fn model_busy_waits_for_real_return_and_memory_close_keeps_accounting_alive()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    let mut h = 0;
    assert_eq!(unsafe { ambient_model_v6_begin(m, 1, 1, 1, 0, &mut h) }, 0);
    let accepted = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let a = accepted.clone();
    let b = release.clone();
    let worker = thread::spawn(move || {
        let mut r = AmbientModelCompletion::default();
        let status = check_only(h, &mut r, move |_| {
            a.wait();
            b.wait();
            ModelResponse {
                check: ModelCheckResult {
                    outcome: CheckOutcome::Confirmed,
                    actual: ActualConfig {
                        model: ActualModel::Luna,
                        effort: ActualEffort::Low,
                        tier: ActualTier::Priority,
                    },
                    usage: Some(CheckUsage::new(8, 3, 11).unwrap_or_else(|_| panic!("fixture"))),
                },
                ..ModelResponse::unknown(CheckOutcome::Confirmed)
            }
        });
        (status, r)
    });
    accepted.wait();
    assert_eq!(ambient_model_v6_close(h), BUSY);
    assert_eq!(crate::ambient_v3_set_manual_pause(m, 1), 0);
    assert_eq!(crate::ambient_v3_close(m), 0);
    release.wait();
    let (status, r) = worker.join().map_err(|_| "worker")?;
    assert_eq!(status, 0);
    assert_eq!(r.attempt.outcome, CheckOutcome::Cancelled as u32);
    assert_eq!(r.attempt.total_tokens, 11);
    assert_eq!(ambient_model_v6_close(h), 0);
    let s = Store::open(&p)?.check_summary()?;
    assert_eq!(s.total, 11);
    assert_eq!(
        s.latest.map(|a| a.result.outcome),
        Some(CheckOutcome::Cancelled)
    );
    std::fs::remove_file(p)?;
    Ok(())
}
#[test]
fn completed_close_keeps_capability_but_unused_assistance_close_discards_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    assert_eq!(ambient_model_v6_session(m, 42, 1, 1), 0);
    let mut h = 0;
    assert_eq!(unsafe { ambient_model_v6_begin(m, 1, 2, 1, 0, &mut h) }, 0);
    let mut out = AmbientModelCompletion::default();
    assert_eq!(
        check_only(h, &mut out, |_| ModelResponse {
            check: ModelCheckResult {
                outcome: CheckOutcome::Confirmed,
                actual: ActualConfig {
                    model: ActualModel::Luna,
                    effort: ActualEffort::High,
                    tier: ActualTier::Fast
                },
                usage: CheckUsage::new(8, 3, 11).ok()
            },
            ..ModelResponse::unknown(CheckOutcome::Confirmed)
        }),
        0
    );
    assert_eq!(ambient_model_v6_close(h), 0);
    let store = crate::get_store(m).map_err(|_| "memory")?;
    let preview = {
        let mut s = store.store.lock().map_err(|_| "store")?;
        assert!(s.assistance_available()?);
        let task = s.create_task("saved intent")?;
        s.prepare_assistance(AssistanceRequest {
            task: TaskVersion {
                task_id: task.id,
                revision: task.revision,
            },
            workspace_revision: 1,
            mode: AssistanceMode::NewContext,
            selected_evidence: vec![],
            reply: None,
        })?
    };
    assert_eq!(
        unsafe { ambient_model_v6_begin(m, 1, 2, 2, preview.request_id, &mut h) },
        0
    );
    assert_eq!(ambient_model_v6_close(h), 0);
    assert!(
        store
            .store
            .lock()
            .map_err(|_| "store")?
            .assistance_preview(preview.request_id)
            .is_err()
    );
    assert_eq!(crate::ambient_v3_close(m), 0);
    drop(store);
    std::fs::remove_file(p)?;
    Ok(())
}

#[test]
fn completion_observes_cancel_after_accounting_without_a_false_confirmed_receipt()
-> Result<(), StoreError> {
    let id = AttemptId::from_raw(1)?;
    let publication = WorkspaceVersion {
        task: TaskVersion {
            task_id: TaskId(1),
            revision: 1,
        },
        workspace_revision: 2,
    };
    for value in [None, Some(publication)] {
        let r = completion(
            CheckAttempt {
                id,
                at_utc: 1,
                stage: 2,
                purpose: ModelPurpose::Assistance,
                profile: ModelProfile::High,
                result: ModelCheckResult {
                    outcome: CheckOutcome::Confirmed,
                    actual: ActualConfig {
                        model: ActualModel::Luna,
                        effort: ActualEffort::High,
                        tier: ActualTier::Fast,
                    },
                    usage: Some(CheckUsage::new(8, 3, 11)?),
                },
            },
            value,
            &AtomicBool::new(true),
        );
        assert_eq!(r.attempt.outcome, CheckOutcome::Cancelled as u32);
        assert_eq!(r.attempt.total_tokens, 11);
        assert_eq!(r.published_task, 0);
        assert_eq!(r.workspace_revision, 0);
    }
    Ok(())
}

fn high() -> ModelCheckResult {
    ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Fast,
        },
        usage: CheckUsage::new(8, 3, 11).ok(),
    }
}
fn question_fixture(m: u64, large: bool) -> Result<Proposal, u32> {
    super::memory(m, |s| {
        s.set_model_session(42, true, true).map_err(error)?;
        let a = s.prepare_check(true, ModelProfile::High).map_err(error)?;
        let f = a.cancellation();
        let d = s.dispatch_check(a, 1).map_err(error)?;
        s.finish_check(d.id(), high()).map_err(error)?;
        s.release_check(&f);
        let mut t = s.create_task("intent").map_err(error)?;
        let mut notes = vec![];
        if large {
            for _ in 0..8 {
                notes.push(
                    s.add_note(t.id, t.revision, &"\\".repeat(8192), 1)
                        .map_err(error)?
                        .id,
                );
                t = s.task(t.id).map_err(error)?;
            }
        }
        let p = s
            .prepare_assistance(AssistanceRequest {
                task: TaskVersion {
                    task_id: t.id,
                    revision: t.revision,
                },
                workspace_revision: 1,
                mode: AssistanceMode::NewContext,
                selected_evidence: notes,
                reply: None,
            })
            .map_err(error)?;
        ambient_providers::validate_assistance_body(&p.context).map_err(|_| LIMIT)?;
        let a = s.begin_assistance(p.request_id).map_err(error)?;
        let f = a.cancellation();
        let d = s.dispatch_check(a, 2).map_err(error)?;
        let answer = AssistanceAnswer {
            proposals: vec![ProposalAnswer {
                proposal_id: None,
                kind: AssistanceKind::Clarification,
                message: if large {
                    "\\".repeat(8192)
                } else {
                    "Question?".into()
                },
                rationale: "Saved".into(),
                evidence_ids: vec![],
                step_id: None,
            }],
            rationale: "Full set".into(),
        };
        let result = s
            .finish_assistance(d.id(), high(), Some(answer))
            .map_err(error);
        s.release_check(&f);
        result?;
        s.proposal_feed()
            .map_err(error)?
            .proposals
            .into_iter()
            .next()
            .ok_or(INVALID)
    })
}
#[test]
fn abi4_feed_details_and_prepare_capacities_do_not_mutate_on_error()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, path) = memory()?;
    let p = question_fixture(m, false).map_err(|e| format!("fixture {e}"))?;
    assert_eq!(std::mem::size_of::<AmbientAssistancePreview>(), 48);
    assert_eq!(std::mem::size_of::<AmbientAssistanceRequest>(), 48);
    assert_eq!(std::mem::size_of::<AmbientModelCompletion>(), 96);
    let mut bytes = vec![0; MAX_ASSISTANCE_CONTEXT];
    let mut out = AmbientAssistancePreview {
        request_id: 999,
        ..Default::default()
    };
    let request = AmbientAssistanceRequest {
        task_id: p.task.task_id.0 as u64,
        revision: p.task.revision,
        workspace_revision: p.workspace_revision,
        proposal_id: p.id.get(),
        content_revision: p.content_revision,
        mode: 1,
        reserved: 0,
    };
    assert_eq!(
        unsafe {
            ambient_model_v6_prepare(
                m,
                request,
                std::ptr::null(),
                0,
                b"No".as_ptr(),
                2,
                &mut out,
                bytes.as_mut_ptr(),
                1,
            )
        },
        INVALID
    );
    assert_eq!(out.request_id, 999);
    assert_eq!(
        unsafe {
            ambient_model_v6_prepare(
                m,
                request,
                std::ptr::null(),
                0,
                b"No".as_ptr(),
                2,
                &mut out,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
            )
        },
        0
    );
    assert!(std::str::from_utf8(&bytes[..out.context.length as usize])?.contains("model_question"));
    let target = AmbientProposalTarget {
        proposal_id: p.id.get(),
        content_revision: p.content_revision,
    };
    assert_eq!(ambient_model_v6_visibility(m, target, 0), 0);
    assert_eq!(crate::ambient_v3_set_manual_pause(m, 1), 0);
    let mut length = 999;
    assert_eq!(
        unsafe {
            ambient_model_v6_details(
                m,
                target,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                &mut length,
            )
        },
        0
    );
    let mut feed = vec![0; MAX_PROPOSAL_FEED];
    assert_eq!(
        unsafe { ambient_model_v6_feed(m, feed.as_mut_ptr(), 1, &mut length) },
        INVALID
    );
    assert_eq!(
        unsafe { ambient_model_v6_feed(m, feed.as_mut_ptr(), feed.len() as u32, &mut length) },
        0
    );
    assert!(std::str::from_utf8(&feed[..length as usize])?.contains("\"visible\":false"));
    let mut kind = 999;
    assert_eq!(
        unsafe { ambient_model_v6_validate(m, target, &mut kind) },
        PAUSED
    );
    assert_eq!(kind, 999);
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn continued_body_overflow_preserves_previous_discussion_and_expense()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, path) = memory()?;
    let p = question_fixture(m, true).map_err(|e| format!("fixture {e}"))?;
    let request = AmbientAssistanceRequest {
        task_id: p.task.task_id.0 as u64,
        revision: p.task.revision,
        workspace_revision: p.workspace_revision,
        proposal_id: p.id.get(),
        content_revision: p.content_revision,
        mode: 1,
        reserved: 0,
    };
    let text = "\\".repeat(16384);
    let mut bytes = vec![0; MAX_ASSISTANCE_CONTEXT];
    let mut out = AmbientAssistancePreview {
        request_id: 999,
        ..Default::default()
    };
    assert_eq!(
        unsafe {
            ambient_model_v6_prepare(
                m,
                request,
                std::ptr::null(),
                0,
                text.as_ptr(),
                text.len() as u32,
                &mut out,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
            )
        },
        LIMIT
    );
    assert_eq!(out.request_id, 999);
    super::memory(m, |s| {
        assert_eq!(s.copy_proposal(p.target()).map_err(error)?, p.message);
        assert_eq!(s.check_summary().map_err(error)?.attempts, 2);
        Ok(())
    })
    .map_err(|e| format!("check {e}"))?;
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
