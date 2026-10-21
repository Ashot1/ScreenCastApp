use ambient_core::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-proposals-{}-{}.sqlite3",
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
    let f = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(d.id(), high())?;
    s.release_check(&f);
    Ok(())
}
fn new_context(
    s: &mut Store,
    t: TaskVersion,
    notes: Vec<EvidenceId>,
) -> Result<AssistancePreview, StoreError> {
    let revision = s.assistant_workspace_revision(t.task_id)?;
    s.prepare_assistance(AssistanceRequest {
        task: t,
        workspace_revision: revision,
        mode: AssistanceMode::NewContext,
        selected_evidence: notes,
        reply: None,
    })
}
fn continue_with(
    s: &mut Store,
    t: TaskVersion,
    reply: Option<ProposalReply>,
) -> Result<AssistancePreview, StoreError> {
    let revision = s.assistant_workspace_revision(t.task_id)?;
    s.prepare_assistance(AssistanceRequest {
        task: t,
        workspace_revision: revision,
        mode: AssistanceMode::Continue,
        selected_evidence: vec![],
        reply,
    })
}
fn proposal(
    t: TaskVersion,
    id: Option<ProposalId>,
    kind: AssistanceKind,
    text: &str,
) -> ProposalAnswer {
    ProposalAnswer {
        proposal_id: id,
        kind,
        message: text.into(),
        rationale: "Saved grounds".into(),
        evidence_ids: vec![format!("task:{}", t.task_id.0)],
        step_id: None,
    }
}
fn run(
    s: &mut Store,
    p: &AssistancePreview,
    proposals: Vec<ProposalAnswer>,
) -> Result<Option<WorkspaceVersion>, StoreError> {
    let a = s.begin_assistance(p.request_id)?;
    let f = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    assert_eq!(d.context(), Some(p.context.as_str()));
    let result = s.finish_assistance(
        d.id(),
        high(),
        Some(AssistanceAnswer {
            proposals,
            rationale: "Full set".into(),
        }),
    );
    s.release_check(&f);
    result
}
fn version(t: &Task) -> TaskVersion {
    TaskVersion {
        task_id: t.id,
        revision: t.revision,
    }
}

#[test]
fn complete_set_retains_identity_fresh_visibility_and_stable_order_after_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("release")?);
    let p = new_context(&mut s, t, vec![])?;
    let bytes = std::fs::read(&db.0)?;
    assert_eq!(s.assistance_preview(p.request_id)?, p);
    assert_eq!(bytes, std::fs::read(&db.0)?);
    assert_eq!(s.check_summary()?.attempts, 1);
    run(
        &mut s,
        &p,
        vec![
            proposal(t, None, AssistanceKind::Suggestion, "first"),
            proposal(t, None, AssistanceKind::Clarification, "question"),
        ],
    )?;
    let first = s.proposal_feed()?.proposals;
    assert_eq!(first.len(), 2);
    assert_ne!(first[0].id, first[1].id);
    let p = continue_with(&mut s, t, None)?;
    let v: serde_json::Value = serde_json::from_str(&p.context)?;
    assert_eq!(v["proposals"].as_array().ok_or("proposals")?.len(), 2);
    s.set_proposal_visibility(first[0].target(), false)?;
    run(
        &mut s,
        &p,
        vec![
            proposal(
                t,
                Some(first[1].id),
                AssistanceKind::Clarification,
                "question",
            ),
            proposal(
                t,
                Some(first[0].id),
                AssistanceKind::Suggestion,
                "rephrased",
            ),
            proposal(t, None, AssistanceKind::Suggestion, "third"),
        ],
    )?;
    let rows = s.proposal_feed()?.proposals;
    assert_eq!(rows[0].id, first[0].id);
    assert!(!rows[0].visible);
    assert_eq!(rows[0].content_revision, 2);
    assert_eq!(rows[1].content_revision, 1);
    assert_eq!(s.task(t.task_id)?.revision, t.revision);
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert_eq!(s.proposal_feed()?.proposals, rows);
    assert!(s.copy_proposal(rows[1].target()).is_err());
    ready(&mut s)?;
    assert_eq!(s.copy_proposal(rows[0].target())?, "rephrased");
    Ok(())
}

#[test]
fn pair_identity_and_historical_attribution_survive_multiple_restarts()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("intent")?);
    let p = new_context(&mut s, t, vec![])?;
    run(
        &mut s,
        &p,
        vec![proposal(t, None, AssistanceKind::Clarification, "Premise?")],
    )?;
    let mut ids = vec![];
    for n in 0..3 {
        let row = s.proposal_feed()?.proposals.remove(0);
        let before = s.assistant_workspace_revision(t.task_id)?;
        let p = continue_with(
            &mut s,
            t,
            Some(ProposalReply {
                target: row.target(),
                text: format!("reply {n}"),
            }),
        )?;
        let v: serde_json::Value = serde_json::from_str(&p.context)?;
        let id = v["replies"][n]["evidence_id"]
            .as_str()
            .ok_or("reply id")?
            .to_owned();
        assert_eq!(id, format!("reply:{}:{before}:{n}", t.task_id.0));
        ids.push(id.clone());
        let mut next = proposal(t, None, AssistanceKind::Clarification, "Next?");
        next.evidence_ids = vec![id];
        run(&mut s, &p, vec![next])?;
        drop(s);
        s = Store::open(&db.0)?;
        ready(&mut s)?;
        let row = s.proposal_feed()?.proposals.remove(0);
        let details = s.proposal_details(row.target())?;
        assert_eq!(
            details.citations[0].model_question.as_deref(),
            Some(if n == 0 { "Premise?" } else { "Next?" })
        );
    }
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        3
    );
    assert!(s.evidence(t.task_id, 0, 50)?.items.is_empty());
    Ok(())
}

#[test]
fn clear_and_empty_publication_never_reuse_revisions_or_proposal_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("intent")?);
    let p = new_context(&mut s, t, vec![])?;
    run(
        &mut s,
        &p,
        vec![proposal(t, None, AssistanceKind::Suggestion, "old")],
    )?;
    let old = s.proposal_feed()?.proposals.remove(0);
    let late = continue_with(&mut s, t, None)?;
    let r = s.assistant_workspace_revision(t.task_id)?;
    s.clear_discussion(t, r)?;
    assert_eq!(s.assistant_workspace_revision(t.task_id)?, r + 1);
    assert!(s.begin_assistance(late.request_id).is_err());
    let p = new_context(&mut s, t, vec![])?;
    run(&mut s, &p, vec![])?;
    assert_eq!(s.assistant_workspace_revision(t.task_id)?, r + 2);
    assert!(s.proposal_feed()?.proposals.is_empty());
    let p = new_context(&mut s, t, vec![])?;
    run(
        &mut s,
        &p,
        vec![proposal(t, None, AssistanceKind::Suggestion, "new")],
    )?;
    assert!(s.proposal_feed()?.proposals[0].id.get() > old.id.get());
    Ok(())
}

#[test]
fn invalid_complete_set_rolls_back_and_new_context_failure_preserves_discussion()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("intent")?);
    let p = new_context(&mut s, t, vec![])?;
    run(
        &mut s,
        &p,
        vec![proposal(t, None, AssistanceKind::Clarification, "Q")],
    )?;
    let rows = s.proposal_feed()?.proposals;
    s.set_proposal_visibility(rows[0].target(), false)?;
    let old = s.proposal_feed()?;
    for duplicate in [false, true] {
        let p = continue_with(&mut s, t, None)?;
        let id = if duplicate {
            rows[0].id
        } else {
            ProposalId::from_raw(99999)?
        };
        let bad = proposal(t, Some(id), AssistanceKind::Suggestion, "bad");
        assert!(run(&mut s, &p, vec![bad.clone(), bad]).is_err());
        assert_eq!(s.proposal_feed()?, old);
        ready(&mut s)?;
    }
    let p = new_context(&mut s, t, vec![])?;
    assert!(!p.context.contains("\"replies\""));
    assert!(!p.context.contains("\"proposal_id\""));
    assert!(
        run(
            &mut s,
            &p,
            vec![proposal(
                t,
                Some(rows[0].id),
                AssistanceKind::Suggestion,
                "bad"
            )]
        )
        .is_err()
    );
    assert_eq!(s.proposal_feed()?, old);
    assert_eq!(s.check_summary()?.total, 77);
    Ok(())
}

#[test]
fn ordinary_edit_is_stale_but_forgetting_uncited_ground_clears_entire_workspace()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let task = s.create_task("intent")?;
    let note = s.add_note(task.id, 1, "uncited secret", 1)?;
    let t = version(&s.task(task.id)?);
    let p = new_context(&mut s, t, vec![note.id])?;
    run(
        &mut s,
        &p,
        vec![proposal(t, None, AssistanceKind::Suggestion, "derived")],
    )?;
    let row = s.proposal_feed()?.proposals.remove(0);
    s.edit_task(t.task_id, t.revision, "changed")?;
    let details = s.proposal_details(row.target())?;
    assert!(details.proposal.stale);
    assert!(details.citations.is_empty());
    assert!(s.copy_proposal(row.target()).is_err());
    let t = version(&s.task(t.task_id)?);
    s.delete_evidence(note.id, t.task_id, t.revision, &[])?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    drop(s);
    let s = Store::open(&db.0)?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    Ok(())
}

#[test]
fn expanded_details_overflow_rejects_publication_after_accounting()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let task = s.create_task("x".repeat(12000).as_str())?;
    let mut notes = vec![];
    for _ in 0..14 {
        let t = s.task(task.id)?;
        notes.push(s.add_note(t.id, t.revision, &"a".repeat(16384), 1)?.id);
    }
    let t = version(&s.task(task.id)?);
    let p = new_context(&mut s, t, notes.clone())?;
    let mut a = proposal(t, None, AssistanceKind::Suggestion, &"m".repeat(16384));
    a.rationale = "r".repeat(8192);
    a.evidence_ids
        .extend(notes.iter().map(|id| format!("note:{}", id.0)));
    let before = s.assistant_workspace_revision(t.task_id)?;
    assert!(run(&mut s, &p, vec![a]).is_err());
    assert!(s.proposal_feed()?.proposals.is_empty());
    assert_eq!(s.assistant_workspace_revision(t.task_id)?, before);
    assert_eq!(s.check_summary()?.total, 22);
    Ok(())
}

#[test]
fn open_rejects_a_stale_proposal_citing_an_unselected_note_without_writing()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let task = s.create_task("intent")?;
    let selected = s.add_note(task.id, 1, "selected", 1)?;
    let task = s.task(task.id)?;
    let other = s.add_note(task.id, task.revision, "never selected", 1)?;
    let t = version(&s.task(task.id)?);
    let p = new_context(&mut s, t, vec![selected.id])?;
    let mut a = proposal(t, None, AssistanceKind::Suggestion, "derived");
    a.evidence_ids = vec![format!("note:{}", selected.id.0)];
    run(&mut s, &p, vec![a])?;
    s.edit_task(t.task_id, t.revision, "changed")?;
    drop(s);
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute(
        "UPDATE assistant_proposals SET citations=?1",
        [format!("[{{\"kind\":\"note\",\"id\":{}}}]", other.id.0)],
    )?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    Ok(())
}

fn fill_fixture_retained_limit(
    path: &std::path::Path,
    owner: TaskId,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = rusqlite::Connection::open(path)?;
    let mut used = 0usize;
    for (table, columns) in [
        ("tasks", vec!["text"]),
        ("assistant_workspaces", vec!["selected", "sources"]),
        (
            "assistant_proposals",
            vec!["message", "rationale", "citations"],
        ),
    ] {
        for column in columns {
            let mut q = c.prepare(&format!(
                "SELECT {column} FROM {table} WHERE {column} IS NOT NULL"
            ))?;
            let mut rows = q.query([])?;
            while let Some(row) = rows.next()? {
                used += row.get::<_, String>(0)?.len();
            }
        }
    }
    let tx = c.transaction()?;
    let mut remaining = MAX_RETAINED_BYTES as usize - used;
    while remaining > 0 {
        let size = remaining.min(MAX_TEXT_BYTES);
        tx.execute("INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,0,1,2,2)",rusqlite::params![owner.0,"x".repeat(size)])?;
        remaining -= size;
    }
    tx.commit()?;
    Ok(())
}
#[test]
fn full_set_and_visibility_cannot_escape_global_count_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    for n in 0..8 {
        let t = version(&s.create_task(&format!("intent {n}"))?);
        let p = new_context(&mut s, t, vec![])?;
        run(
            &mut s,
            &p,
            (0..16)
                .map(|i| proposal(t, None, AssistanceKind::Suggestion, &format!("action {i}")))
                .collect(),
        )?;
    }
    let before = s.proposal_feed()?;
    assert_eq!(before.proposals.len(), 128);
    s.set_proposal_visibility(before.proposals[0].target(), false)?;
    let t = version(&s.create_task("ninth")?);
    let p = new_context(&mut s, t, vec![])?;
    assert_eq!(
        run(
            &mut s,
            &p,
            vec![proposal(t, None, AssistanceKind::Suggestion, "overflow")]
        )
        .err(),
        Some(StoreError::ResourceLimit)
    );
    assert_eq!(s.proposal_feed()?.proposals.len(), 128);
    assert_eq!(s.check_summary()?.total, 110);
    let task = before.proposals[0].task;
    s.clear_discussion(task, before.proposals[0].workspace_revision)?;
    assert_eq!(s.proposal_feed()?.proposals.len(), 112);
    let p = new_context(&mut s, t, vec![])?;
    assert!(
        run(
            &mut s,
            &p,
            vec![proposal(t, None, AssistanceKind::Suggestion, "now fits")]
        )?
        .is_some()
    );
    Ok(())
}
#[test]
fn shared_retained_limit_rejects_publication_and_oversize_database_without_losing_usage()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("intent")?);
    let p = new_context(&mut s, t, vec![])?;
    run(
        &mut s,
        &p,
        vec![proposal(t, None, AssistanceKind::Suggestion, "old")],
    )?;
    let before = s.proposal_feed()?;
    fill_fixture_retained_limit(&db.0, t.task_id)?;
    let p = new_context(&mut s, t, vec![])?;
    assert_eq!(
        run(
            &mut s,
            &p,
            vec![proposal(
                t,
                None,
                AssistanceKind::Suggestion,
                &"m".repeat(1000)
            )]
        )
        .err(),
        Some(StoreError::ResourceLimit)
    );
    assert_eq!(s.proposal_feed()?, before);
    assert_eq!(s.check_summary()?.total, 33);
    assert_eq!(s.check_summary()?.unknown, 0);
    drop(s);
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute(
        "UPDATE evidence SET text=text||'z' WHERE id=(SELECT max(id) FROM evidence)",
        [],
    )?;
    drop(c);
    let bytes = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, bytes);
    Ok(())
}
#[test]
fn completing_at_capacity_counts_only_the_final_content_after_discussion_erasure()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("intent")?);
    let other = s.create_task("other")?;
    let p = new_context(&mut s, t, vec![])?;
    run(
        &mut s,
        &p,
        vec![proposal(
            t,
            None,
            AssistanceKind::Suggestion,
            &"m".repeat(16000),
        )],
    )?;
    fill_fixture_retained_limit(&db.0, other.id)?;
    s.complete_task(t.task_id, t.revision, &"summary".repeat(100), &[], &[], &[])?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    assert_eq!(s.task(t.task_id)?.lifecycle, TaskLifecycle::Completed);
    assert_eq!(s.assistant_workspace_revision(t.task_id)?, 3);
    Ok(())
}

#[test]
fn open_checks_stored_step_ownership_but_preserves_completed_and_deleted_step_history()
-> Result<(), Box<dyn std::error::Error>> {
    for change in ["foreign", "negative", "completed", "deleted"] {
        let db = Database::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let task = s.create_task("intent")?;
        let step = s.add_step(
            task.id,
            1,
            &StepInput {
                description: "ready".into(),
                prompt: None,
                reference: None,
            },
        )?;
        let task = s.task(task.id)?;
        s.set_plan_completeness(task.id, task.revision, Completeness::Complete)?;
        let t = version(&s.task(task.id)?);
        let other = s.create_task("other")?;
        let foreign = s.add_step(
            other.id,
            1,
            &StepInput {
                description: "foreign".into(),
                prompt: None,
                reference: None,
            },
        )?;
        let p = new_context(&mut s, t, vec![])?;
        let mut a = proposal(t, None, AssistanceKind::Suggestion, "next");
        a.step_id = Some(step);
        run(&mut s, &p, vec![a])?;
        if change == "completed" {
            s.set_step_completed(t.task_id, t.revision, step, Some(123))?;
        }
        if change == "deleted" {
            s.delete_step(t.task_id, t.revision, step)?;
        }
        drop(s);
        if change == "foreign" || change == "negative" {
            let c = rusqlite::Connection::open(&db.0)?;
            c.pragma_update(None, "foreign_keys", false)?;
            c.execute(
                "UPDATE assistant_proposals SET step_id=?1",
                [if change == "foreign" { foreign.0 } else { -1 }],
            )?;
            drop(c);
            let bytes = std::fs::read(&db.0)?;
            assert!(
                matches!(Store::open(&db.0), Err(StoreError::UnsupportedSchema)),
                "{change}"
            );
            assert_eq!(std::fs::read(&db.0)?, bytes);
        } else {
            let s = Store::open(&db.0)?;
            let p = &s.proposal_feed()?.proposals[0];
            assert!(p.stale);
            assert_eq!(p.step_id, Some(step));
        }
    }
    Ok(())
}
#[test]
fn discarding_an_old_preview_cannot_cancel_a_new_preview_for_another_task()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let a = version(&s.create_task("first")?);
    let b = version(&s.create_task("second")?);
    let old = new_context(&mut s, a, vec![])?;
    let new = new_context(&mut s, b, vec![])?;
    s.discard_assistance_preview(old.request_id)?;
    assert_eq!(s.assistance_preview(new.request_id)?, new);
    assert!(
        run(
            &mut s,
            &new,
            vec![proposal(b, None, AssistanceKind::Suggestion, "second only")]
        )?
        .is_some()
    );
    Ok(())
}

fn wire_size(a: &ProposalAnswer) -> usize {
    serde_json::json!({"proposals":[{"proposal_id":a.proposal_id,"kind":a.kind,"message":a.message,"rationale":a.rationale,"evidence_ids":a.evidence_ids,"step_id":a.step_id.map(|s|format!("step:{}",s.0))}],"rationale":"Full set"}).to_string().len()
}
fn full_row(t: TaskVersion, id: u64) -> Result<Proposal, StoreError> {
    Ok(Proposal {
        id: ProposalId::from_raw(id)?,
        task: t,
        workspace_revision: 2,
        content_revision: 1,
        kind: AssistanceKind::Suggestion,
        message: format!("{}{}", "m".repeat(8192), "\\".repeat(8192)),
        rationale: String::new(),
        citations: vec![ProposalCitationRef::Task(t.task_id)],
        step_id: None,
        visible: true,
        stale: false,
    })
}
#[test]
fn exact_feed_framing_rejects_the_complete_128th_row_without_partial_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    for n in 1..=128 {
        let t = version(&s.create_task("intent")?);
        let mut row = full_row(t, n)?;
        row.rationale = "r".repeat(MAX_PROPOSAL_ROW - serde_json::to_string(&row)?.len());
        assert_eq!(serde_json::to_string(&row)?.len(), MAX_PROPOSAL_ROW);
        let mut a = proposal(t, None, AssistanceKind::Suggestion, &row.message);
        a.rationale = row.rationale;
        assert!(wire_size(&a) <= MAX_ASSISTANCE_TEXT);
        let p = new_context(&mut s, t, vec![])?;
        let result = run(&mut s, &p, vec![a]);
        if n < 128 {
            assert!(result?.is_some());
        } else {
            assert_eq!(result.err(), Some(StoreError::ResourceLimit));
            assert_eq!(s.proposal_feed()?.proposals.len(), 127);
            assert_eq!(s.assistant_workspace_revision(t.task_id)?, 1);
        }
    }
    assert_eq!(s.check_summary()?.attempts, 129);
    assert_eq!(s.check_summary()?.unknown, 0);
    Ok(())
}
#[test]
fn exact_row_metadata_can_exceed_the_limit_while_the_model_json_still_fits()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = version(&s.create_task("intent")?);
    let p = new_context(&mut s, t, vec![])?;
    let mut row = full_row(t, 1)?;
    row.rationale = "r".repeat(MAX_PROPOSAL_ROW + 1 - serde_json::to_string(&row)?.len());
    let mut a = proposal(t, None, AssistanceKind::Suggestion, &row.message);
    a.rationale = row.rationale;
    assert!(wire_size(&a) <= MAX_ASSISTANCE_TEXT);
    assert_eq!(
        run(&mut s, &p, vec![a]).err(),
        Some(StoreError::ResourceLimit)
    );
    assert_eq!(s.assistant_workspace_revision(t.task_id)?, 1);
    assert_eq!(s.check_summary()?.total, 22);
    Ok(())
}

#[test]
fn durable_counter_exhaustion_rejects_without_reset_partial_writes_or_lost_usage()
-> Result<(), Box<dyn std::error::Error>> {
    for boundary in ["workspace", "content", "proposal"] {
        let db = Database::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = version(&s.create_task("intent")?);
        let p = new_context(&mut s, t, vec![])?;
        run(
            &mut s,
            &p,
            vec![proposal(t, None, AssistanceKind::Suggestion, "old")],
        )?;
        let c = rusqlite::Connection::open(&db.0)?;
        match boundary {
            "workspace" => {
                c.execute(
                    "UPDATE tasks SET assistant_workspace_revision=?1",
                    [i64::MAX],
                )?;
            }
            "content" => {
                c.execute(
                    "UPDATE assistant_proposals SET content_revision=?1",
                    [i64::MAX],
                )?;
            }
            _ => {
                c.execute(
                    "UPDATE sqlite_sequence SET seq=?1 WHERE name='assistant_proposals'",
                    [i64::MAX],
                )?;
            }
        }
        drop(c);
        let before = s.proposal_feed()?;
        let revision = s.assistant_workspace_revision(t.task_id)?;
        if boundary == "workspace" {
            assert_eq!(
                s.clear_discussion(t, revision).err(),
                Some(StoreError::ResourceLimit)
            );
        }
        let p = if boundary == "content" {
            continue_with(&mut s, t, None)?
        } else {
            new_context(&mut s, t, vec![])?
        };
        let id = if boundary == "content" {
            Some(before.proposals[0].id)
        } else {
            None
        };
        assert_eq!(
            run(
                &mut s,
                &p,
                vec![proposal(t, id, AssistanceKind::Suggestion, "changed")]
            )
            .err(),
            Some(StoreError::ResourceLimit)
        );
        assert_eq!(s.proposal_feed()?, before);
        assert_eq!(s.assistant_workspace_revision(t.task_id)?, revision);
        assert_eq!(s.check_summary()?.total, 33);
        assert_eq!(s.check_summary()?.unknown, 0);
    }
    Ok(())
}
