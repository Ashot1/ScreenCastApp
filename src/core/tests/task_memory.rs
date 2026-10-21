use ambient_core::*;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-tasks-{}-{}.sqlite3",
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
fn key() -> SourceKey {
    SourceKey {
        namespace: "test.adapter".into(),
        scope: "account-a".into(),
        object_id: "exact/Object🌿".into(),
    }
}
fn input(text: &str) -> SourceEvidence {
    SourceEvidence {
        text: text.into(),
        observed_at_utc: None,
        completeness: Completeness::Unknown,
        kind: AssertionKind::SourceFact,
    }
}
#[test]
fn multiple_tasks_exact_edits_revision_fences_pause_and_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("  A🌿\n")?;
    let b = s.create_task("B")?;
    assert_ne!(a.id, b.id);
    assert_eq!(s.tasks(0, 50)?.items.len(), 2);
    let a = s.edit_task(a.id, a.revision, "e\u{301}")?;
    assert_eq!(s.edit_task(a.id, 1, "late"), Err(StoreError::StaleRevision));
    s.set_manual_pause(true)?;
    assert_eq!(s.task(a.id)?.revision, 2);
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert!(s.manual_pause()?);
    assert_eq!(s.task(a.id)?.text.as_bytes(), [101, 204, 129]);
    s.set_manual_pause(false)?;
    s.delete_task(a.id, 2, &[])?;
    assert_eq!(s.task(a.id), Err(StoreError::Deleted));
    assert_eq!(
        s.edit_task(a.id, 3, "resurrection"),
        Err(StoreError::Deleted)
    );
    assert!(s.create_task("new")?.id.0 > b.id.0);
    Ok(())
}
#[test]
fn note_correction_and_symmetric_user_links_fence_both_tasks()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task("B")?;
    let n = s.add_note(a.id, 1, "note\0🌿", 1700000000)?;
    assert_eq!(n.origin, Origin::User);
    assert_eq!(n.kind, AssertionKind::UserConfirmation);
    s.move_evidence(n.id, a.id, 2, b.id, 1, 1700000001)?;
    assert_eq!(s.evidence(a.id, 0, 50)?.items.len(), 0);
    assert_eq!(s.evidence(b.id, 0, 50)?.items[0].text, "note\0🌿");
    assert_eq!(
        s.edit_evidence(n.id, a.id, 2, "late", 1700000002),
        Err(StoreError::StaleRevision)
    );
    assert!(!s.decisions(a.id, 0, 50)?.items[0].accepted);
    assert!(s.decisions(b.id, 0, 50)?.items[0].accepted);
    s.set_link(a.id, 3, b.id, 2, true, 1700000003)?;
    assert_eq!(s.links(b.id, 0, 50)?.items[0].other_task_id, a.id);
    assert_eq!(
        s.set_link(a.id, 3, b.id, 2, false, 1700000004),
        Err(StoreError::StaleRevision)
    );
    s.set_link(a.id, 4, b.id, 3, false, 1700000004)?;
    assert!(!s.links(a.id, 0, 50)?.items[0].related);
    s.delete_task(
        a.id,
        5,
        &[TaskVersion {
            task_id: b.id,
            revision: 4,
        }],
    )?;
    assert_eq!(s.task(b.id)?.revision, 5);
    assert!(s.links(b.id, 0, 50)?.items.is_empty());
    s.delete_evidence(n.id, b.id, 5, &[])?;
    assert!(s.evidence(b.id, 0, 50)?.items.is_empty());
    assert_eq!(
        s.edit_evidence(n.id, b.id, 6, "late", 1),
        Err(StoreError::Deleted)
    );
    Ok(())
}
#[test]
fn forgotten_source_blocks_new_version_but_other_live_binding_updates()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task("B")?;
    let IngestOutcome::Applied(n) =
        s.ingest_source(a.id, 1, &key(), Some("v1"), &input("private source"))?
    else {
        return Err("not applied".into());
    };
    let other = s.bind_source(b.id, 1, &key(), None, &input("independent"), 1)?;
    s.delete_evidence(n, a.id, 2, &[])?;
    assert_eq!(
        s.ingest_source(
            a.id,
            3,
            &key(),
            Some("new-id-and-version"),
            &input("must not return")
        )?,
        IngestOutcome::Forgotten
    );
    let c = s.create_task("C")?;
    assert_eq!(
        s.ingest_source(c.id, 1, &key(), None, &input("must not implicitly bind"))?,
        IngestOutcome::Forgotten
    );
    assert_eq!(
        s.ingest_source(b.id, 2, &key(), Some("v2"), &input("updated independent"))?,
        IngestOutcome::Applied(other)
    );
    let bound = s.bind_source(
        c.id,
        1,
        &key(),
        Some("v3"),
        &input("fresh explicit content"),
        1700000000,
    )?;
    assert_ne!(bound, n);
    assert_eq!(
        s.evidence(c.id, 0, 50)?.items[0].text,
        "fresh explicit content"
    );
    s.edit_evidence(bound, c.id, 2, "user verified text", 1700000001)?;
    assert_eq!(
        s.ingest_source(c.id, 3, &key(), Some("v4"), &input("late guess"))?,
        IngestOutcome::UserProtected
    );
    assert_eq!(s.task(c.id)?.revision, 3);
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(
        s.evidence(b.id, 0, 50)?.items[0].text,
        "updated independent"
    );
    assert_eq!(s.sources(a.id, 0, 50)?.items.len(), 0);
    let con = Connection::open(&db.0)?;
    let old_keys: i64 = con.query_row(
        "SELECT count(*) FROM source_bindings WHERE task_id=?1 AND namespace IS NOT NULL",
        [a.id.0],
        |r| r.get(0),
    )?;
    assert_eq!(old_keys, 0);
    Ok(())
}
#[test]
fn corrected_binding_updates_in_place_without_restoring_rejected_task()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task("B")?;
    let IngestOutcome::Applied(id) = s.ingest_source(a.id, 1, &key(), None, &input("first"))?
    else {
        return Err("not applied".into());
    };
    assert_eq!(
        s.ingest_source(a.id, 2, &key(), Some("v2"), &input("second"))?,
        IngestOutcome::Applied(id)
    );
    s.move_evidence(id, a.id, 3, b.id, 1, 17)?;
    assert_eq!(
        s.ingest_source(a.id, 4, &key(), Some("v3"), &input("late"))?,
        IngestOutcome::Forgotten
    );
    assert_eq!(
        s.ingest_source(b.id, 2, &key(), None, &input("latest source"))?,
        IngestOutcome::Applied(id)
    );
    let note = &s.evidence(b.id, 0, 50)?.items[0];
    assert_eq!(note.id, id);
    assert_eq!(note.text, "latest source");
    assert_eq!(note.kind, AssertionKind::SourceFact);
    assert!(!s.decisions(a.id, 0, 50)?.items[0].accepted);
    assert!(s.decisions(b.id, 0, 50)?.items[0].accepted);
    Ok(())
}
#[test]
fn page_caps_use_bytes_and_stable_ids_and_limits_preserve_state()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    for _ in 0..55 {
        s.create_task(&"🌿".repeat(4096))?;
    }
    let first = s.tasks(0, 50)?;
    assert!(first.items.len() < 50);
    assert!(first.payload_bytes <= 262144);
    let last = first.next_after.ok_or("missing cursor")?;
    let second = s.tasks(last, 50)?;
    assert!(second.items[0].id.0 > last);
    assert_eq!(s.tasks(0, 51), Err(StoreError::InvalidInput));
    let a = first.items[0].clone();
    assert_eq!(s.edit_task(a.id, 1, ""), Err(StoreError::InvalidInput));
    assert_eq!(
        s.edit_task(a.id, 1, &"x".repeat(16385)),
        Err(StoreError::TextTooLong)
    );
    let bad = SourceKey {
        namespace: "bad\0key".into(),
        ..key()
    };
    assert_eq!(
        s.ingest_source(a.id, 1, &bad, None, &input("x")),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(s.task(a.id)?.revision, 1);
    Ok(())
}
#[test]
fn transaction_failure_rolls_back_all_affected_revisions() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task("B")?;
    let con = Connection::open(&db.0)?;
    con.execute_batch("CREATE TRIGGER fail_revision BEFORE UPDATE ON tasks WHEN new.id=2 BEGIN SELECT RAISE(ABORT,'fixture'); END")?;
    assert_eq!(
        s.set_link(a.id, 1, b.id, 1, true, 1),
        Err(StoreError::Storage)
    );
    assert_eq!(s.task(a.id)?.revision, 1);
    assert_eq!(s.task(b.id)?.revision, 1);
    assert!(s.links(a.id, 0, 50)?.items.is_empty());
    Ok(())
}
#[test]
fn unknown_and_corrupt_schema_remain_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    for version in [0, 99] {
        let db = Database::new();
        let c = Connection::open(&db.0)?;
        c.execute_batch(
            "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('fixture')",
        )?;
        c.pragma_update(None, "user_version", version)?;
        drop(c);
        let before = std::fs::read(&db.0)?;
        assert!(matches!(
            Store::open(&db.0),
            Err(StoreError::UnsupportedSchema)
        ));
        assert_eq!(std::fs::read(&db.0)?, before);
    }
    let db = Database::new();
    std::fs::write(&db.0, b"corrupt fixture")?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, b"corrupt fixture");
    Ok(())
}

#[test]
fn deletion_checks_every_affected_revision_and_bumps_once() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task("B")?;
    let n = s.add_note(a.id, 1, "move me", 1)?;
    s.move_evidence(n.id, a.id, 2, b.id, 1, 2)?;
    let impact = s.deletion_impact(b.id, Some(n.id), 0, 50)?;
    assert_eq!(impact.items[0].id, a.id);
    assert_eq!(
        s.delete_evidence(n.id, b.id, 2, &[]),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(
        s.delete_evidence(
            n.id,
            b.id,
            2,
            &[TaskVersion {
                task_id: a.id,
                revision: 2
            }]
        ),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(s.task(b.id)?.revision, 2);
    s.delete_evidence(
        n.id,
        b.id,
        2,
        &[TaskVersion {
            task_id: a.id,
            revision: 3,
        }],
    )?;
    assert_eq!(s.task(a.id)?.revision, 4);
    assert_eq!(s.task(b.id)?.revision, 3);
    Ok(())
}
#[test]
fn deletion_releases_text_at_exact_total_byte_cap() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task(&"a".repeat(16384))?;
    let c = Connection::open(&db.0)?;
    c.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) SELECT ?1,?2,0,1,2,2 FROM n",rusqlite::params![a.id.0,"x".repeat(16384)])?;
    assert_eq!(s.add_note(a.id, 1, "x", 1), Err(StoreError::ResourceLimit));
    assert_eq!(s.create_task("x"), Err(StoreError::ResourceLimit));
    s.delete_evidence(EvidenceId(1), a.id, 1, &[])?;
    let n = s.add_note(a.id, 2, &"z".repeat(16384), 1)?;
    assert_eq!(n.text.len(), 16384);
    assert_eq!(s.add_note(a.id, 3, "x", 1), Err(StoreError::ResourceLimit));
    s.delete_task(a.id, 3, &[])?;
    assert_eq!(s.tasks(0, 50)?.items.len(), 0);
    Ok(())
}
#[test]
fn reserved_identity_and_digest_slots_allow_deletion_when_full()
-> Result<(), Box<dyn std::error::Error>> {
    for table in ["tasks", "evidence", "source_bindings", "decisions"] {
        let db = Database::new();
        let mut s = Store::open(&db.0)?;
        let a = s.create_task("A")?;
        let c = Connection::open(&db.0)?;
        match table {
            "tasks" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO tasks(id,revision,text) SELECT x,1,NULL FROM n")?;
                assert_eq!(s.create_task("blocked"), Err(StoreError::ResourceLimit));
            }
            "evidence" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO evidence(id,task_id) SELECT x,1 FROM n")?;
                assert_eq!(
                    s.add_note(a.id, 1, "blocked", 1),
                    Err(StoreError::ResourceLimit)
                );
            }
            "source_bindings" => {
                c.execute_batch("INSERT INTO evidence(id,task_id) VALUES(1,1); WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO source_bindings(task_id,evidence_id,digest) SELECT 1,1,CAST(printf('%032d',x) AS BLOB) FROM n")?;
                assert_eq!(
                    s.ingest_source(a.id, 1, &key(), None, &input("blocked")),
                    Err(StoreError::ResourceLimit)
                );
            }
            "decisions" => {
                c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO evidence(id,task_id,text,origin,observed_at_utc,completeness,kind) SELECT x,1,'x',0,1,2,2 FROM n; INSERT INTO decisions(task_id,evidence_id,accepted,action_at_utc) SELECT 1,id,1,1 FROM evidence")?;
                let b = s.create_task("B")?;
                assert_eq!(
                    s.set_link(a.id, 1, b.id, 1, true, 1),
                    Err(StoreError::ResourceLimit)
                );
            }
            _ => return Err("bad table fixture".into()),
        }
        assert_eq!(s.task(a.id)?.revision, 1);
        s.delete_task(a.id, 1, &[])?;
        assert_eq!(s.task(a.id), Err(StoreError::Deleted));
    }
    Ok(())
}
#[test]
fn live_task_cap_is_independent_of_reserved_identity_cap() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let c = Connection::open(&db.0)?;
    c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1024) INSERT INTO tasks(id,revision,text) SELECT x,1,'x' FROM n")?;
    assert_eq!(s.create_task("blocked"), Err(StoreError::ResourceLimit));
    s.delete_task(TaskId(1), 1, &[])?;
    assert_eq!(s.create_task("new")?.id, TaskId(1025));
    Ok(())
}
#[test]
fn link_page_carries_displayed_revision_and_exact_task_text()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let a = s.create_task("A")?;
    let b = s.create_task(" B🌿\n")?;
    s.set_link(a.id, 1, b.id, 1, true, 1)?;
    let page = s.links(a.id, 0, 50)?;
    assert_eq!(page.items[0].other_task_text, " B🌿\n");
    assert_eq!(page.items[0].other_task_revision, 2);
    Ok(())
}
