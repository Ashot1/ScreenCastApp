#[path = "support/assistance.rs"]
pub mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use rusqlite::Connection;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-check-{}-{}.sqlite3",
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
fn admission_pause_restart_and_known_usage_never_erase_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    assert!(matches!(
        s.prepare_check(false, ModelProfile::Low),
        Err(StoreError::ReportRequired)
    ));
    s.set_manual_pause(true)?;
    assert!(matches!(
        s.prepare_check(true, ModelProfile::Low),
        Err(StoreError::Paused)
    ));
    s.set_manual_pause(false)?;
    let admission = s.prepare_check(true, ModelProfile::Low)?;
    let flag = admission.cancellation();
    assert!(matches!(
        s.prepare_check(true, ModelProfile::Low),
        Err(StoreError::Busy)
    ));
    s.set_manual_pause(true)?;
    s.set_manual_pause(false)?;
    assert!(matches!(
        s.dispatch_check(admission, 10),
        Err(StoreError::Cancelled)
    ));
    s.release_check(&flag);
    assert_eq!(s.check_summary()?.attempts, 0);
    let a = s.prepare_check(true, ModelProfile::Low)?;
    let permit = s.dispatch_check(a, 11)?;
    assert_eq!(s.check_summary()?.unknown, 1);
    drop(permit);
    drop(s);
    let mut s = Store::open(&db.0)?;
    let summary = s.check_summary()?;
    assert_eq!(
        summary.latest.map(|v| v.result.outcome),
        Some(CheckOutcome::Interrupted)
    );
    assert_eq!(summary.unknown, 1);
    let a = s.prepare_check(true, ModelProfile::Low)?;
    let flag = a.cancellation();
    let permit = s.dispatch_check(a, 12)?;
    s.finish_check(
        permit.id(),
        ModelCheckResult {
            outcome: CheckOutcome::Mismatch,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::Low,
                tier: ActualTier::Default,
            },
            usage: Some(CheckUsage::new(8, 3, 11)?),
        },
    )?;
    s.release_check(&flag);
    let summary = s.check_summary()?;
    assert_eq!(
        (
            summary.attempts,
            summary.input,
            summary.output,
            summary.total,
            summary.unknown
        ),
        (2, 8, 3, 11, 1)
    );
    drop(s);
    let summary = Store::open(&db.0)?.check_summary()?;
    assert_eq!(summary.unknown, 1);
    assert_eq!(summary.total, 11);
    assert!(CheckUsage::new(1, 2, 4).is_err());
    assert!(CheckUsage::new(i64::MAX as u64, 1, i64::MAX as u64 + 1).is_err());
    Ok(())
}
#[test]
fn schema_three_and_its_wal_are_rejected_without_change() -> Result<(), Box<dyn std::error::Error>>
{
    let db = Database::new();
    let c = Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-three.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=3; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO tasks(revision,text) VALUES(1,'preserve');")?;
    let wal = PathBuf::from(format!("{}-wal", db.0.display()));
    let bytes = std::fs::read(&db.0)?;
    let wal_bytes = std::fs::read(&wal)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, bytes);
    assert_eq!(std::fs::read(&wal)?, wal_bytes);
    drop(c);
    Ok(())
}
#[test]
fn schema_four_and_its_wal_are_rejected_without_change() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let c = Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-four.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=4; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO tasks(revision,text) VALUES(1,'preserve');")?;
    let wal = PathBuf::from(format!("{}-wal", db.0.display()));
    let bytes = std::fs::read(&db.0)?;
    let wal_bytes = std::fs::read(&wal)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, bytes);
    assert_eq!(std::fs::read(&wal)?, wal_bytes);
    drop(c);
    Ok(())
}
#[test]
fn failed_commit_denies_admission_and_a_full_journal_folds_old_rows_into_totals()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let c = Connection::open(&db.0)?;
    c.execute_batch("CREATE TRIGGER reject_attempt BEFORE INSERT ON connection_checks BEGIN SELECT RAISE(ABORT,'test'); END;")?;
    let a = s.prepare_check(true, ModelProfile::Low)?;
    let flag = a.cancellation();
    assert!(s.dispatch_check(a, 1).is_err());
    s.release_check(&flag);
    assert_eq!(s.check_summary()?.attempts, 0);
    c.execute_batch("DROP TRIGGER reject_attempt;")?;
    // An in-flight stage-1 row is the oldest; then 4095 completed rows: every third has known usage.
    c.execute(
        "INSERT INTO connection_checks VALUES(1,1,1,1,0,1,1,1,0,0,0,NULL,NULL,NULL)",
        [],
    )?;
    let tx = c.unchecked_transaction()?;
    for id in 2..=4096i64 {
        if id % 3 == 0 {
            tx.execute(
                "INSERT INTO connection_checks VALUES(?1,2,?1,2,1,1,2,1,1,3,2,10,5,15)",
                [id],
            )?;
        } else {
            tx.execute(
                "INSERT INTO connection_checks VALUES(?1,1,?1,2,3,1,1,1,0,0,0,NULL,NULL,NULL)",
                [id],
            )?;
        }
    }
    tx.commit()?;
    let before = s.check_summary()?;
    assert_eq!(before.attempts, 4096);
    for at in 0..3 {
        let a = s.prepare_check(true, ModelProfile::Low)?;
        let flag = a.cancellation();
        let p = s.dispatch_check(a, 5000 + at)?;
        s.finish_check(
            p.id(),
            ModelCheckResult {
                usage: Some(CheckUsage::new(1, 1, 2)?),
                ..ModelCheckResult::unknown(CheckOutcome::Provider)
            },
        )?;
        s.release_check(&flag);
    }
    let after = s.check_summary()?;
    assert_eq!(after.attempts, before.attempts + 3);
    assert_eq!(after.unknown, before.unknown);
    assert_eq!(after.input, before.input + 3);
    assert_eq!(after.total, before.total + 6);
    assert_eq!(after.latest.map(|l| l.id.get()), Some(4099));
    let live: i64 = c.query_row("SELECT count(*) FROM connection_checks", [], |r| r.get(0))?;
    assert_eq!(live, 4096);
    // The stage-1 row is never folded; the three oldest completed rows are.
    let oldest: Vec<i64> = c
        .prepare("SELECT id FROM connection_checks ORDER BY id LIMIT 2")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    assert_eq!(oldest, vec![1, 5]);
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(s.check_summary()?.attempts, after.attempts);
    assert_eq!(s.check_summary()?.unknown, after.unknown);
    assert_eq!(s.check_summary()?.total, after.total);
    Ok(())
}

#[test]
fn failed_result_commit_keeps_original_unknown_and_checked_totals_reject_overflow()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    let c = Connection::open(&db.0)?;
    let a = s.prepare_check(true, ModelProfile::Low)?;
    let flag = a.cancellation();
    let p = s.dispatch_check(a, 1)?;
    c.execute_batch("CREATE TRIGGER reject_result BEFORE UPDATE ON connection_checks BEGIN SELECT RAISE(ABORT,'test'); END;")?;
    assert!(
        s.finish_check(
            p.id(),
            ModelCheckResult {
                usage: Some(CheckUsage::new(8, 3, 11)?),
                ..ModelCheckResult::unknown(CheckOutcome::Mismatch)
            }
        )
        .is_err()
    );
    s.release_check(&flag);
    assert_eq!(s.check_summary()?.unknown, 1);
    c.execute_batch("DROP TRIGGER reject_result;")?;
    for index in 0..3 {
        let a = s.prepare_check(true, ModelProfile::Low)?;
        let flag = a.cancellation();
        let p = s.dispatch_check(a, 2)?;
        let result = s.finish_check(
            p.id(),
            ModelCheckResult {
                usage: Some(CheckUsage::new(i64::MAX as u64, 0, i64::MAX as u64)?),
                ..ModelCheckResult::unknown(CheckOutcome::Mismatch)
            },
        );
        if index < 2 {
            result?;
        } else {
            assert_eq!(result, Err(StoreError::ResourceLimit));
        }
        s.release_check(&flag);
    }
    let r = s.check_summary()?;
    assert_eq!(r.total, u64::MAX - 1);
    assert_eq!(r.unknown, 2);
    drop(s);
    assert_eq!(Store::open(&db.0)?.check_summary()?.unknown, 2);
    Ok(())
}

#[test]
fn only_committed_high_fast_of_current_epoch_opens_assistance()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    s.set_model_session(10, true, true)?;
    let task = s.create_task("Prepare release")?;
    assert!(matches!(
        prepare_new(&mut s, task.id, task.revision, &[]),
        Err(StoreError::CapabilityRequired)
    ));
    let a = s.prepare_check(true, ModelProfile::Low)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(
        d.id(),
        ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::Low,
                tier: ActualTier::Priority,
            },
            usage: Some(CheckUsage::new(8, 3, 11)?),
        },
    )?;
    s.release_check(&flag);
    assert!(!s.assistance_available()?);
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    s.set_model_session(11, true, true)?;
    s.finish_check(d.id(), high_result()?)?;
    s.release_check(&flag);
    assert!(!s.assistance_available()?);
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 3)?;
    s.finish_check(d.id(), high_result()?)?;
    assert!(s.assistance_available()?);
    assert!(matches!(
        prepare_new(&mut s, task.id, task.revision, &[]),
        Err(StoreError::Busy)
    ));
    s.release_check(&flag);
    let preview = prepare_new(&mut s, task.id, task.revision, &[])?;
    assert!(preview.context.contains("Prepare release"));
    s.set_model_session(11, true, false)?;
    assert!(!s.assistance_available()?);
    assert!(s.assistance_preview(preview.request_id).is_err());
    Ok(())
}
fn high_result() -> Result<ModelCheckResult, StoreError> {
    Ok(ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Fast,
        },
        usage: Some(CheckUsage::new(8, 3, 11)?),
    })
}
