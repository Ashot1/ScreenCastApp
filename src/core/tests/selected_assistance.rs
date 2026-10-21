#[path = "support/assistance.rs"]
pub mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-assist-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn high() -> ModelCheckResult {
    ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Fast,
        },
        usage: Some(CheckUsage::new(8, 3, 11).unwrap_or_else(|_| panic!("fixture"))),
    }
}
fn ready(s: &mut Store) -> Result<(), StoreError> {
    s.set_model_session(42, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(d.id(), high())?;
    s.release_check(&flag);
    Ok(())
}
fn answer(
    kind: AssistanceKind,
    message: Option<&str>,
    ids: &[String],
    step: Option<StepId>,
) -> AssistanceAnswer {
    AssistanceAnswer {
        proposals: vec![ProposalAnswer {
            proposal_id: None,
            kind,
            message: message.unwrap_or("").to_owned(),
            rationale: "Based on the saved task".into(),
            evidence_ids: ids.to_vec(),
            step_id: step,
        }],
        rationale: "Full set".into(),
    }
}
fn run(
    s: &mut Store,
    request: u64,
    answer: AssistanceAnswer,
) -> Result<ProposalTarget, StoreError> {
    let a = s.begin_assistance(request)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    let id = d.id();
    let r = finish_single(s, id, high(), Some(answer));
    s.release_check(&flag);
    r?.ok_or(StoreError::InvalidInput)
}
#[test]
fn exact_snapshot_contains_full_plan_and_saved_manual_text_only()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let task = s.create_task("saved intent")?;
    let note = s.add_note(task.id, task.revision, "saved note", 123)?;
    let other = s.create_task("excluded other task")?;
    s.add_note(other.id, other.revision, "excluded other note", 1)?;
    for n in 0..51 {
        let t = s.task(task.id)?;
        s.add_step(
            task.id,
            t.revision,
            &StepInput {
                description: format!("step {n}"),
                prompt: Some(format!("prompt {n}")),
                reference: Some(format!("https://example.test/{n}")),
            },
        )?;
    }
    let steps = s.steps(task.id, 0, 50)?;
    let last = s
        .steps(task.id, steps.next_after.ok_or("next")?, 50)?
        .items
        .remove(0);
    let t = s.task(task.id)?;
    s.add_check(task.id, t.revision, last.id, "last check")?;
    let t = s.task(task.id)?;
    s.add_dependency(task.id, t.revision, steps.items[0].id, last.id)?;
    let t = s.task(task.id)?;
    let p = prepare_new(&mut s, task.id, t.revision, &[note.id])?;
    let v: serde_json::Value = serde_json::from_str(&p.context)?;
    assert_eq!(v["steps"].as_array().ok_or("steps")?.len(), 51);
    assert_eq!(v["notes"][0]["text"], "saved note");
    assert_eq!(v["notes"][0]["origin"], "user");
    assert_eq!(v["checks"][0]["description"], "last check");
    assert_eq!(v["dependencies"].as_array().ok_or("deps")?.len(), 1);
    assert!(!p.context.contains("excluded other"));
    assert!(p.context.contains("prompt 50"));
    assert_eq!(s.assistance_preview(p.request_id)?.context, p.context);
    s.edit_evidence(note.id, task.id, t.revision, "changed", 2)?;
    assert!(matches!(
        s.begin_assistance(p.request_id),
        Err(StoreError::StaleRevision)
    ));
    assert_eq!(s.check_summary()?.attempts, 1);
    Ok(())
}
#[test]
fn only_current_grounded_result_can_supply_exact_copy_text()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("draft release")?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let r = run(
        &mut s,
        p.request_id,
        answer(
            AssistanceKind::Suggestion,
            Some("copy\0exact 🌿"),
            &[format!("task:{}", t.id.0)],
            None,
        ),
    )?;
    assert_eq!(s.copy_proposal(r)?, "copy\0exact 🌿");
    assert_eq!(s.proposal_details(r)?.citations[0].text, "draft release");
    s.edit_task(t.id, t.revision, "new intent")?;
    assert!(matches!(s.copy_proposal(r), Err(StoreError::StaleRevision)));
    assert_eq!(s.task(t.id)?.text, "new intent");
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 0);
    Ok(())
}
#[test]
fn incomplete_plans_and_unready_steps_never_become_suggestions()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let step = s.add_step(
        t.id,
        t.revision,
        &StepInput {
            description: "first".into(),
            prompt: None,
            reference: None,
        },
    )?;
    let t = s.task(t.id)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    assert_eq!(
        run(
            &mut s,
            p.request_id,
            answer(
                AssistanceKind::Suggestion,
                Some("go"),
                &[format!("step:{}", step.0)],
                Some(step)
            )
        ),
        Err(StoreError::InvalidInput)
    );
    ready(&mut s)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let r = run(
        &mut s,
        p.request_id,
        answer(
            AssistanceKind::Clarification,
            Some("Is the plan complete?"),
            &[format!("task:{}", t.id.0)],
            None,
        ),
    )?;
    assert_eq!(
        s.proposal_details(r)?.proposal.kind,
        AssistanceKind::Clarification
    );
    s.delete_step(t.id, t.revision, step)?;
    let t = s.task(t.id)?;
    assert_eq!(t.plan_completeness, Completeness::Partial);
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    assert_eq!(
        run(
            &mut s,
            p.request_id,
            answer(
                AssistanceKind::Suggestion,
                Some("go"),
                &[format!("task:{}", t.id.0)],
                None
            )
        ),
        Err(StoreError::InvalidInput)
    );
    ready(&mut s)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    assert!(
        run(
            &mut s,
            p.request_id,
            AssistanceAnswer {
                proposals: vec![],
                rationale: "No suggestion".into()
            }
        )
        .is_err()
    );
    assert!(s.proposal_feed()?.proposals.is_empty());
    Ok(())
}
#[test]
fn duplicate_foreign_external_and_oversize_context_are_rejected_before_dispatch()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let note = s.add_note(t.id, t.revision, "one", 1)?;
    let t = s.task(t.id)?;
    assert!(prepare_new(&mut s, t.id, t.revision, &[note.id, note.id]).is_err());
    let other = s.create_task("other")?;
    let foreign = s.add_note(other.id, other.revision, "foreign", 1)?;
    assert!(prepare_new(&mut s, t.id, t.revision, &[foreign.id]).is_err());
    let key = SourceKey {
        namespace: "test".into(),
        scope: "test".into(),
        object_id: "1".into(),
    };
    let external = s.bind_source(
        t.id,
        t.revision,
        &key,
        None,
        &SourceEvidence {
            text: "outside".into(),
            observed_at_utc: None,
            completeness: Completeness::Partial,
            kind: AssertionKind::SourceFact,
        },
        1,
    )?;
    let t = s.task(t.id)?;
    assert!(prepare_new(&mut s, t.id, t.revision, &[external]).is_err());
    let mut ids = Vec::new();
    for _ in 0..16 {
        let t = s.task(t.id)?;
        ids.push(s.add_note(t.id, t.revision, &"x".repeat(16384), 1)?.id)
    }
    let t = s.task(t.id)?;
    assert!(matches!(
        prepare_new(&mut s, t.id, t.revision, &ids),
        Err(StoreError::TextTooLong)
    ));
    assert_eq!(s.check_summary()?.attempts, 1);
    Ok(())
}
#[test]
fn changed_epoch_pause_cancel_and_invalid_citations_cannot_publish_or_copy()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    assert_eq!(
        run(
            &mut s,
            p.request_id,
            answer(
                AssistanceKind::Suggestion,
                Some("go"),
                &["task:99999".into()],
                None
            )
        ),
        Err(StoreError::InvalidInput)
    );
    ready(&mut s)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    s.set_manual_pause(true)?;
    s.set_manual_pause(false)?;
    assert!(
        finish_single(
            &mut s,
            d.id(),
            high(),
            Some(answer(
                AssistanceKind::Suggestion,
                Some("late"),
                &[format!("task:{}", t.id.0)],
                None
            ))
        )?
        .is_none()
    );
    s.release_check(&flag);
    assert_eq!(s.check_summary()?.total, 44);
    assert!(s.assistance_available()?);
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let r = run(
        &mut s,
        p.request_id,
        answer(
            AssistanceKind::Suggestion,
            Some("current"),
            &[format!("task:{}", t.id.0)],
            None,
        ),
    )?;
    s.set_model_session(43, true, true)?;
    assert!(s.copy_proposal(r).is_err());
    assert!(!s.assistance_available()?);
    Ok(())
}
#[test]
fn one_preview_authorizes_one_dispatch_even_after_operation_release()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    s.finish_check(d.id(), high())?;
    s.release_check(&flag);
    assert!(matches!(
        s.begin_assistance(p.request_id),
        Err(StoreError::StaleRevision)
    ));
    assert_eq!(s.check_summary()?.attempts, 2);
    Ok(())
}
#[test]
fn failed_high_accounting_commit_never_opens_work_context() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    s.set_model_session(42, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute_batch("CREATE TRIGGER reject_result BEFORE UPDATE ON connection_checks BEGIN SELECT RAISE(ABORT,'test'); END;")?;
    assert!(s.finish_check(d.id(), high()).is_err());
    s.release_check(&flag);
    assert!(!s.assistance_available()?);
    assert_eq!(s.check_summary()?.unknown, 1);
    Ok(())
}
#[test]
fn invalid_grounding_is_an_error_in_the_same_ledger_and_keeps_usage()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    let receipt = finish_single(
        &mut s,
        d.id(),
        high(),
        Some(answer(
            AssistanceKind::Suggestion,
            Some("unsupported"),
            &["note:9999".into()],
            None,
        )),
    );
    s.release_check(&flag);
    assert_eq!(receipt, Err(StoreError::InvalidInput));
    assert_eq!(
        s.check_summary()?.latest.map(|r| r.result.outcome),
        Some(CheckOutcome::Invalid)
    );
    assert_eq!(s.check_summary()?.total, 22);
    assert!(!s.assistance_available()?);
    Ok(())
}
#[test]
fn saved_references_and_ready_step_grounding_are_part_of_the_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("old intent")?;
    s.complete_task(
        t.id,
        t.revision,
        "saved summary",
        &[],
        &["https://example.test/retained".into()],
        &[],
    )?;
    let t = s.task(t.id)?;
    s.reopen_task(t.id, t.revision)?;
    let t = s.task(t.id)?;
    let a = s.add_step(
        t.id,
        t.revision,
        &StepInput {
            description: "first".into(),
            prompt: None,
            reference: None,
        },
    )?;
    let t = s.task(t.id)?;
    let b = s.add_step(
        t.id,
        t.revision,
        &StepInput {
            description: "second".into(),
            prompt: None,
            reference: None,
        },
    )?;
    let t = s.task(t.id)?;
    s.add_dependency(t.id, t.revision, a, b)?;
    let t = s.task(t.id)?;
    s.set_plan_completeness(t.id, t.revision, Completeness::Complete)?;
    let t = s.task(t.id)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let json: serde_json::Value = serde_json::from_str(&p.context)?;
    assert_eq!(
        json["useful_references"][0]["text"],
        "https://example.test/retained"
    );
    assert_eq!(json["steps"][0]["ready"], true);
    assert_eq!(json["steps"][1]["ready"], false);
    let r = run(
        &mut s,
        p.request_id,
        answer(
            AssistanceKind::Suggestion,
            Some("first next"),
            &[format!("step:{}", a.0)],
            Some(a),
        ),
    )?;
    assert_eq!(s.copy_proposal(r)?, "first next");
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    assert!(
        run(
            &mut s,
            p.request_id,
            answer(
                AssistanceKind::Suggestion,
                Some("second early"),
                &[format!("step:{}", b.0)],
                Some(b)
            )
        )
        .is_err()
    );
    ready(&mut s)?;
    s.set_plan_completeness(t.id, t.revision, Completeness::Unknown)?;
    let t = s.task(t.id)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    assert!(
        run(
            &mut s,
            p.request_id,
            answer(
                AssistanceKind::Suggestion,
                Some("unknown plan"),
                &[format!("task:{}", t.id.0)],
                None
            )
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn accounting_failure_revokes_a_previously_confirmed_high_capability()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute_batch("CREATE TRIGGER reject_result BEFORE UPDATE ON connection_checks BEGIN SELECT RAISE(ABORT,'test'); END;")?;
    assert!(s.finish_check(d.id(), high()).is_err());
    s.release_check(&flag);
    assert!(!s.assistance_available()?);
    assert_eq!(s.check_summary()?.unknown, 1);
    Ok(())
}
#[test]
fn committed_capability_and_receipt_still_observe_their_original_cancellation_signal()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    s.set_model_session(42, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(d.id(), high())?;
    s.release_check(&flag);
    assert!(s.assistance_available()?);
    flag.store(true, Ordering::Release);
    assert!(!s.assistance_available()?);
    ready(&mut s)?;
    let task = s.create_task("intent")?;
    let p = prepare_new(&mut s, task.id, task.revision, &[])?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    let receipt = finish_single(
        &mut s,
        d.id(),
        high(),
        Some(answer(
            AssistanceKind::Suggestion,
            Some("copy"),
            &[format!("task:{}", task.id.0)],
            None,
        )),
    )?
    .ok_or("receipt")?;
    s.release_check(&flag);
    assert_eq!(s.copy_proposal(receipt)?, "copy");
    flag.store(true, Ordering::Release);
    assert!(s.assistance_preview(p.request_id).is_err());
    assert_eq!(s.copy_proposal(receipt)?, "copy");
    Ok(())
}

#[test]
fn reply_preview_replaces_only_pending_and_dispatches_exact_temporary_context()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("saved intent")?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let parent = run(
        &mut s,
        p.request_id,
        answer(
            AssistanceKind::Clarification,
            Some("Is the premise true?"),
            &[],
            None,
        ),
    )?;
    let before = std::fs::read(&db.0)?;
    let old = prepare_reply(&mut s, parent, "Yes")?;
    let latest = prepare_reply(&mut s, parent, "No\0 — exact 🌿")?;
    assert_eq!(s.copy_proposal(parent)?, "Is the premise true?");
    assert_eq!(std::fs::read(&db.0)?, before);
    assert!(matches!(
        s.begin_assistance(old.request_id),
        Err(StoreError::StaleRevision)
    ));
    let v: serde_json::Value = serde_json::from_str(&latest.context)?;
    assert_eq!(v["replies"][0]["model_question"], "Is the premise true?");
    assert_eq!(v["replies"][0]["user_reply"], "No\0 — exact 🌿");
    let a = s.begin_assistance(latest.request_id)?;
    let flag = a.cancellation();
    assert!(s.begin_assistance(latest.request_id).is_err());
    let d = s.dispatch_check(a, 3)?;
    assert_eq!(d.context(), Some(latest.context.as_str()));
    let next = finish_single(
        &mut s,
        d.id(),
        high(),
        Some(answer(
            AssistanceKind::Clarification,
            Some("Next question"),
            &[v["replies"][0]["evidence_id"]
                .as_str()
                .ok_or("reply id")?
                .into()],
            None,
        )),
    )?
    .ok_or("receipt")?;
    s.release_check(&flag);
    let citation = &s.proposal_details(next)?.citations[0];
    assert!(matches!(citation.reference, ProposalCitationRef::Reply(_)));
    assert_eq!(citation.text, "No\0 — exact 🌿");
    assert_eq!(
        citation.model_question.as_deref(),
        Some("Is the premise true?")
    );
    let second = prepare_reply(&mut s, next, "Second exact reply")?;
    let v: serde_json::Value = serde_json::from_str(&second.context)?;
    assert_eq!(v["replies"].as_array().ok_or("pairs")?.len(), 2);
    assert_eq!(s.task(t.id)?.revision, t.revision);
    assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    assert_eq!(s.check_summary()?.attempts, 3);
    s.set_manual_pause(true)?;
    assert!(matches!(
        prepare_reply(&mut s, next, "answer"),
        Err(StoreError::Paused)
    ));
    assert!(s.assistance_preview(second.request_id).is_err());
    Ok(())
}

#[test]
fn reply_limits_do_not_truncate_count_edits_or_write_memory()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let mut r = run(
        &mut s,
        p.request_id,
        answer(AssistanceKind::Clarification, Some("Question"), &[], None),
    )?;
    let valid = prepare_reply(&mut s, r, &"x".repeat(16384))?;
    assert!(matches!(
        prepare_reply(&mut s, r, &"я".repeat(8193)),
        Err(StoreError::TextTooLong)
    ));
    assert!(s.begin_assistance(valid.request_id).is_err());
    assert!(matches!(
        prepare_reply(&mut s, r, " \n"),
        Err(StoreError::InvalidInput)
    ));
    for n in 0..32 {
        let _ = prepare_reply(&mut s, r, "edited draft")?;
        let p = prepare_reply(&mut s, r, &format!("pair {n}"))?;
        r = run(
            &mut s,
            p.request_id,
            answer(AssistanceKind::Clarification, Some("Next"), &[], None),
        )?;
    }
    assert!(matches!(
        prepare_reply(&mut s, r, "33rd"),
        Err(StoreError::ResourceLimit)
    ));
    assert_eq!(s.check_summary()?.attempts, 34);
    assert_eq!(s.task(t.id)?.revision, 1);
    assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    s.clear_assistance();
    let p = prepare_new(&mut s, t.id, t.revision, &[])?;
    let mut r = run(
        &mut s,
        p.request_id,
        answer(
            AssistanceKind::Clarification,
            Some(&"Q".repeat(16384)),
            &[],
            None,
        ),
    )?;
    let mut limit = false;
    for _ in 0..32 {
        match prepare_reply(&mut s, r, &"R".repeat(16384)) {
            Ok(p) => {
                r = run(
                    &mut s,
                    p.request_id,
                    answer(
                        AssistanceKind::Clarification,
                        Some(&"Q".repeat(16384)),
                        &[],
                        None,
                    ),
                )?;
            }
            Err(StoreError::TextTooLong) => {
                limit = true;
                break;
            }
            other => return Err(format!("unexpected {other:?}").into()),
        }
    }
    assert!(limit);
    assert_eq!(s.copy_proposal(r)?.len(), 16384);
    Ok(())
}

#[test]
fn reply_stale_parent_epoch_and_exact_suppression_never_retarget()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let first = s.create_task("first")?;
    let other = s.create_task("other")?;
    let p = prepare_new(&mut s, first.id, 1, &[])?;
    let r = run(
        &mut s,
        p.request_id,
        answer(AssistanceKind::Clarification, Some("Q"), &[], None),
    )?;
    let pending = prepare_reply(&mut s, r, "answer")?;
    s.set_model_session(43, true, true)?;
    assert!(s.begin_assistance(pending.request_id).is_err());
    assert!(s.copy_proposal(r).is_err());
    ready(&mut s)?;
    let p = prepare_new(&mut s, first.id, 1, &[])?;
    let r = run(
        &mut s,
        p.request_id,
        answer(AssistanceKind::Clarification, Some("Q2"), &[], None),
    )?;
    suppress(&mut s, r)?;
    assert!(s.task(first.id)?.initiative_suppressed);
    assert!(!s.task(other.id)?.initiative_suppressed);
    assert!(prepare_reply(&mut s, r, "answer").is_err());
    assert!(suppress(&mut s, r).is_err());
    Ok(())
}
