use ambient_core::*;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-plan-{}-{}.sqlite3",
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
#[test]
fn new_database_uses_schema_sixteen() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let _store = Store::open(&db.0)?;
    let c = Connection::open(&db.0)?;
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))?,
        16
    );
    Ok(())
}
#[test]
fn schema_two_is_read_only_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let c = Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-two.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO tasks(revision,text) VALUES(1,'keep'); PRAGMA user_version=2; PRAGMA application_id=1095582257;")?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    Ok(())
}
fn revision(s: &Store, id: TaskId) -> Result<u64, StoreError> {
    Ok(s.task(id)?.revision)
}
fn step_input(text: &str) -> StepInput {
    StepInput {
        description: text.into(),
        prompt: None,
        reference: None,
    }
}
#[test]
fn dag_readiness_completion_and_invalidation_are_manual() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Affair")?;
    assert_eq!(t.plan_completeness, Completeness::Unknown);
    let a = s.add_step(
        t.id,
        1,
        &StepInput {
            description: "A".into(),
            prompt: Some(" e\u{301}\0\n".into()),
            reference: Some("custom:🌿/exact".into()),
        },
    )?;
    let b = s.add_step(t.id, revision(&s, t.id)?, &step_input("B"))?;
    let c = s.add_step(t.id, revision(&s, t.id)?, &step_input("C"))?;
    let check = s.add_check(t.id, revision(&s, t.id)?, a, "Build passes")?;
    s.add_dependency(t.id, revision(&s, t.id)?, a, b)?;
    let before = s.task(t.id)?;
    assert_eq!(
        s.add_dependency(t.id, before.revision, b, a),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(s.task(t.id)?, before);
    assert!(s.steps(t.id, 0, 50)?.items.iter().all(|x| !x.ready));
    s.set_plan_completeness(t.id, revision(&s, t.id)?, Completeness::Complete)?;
    let p = s.steps(t.id, 0, 50)?;
    assert_eq!(p.task_revision, Some(revision(&s, t.id)?));
    assert_eq!(
        p.items
            .iter()
            .filter(|x| x.ready)
            .map(|x| x.id)
            .collect::<Vec<_>>(),
        vec![a, c]
    );
    assert_eq!(p.items[0].prompt.as_deref(), Some(" e\u{301}\0\n"));
    assert_eq!(
        s.set_step_completed(t.id, revision(&s, t.id)?, a, Some(10)),
        Err(StoreError::InvalidInput)
    );
    s.set_check_result(
        t.id,
        revision(&s, t.id)?,
        check,
        CheckResult::Passed,
        Some(11),
    )?;
    s.set_step_completed(t.id, revision(&s, t.id)?, a, Some(12))?;
    s.set_step_completed(t.id, revision(&s, t.id)?, b, Some(13))?;
    s.set_check_result(
        t.id,
        revision(&s, t.id)?,
        check,
        CheckResult::Passed,
        Some(14),
    )?;
    assert_eq!(s.steps(t.id, 0, 50)?.items[1].completed_at_utc, Some(13));
    s.set_check_result(
        t.id,
        revision(&s, t.id)?,
        check,
        CheckResult::Failed,
        Some(15),
    )?;
    assert!(
        s.steps(t.id, 0, 50)?
            .items
            .iter()
            .all(|x| x.completed_at_utc.is_none())
    );
    assert_eq!(
        s.checks(t.id, a, 0, 50)?.items[0].result,
        CheckResult::Failed
    );
    assert_eq!(s.task(t.id)?.lifecycle, TaskLifecycle::Open);
    s.set_check_result(
        t.id,
        revision(&s, t.id)?,
        check,
        CheckResult::Passed,
        Some(16),
    )?;
    s.edit_step(t.id, revision(&s, t.id)?, a, &step_input("A changed"))?;
    assert_eq!(s.task(t.id)?.plan_completeness, Completeness::Partial);
    assert_eq!(
        s.checks(t.id, a, 0, 50)?.items[0].result,
        CheckResult::Unknown
    );
    assert_eq!(
        s.delete_step(t.id, revision(&s, t.id)?, a),
        Err(StoreError::InvalidInput)
    );
    // Explicit manual completion remains possible with a partial plan.
    s.set_check_result(
        t.id,
        revision(&s, t.id)?,
        check,
        CheckResult::Passed,
        Some(17),
    )?;
    s.set_step_completed(t.id, revision(&s, t.id)?, a, Some(18))?;
    s.set_step_completed(t.id, revision(&s, t.id)?, b, Some(19))?;
    s.edit_task(t.id, revision(&s, t.id)?, "Changed intention")?;
    assert!(
        s.steps(t.id, 0, 50)?
            .items
            .iter()
            .all(|x| x.completed_at_utc.is_none())
    );
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(s.task(t.id)?.plan_completeness, Completeness::Partial);
    Ok(())
}
#[test]
fn completion_preserves_only_selected_links_and_reopen_keeps_forgotten_history()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A details")?;
    let b = s.create_task("B")?;
    let c = s.create_task("C")?;
    let key = SourceKey {
        namespace: "adapter".into(),
        scope: "scope".into(),
        object_id: "exact".into(),
    };
    let source = SourceEvidence {
        text: "raw secret".into(),
        observed_at_utc: None,
        completeness: Completeness::Partial,
        kind: AssertionKind::SourceFact,
    };
    s.bind_source(a.id, 1, &key, None, &source, 1)?;
    s.set_link(a.id, revision(&s, a.id)?, b.id, 1, true, 2)?;
    s.set_link(a.id, revision(&s, a.id)?, c.id, 1, true, 3)?;
    s.add_step(a.id, revision(&s, a.id)?, &step_input("old plan"))?;
    s.set_initiative_suppressed(a.id, revision(&s, a.id)?, true)?;
    let impact = s.completion_impact(a.id, &[b.id], 0, 50)?;
    assert_eq!(
        impact.items.iter().map(|x| x.id).collect::<Vec<_>>(),
        vec![c.id]
    );
    let before = s.task(a.id)?;
    let b_before = s.task(b.id)?;
    assert_eq!(
        s.complete_task(
            a.id,
            before.revision,
            "Summary",
            &[b.id],
            &["exact:🌿".into()],
            &[]
        ),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(s.task(a.id)?, before);
    let versions = impact
        .items
        .iter()
        .map(|t| TaskVersion {
            task_id: t.id,
            revision: t.revision,
        })
        .collect::<Vec<_>>();
    s.complete_task(
        a.id,
        before.revision,
        " Summary\0🌿 ",
        &[b.id],
        &["exact:🌿".into()],
        &versions,
    )?;
    assert_eq!(s.task(b.id)?, b_before);
    assert_eq!(s.task(c.id)?.revision, 3);
    assert_eq!(s.task(a.id)?.lifecycle, TaskLifecycle::Completed);
    assert!(s.task(a.id)?.initiative_suppressed);
    assert!(s.evidence(a.id, 0, 50)?.items.is_empty());
    assert!(s.steps(a.id, 0, 50)?.items.is_empty());
    assert_eq!(s.links(a.id, 0, 50)?.items.len(), 1);
    assert_eq!(s.references(a.id, 0, 50)?.items[0].text, "exact:🌿");
    let r = revision(&s, a.id)?;
    assert_eq!(
        s.add_note(a.id, r, "late", 4),
        Err(StoreError::TaskCompleted)
    );
    assert_eq!(
        s.ingest_source(a.id, r, &key, None, &source),
        Err(StoreError::TaskCompleted)
    );
    assert_eq!(
        s.bind_source(a.id, r, &key, None, &source, 4),
        Err(StoreError::TaskCompleted)
    );
    assert_eq!(
        s.add_step(a.id, r, &step_input("late")),
        Err(StoreError::TaskCompleted)
    );
    let incoming = s.add_note(c.id, 3, "Keep in C", 5)?;
    assert_eq!(
        s.move_evidence(incoming.id, c.id, 4, a.id, r, 6),
        Err(StoreError::TaskCompleted)
    );
    assert_eq!(s.task(c.id)?.revision, 4);
    assert_eq!(s.evidence(c.id, 0, 50)?.items[0].text, "Keep in C");
    assert_eq!(
        s.set_plan_completeness(a.id, r, Completeness::Complete),
        Err(StoreError::TaskCompleted)
    );
    s.edit_task(a.id, r, "Corrected summary")?;
    s.reopen_task(a.id, revision(&s, a.id)?)?;
    assert!(s.task(a.id)?.initiative_suppressed);
    assert_eq!(s.task(a.id)?.text, "Corrected summary");
    assert_eq!(s.task(a.id)?.plan_completeness, Completeness::Unknown);
    assert_eq!(
        s.ingest_source(a.id, revision(&s, a.id)?, &key, None, &source)?,
        IngestOutcome::Forgotten
    );
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert!(s.evidence(a.id, 0, 50)?.items.is_empty());
    assert_eq!(s.references(a.id, 0, 50)?.items.len(), 1);
    s.delete_task(
        a.id,
        revision(&s, a.id)?,
        &[TaskVersion {
            task_id: b.id,
            revision: b_before.revision,
        }],
    )?;
    let raw = Connection::open(&db.0)?;
    assert_eq!(
        raw.query_row(
            "SELECT count(*) FROM useful_references WHERE text IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    Ok(())
}
#[test]
fn limits_terminal_ids_and_cross_affair_edges_preserve_originals()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Limits")?;
    let foreign = s.create_task("Other")?;
    let other = s.add_step(foreign.id, 1, &step_input("foreign"))?;
    let a = s.add_step(t.id, 1, &step_input("A"))?;
    assert_eq!(
        s.add_dependency(t.id, revision(&s, t.id)?, a, other),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        s.add_dependency(t.id, revision(&s, t.id)?, a, StepId(9999)),
        Err(StoreError::NotFound)
    );
    assert_eq!(
        s.add_dependency(t.id, revision(&s, t.id)?, a, a),
        Err(StoreError::InvalidInput)
    );
    for _ in 0..8 {
        s.add_check(t.id, revision(&s, t.id)?, a, "check")?;
    }
    assert_eq!(
        s.add_check(t.id, revision(&s, t.id)?, a, "ninth"),
        Err(StoreError::ResourceLimit)
    );
    for _ in 1..64 {
        s.add_step(t.id, revision(&s, t.id)?, &step_input("step"))?;
    }
    let before = s.task(t.id)?;
    assert_eq!(
        s.add_step(t.id, before.revision, &step_input("65th")),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(
        s.edit_step(
            t.id,
            before.revision,
            a,
            &StepInput {
                description: "A".into(),
                prompt: Some(String::new()),
                reference: None
            }
        ),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        s.edit_step(
            t.id,
            before.revision,
            a,
            &StepInput {
                description: "A".into(),
                prompt: None,
                reference: Some("nul\0".into())
            }
        ),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(s.task(t.id)?, before);
    s.delete_step(t.id, before.revision, a)?;
    assert_eq!(
        s.edit_step(t.id, revision(&s, t.id)?, a, &step_input("revive")),
        Err(StoreError::Deleted)
    );
    let last = s.add_step(t.id, revision(&s, t.id)?, &step_input("new"))?;
    assert!(last.0 > a.0);
    let p = s.steps(t.id, 0, 50)?;
    assert_eq!(p.items.len(), 50);
    assert!(p.next_after.is_some());
    assert_eq!(
        s.steps(t.id, p.next_after.unwrap_or(0), 50)?.items.len(),
        14
    );
    Ok(())
}
#[test]
fn completion_fences_foreign_evidence_decisions_and_rolls_back_capacity_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task("B")?;
    let note = s.add_note(a.id, 1, "Note", 1)?;
    s.move_evidence(note.id, a.id, 2, b.id, 1, 2)?;
    let impact = s.completion_impact(b.id, &[], 0, 50)?;
    assert_eq!(impact.items[0].id, a.id);
    let stale = TaskVersion {
        task_id: a.id,
        revision: 2,
    };
    let before = s.task(b.id)?;
    assert_eq!(
        s.complete_task(b.id, before.revision, "Done", &[], &[], &[stale]),
        Err(StoreError::StaleRevision)
    );
    let raw = Connection::open(&db.0)?;
    raw.execute(
        "INSERT INTO useful_references(id,task_id,text) VALUES(9223372036854775807,?1,NULL)",
        [b.id.0],
    )?;
    let v = TaskVersion {
        task_id: a.id,
        revision: 3,
    };
    assert_eq!(
        s.complete_task(b.id, before.revision, "Done", &[], &["ref".into()], &[v]),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(s.task(b.id)?, before);
    assert_eq!(s.task(a.id)?.revision, 3);
    assert_eq!(s.evidence(b.id, 0, 50)?.items[0].text, "Note");
    s.complete_task(b.id, before.revision, "Done", &[], &[], &[v])?;
    assert_eq!(s.task(a.id)?.revision, 4);
    Ok(())
}
#[test]
fn every_owned_page_reports_revision_even_when_empty() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Snapshot")?;
    assert_eq!(s.evidence(t.id, 0, 50)?.task_revision, Some(1));
    assert_eq!(s.sources(t.id, 0, 50)?.task_revision, Some(1));
    assert_eq!(s.links(t.id, 0, 50)?.task_revision, Some(1));
    assert_eq!(s.decisions(t.id, 0, 50)?.task_revision, Some(1));
    assert_eq!(s.deletion_impact(t.id, None, 0, 50)?.task_revision, Some(1));
    Ok(())
}
#[test]
fn deleted_dependencies_keep_identity_without_old_graph_content()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Affair")?;
    let a = s.add_step(t.id, 1, &step_input("A"))?;
    let b = s.add_step(t.id, 2, &step_input("B"))?;
    let edge = s.add_dependency(t.id, 3, a, b)?;
    s.delete_dependency(t.id, 4, edge)?;
    let c = Connection::open(&db.0)?;
    assert!(c.query_row(
        "SELECT predecessor IS NULL AND dependent IS NULL FROM plan_dependencies WHERE id=?1",
        [edge.0],
        |r| r.get::<_, bool>(0)
    )?);
    assert_eq!(s.delete_dependency(t.id, 5, edge), Err(StoreError::Deleted));
    Ok(())
}
#[test]
fn null_check_result_is_corrupt_not_an_unknown_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Affair")?;
    let step = s.add_step(t.id, 1, &step_input("A"))?;
    s.add_check(t.id, 2, step, "Check")?;
    drop(s);
    let c = Connection::open(&db.0)?;
    c.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE plan_checks SET result=NULL;")?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    Ok(())
}
#[test]
fn long_exact_plan_pages_include_record_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Affair")?;
    let input = StepInput {
        description: "d".repeat(16384),
        prompt: Some("🌿".repeat(4096)),
        reference: Some("r".repeat(2048)),
    };
    for _ in 0..12 {
        s.add_step(t.id, revision(&s, t.id)?, &input)?;
    }
    let p = s.steps(t.id, 0, 50)?;
    assert_eq!(p.items.len(), 7);
    assert!(p.payload_bytes <= 262144);
    assert_eq!(s.steps(t.id, p.next_after.unwrap_or(0), 50)?.items.len(), 5);
    let bad = StepInput {
        description: "d".into(),
        prompt: Some("x".repeat(16385)),
        reference: None,
    };
    assert_eq!(
        s.add_step(t.id, revision(&s, t.id)?, &bad),
        Err(StoreError::TextTooLong)
    );
    assert_eq!(
        s.complete_task(
            t.id,
            revision(&s, t.id)?,
            "Summary",
            &[],
            &vec!["r".into(); 33],
            &[]
        ),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(
        s.complete_task(
            t.id,
            revision(&s, t.id)?,
            "Summary",
            &[],
            &["r".repeat(2049)],
            &[]
        ),
        Err(StoreError::TextTooLong)
    );
    Ok(())
}
#[test]
fn completion_checks_final_bytes_and_failure_keeps_all_details()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task(&"a".repeat(16384))?;
    let b = s.create_task(&"b".repeat(16384))?;
    let c = Connection::open(&db.0)?;
    c.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4093) INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) SELECT ?1,?2,0,1,2,2 FROM n",rusqlite::params![b.id.0,"x".repeat(16384)])?;
    s.add_note(a.id, 1, &"n".repeat(16384), 1)?;
    assert_eq!(
        s.add_step(a.id, 2, &step_input("too much")),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(
        s.complete_task(
            a.id,
            2,
            &"s".repeat(16384),
            &[],
            &vec!["r".repeat(2048); 32],
            &[]
        ),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(s.task(a.id)?.text, "a".repeat(16384));
    assert_eq!(s.task(a.id)?.revision, 2);
    assert_eq!(s.evidence(a.id, 0, 50)?.items[0].text, "n".repeat(16384));
    s.complete_task(a.id, 2, &"s".repeat(16384), &[], &["r".repeat(2048)], &[])?;
    assert_eq!(s.references(a.id, 0, 50)?.items.len(), 1);
    assert!(s.evidence(a.id, 0, 50)?.items.is_empty());
    Ok(())
}
#[test]
fn graph_edge_cap_and_deep_cycle_are_bounded() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Graph")?;
    for i in 0..64 {
        s.add_step(t.id, revision(&s, t.id)?, &step_input(&format!("Step {i}")))?;
    }
    let c = Connection::open(&db.0)?;
    c.execute_batch("INSERT INTO plan_dependencies(task_id,predecessor,dependent) SELECT 1,id,id+1 FROM plan_steps WHERE id<64;")?;
    assert_eq!(
        s.add_dependency(t.id, 65, StepId(64), StepId(1)),
        Err(StoreError::InvalidInput)
    );
    c.execute_batch("INSERT INTO plan_dependencies(task_id,predecessor,dependent) SELECT 1,a.id,b.id FROM plan_steps a CROSS JOIN plan_steps b WHERE a.id+1<b.id ORDER BY a.id,b.id LIMIT 449;")?;
    assert_eq!(s.dependencies(t.id, 0, 50)?.items.len(), 50);
    assert_eq!(
        s.add_dependency(t.id, 65, StepId(63), StepId(64)),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        s.add_dependency(t.id, 65, StepId(62), StepId(64)),
        Err(StoreError::ResourceLimit)
    );
    drop(c);
    drop(s);
    let _s = Store::open(&db.0)?;
    Ok(())
}
#[test]
fn every_new_identity_cap_still_allows_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    for table in [
        "plan_steps",
        "plan_checks",
        "plan_dependencies",
        "useful_references",
    ] {
        let db = Database::new();
        let mut s = Store::open(&db.0)?;
        let t = s.create_task("Affair")?;
        let a = s.add_step(t.id, 1, &step_input("A"))?;
        let b = s.add_step(t.id, 2, &step_input("B"))?;
        let c = Connection::open(&db.0)?;
        match table {
            "plan_steps" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(3) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO plan_steps(id,task_id) SELECT x,1 FROM n")?;
                assert_eq!(
                    s.add_step(t.id, 3, &step_input("blocked")),
                    Err(StoreError::ResourceLimit)
                );
            }
            "plan_checks" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO plan_checks(id,task_id,step_id) SELECT x,1,1 FROM n")?;
                assert_eq!(
                    s.add_check(t.id, 3, a, "blocked"),
                    Err(StoreError::ResourceLimit)
                );
            }
            "plan_dependencies" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO plan_dependencies(id,task_id) SELECT x,1 FROM n")?;
                assert_eq!(
                    s.add_dependency(t.id, 3, a, b),
                    Err(StoreError::ResourceLimit)
                );
            }
            "useful_references" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO useful_references(id,task_id) SELECT x,1 FROM n")?;
                assert_eq!(
                    s.complete_task(t.id, 3, "Done", &[], &["r".into()], &[]),
                    Err(StoreError::ResourceLimit)
                );
            }
            _ => unreachable!(),
        }
        s.complete_task(t.id, 3, "Done", &[], &[], &[])?;
        s.delete_task(t.id, 4, &[])?;
        assert_eq!(s.task(t.id), Err(StoreError::Deleted));
    }
    Ok(())
}

#[test]
fn confirmations_survive_restart_and_upstream_edits_reset_descendant_checks()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Affair")?;
    s.set_plan_completeness(t.id, 1, Completeness::Complete)?;
    assert!(s.steps(t.id, 0, 50)?.items.is_empty());
    assert_eq!(s.task(t.id)?.lifecycle, TaskLifecycle::Open);
    let a = s.add_step(t.id, 2, &step_input("A"))?;
    let b = s.add_step(t.id, 3, &step_input("B"))?;
    let check = s.add_check(t.id, 4, b, "Child check")?;
    s.add_dependency(t.id, 5, a, b)?;
    s.set_plan_completeness(t.id, 6, Completeness::Complete)?;
    s.set_step_completed(t.id, 7, a, Some(10))?;
    s.set_check_result(t.id, 8, check, CheckResult::Passed, Some(11))?;
    s.set_step_completed(t.id, 9, b, Some(12))?;
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert_eq!(s.steps(t.id, 0, 50)?.items[1].completed_at_utc, Some(12));
    assert_eq!(
        s.checks(t.id, b, 0, 50)?.items[0].confirmed_at_utc,
        Some(11)
    );
    assert_eq!(s.task(t.id)?.lifecycle, TaskLifecycle::Open);
    s.edit_step(t.id, 10, a, &step_input("Changed upstream"))?;
    assert!(
        s.steps(t.id, 0, 50)?
            .items
            .iter()
            .all(|step| step.completed_at_utc.is_none())
    );
    assert_eq!(
        s.checks(t.id, b, 0, 50)?.items[0].result,
        CheckResult::Unknown
    );
    assert_eq!(s.checks(t.id, b, 0, 50)?.items[0].confirmed_at_utc, None);
    Ok(())
}
