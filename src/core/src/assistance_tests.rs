use super::*;
use std::sync::{Arc, Barrier, Mutex, atomic::Ordering};
static WAIT: Mutex<Option<(Arc<Barrier>, Arc<Barrier>)>> = Mutex::new(None);
fn blocked(_: i32) -> bool {
    let pair = WAIT
        .lock()
        .unwrap_or_else(|_| panic!("test lock"))
        .clone()
        .unwrap_or_else(|| panic!("test barrier"));
    pair.0.wait();
    pair.1.wait();
    true
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
#[test]
fn cancellation_during_sqlite_write_wait_cannot_grant_capability_or_a_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    for assistance in [false, true] {
        let path = std::env::temp_dir().join(format!(
            "ambient-cancel-commit-{}-{assistance}.sqlite3",
            std::process::id()
        ));
        let mut s = Store::open(&path)?;
        s.set_model_session(42, true, true)?;
        let a = s.prepare_check(true, ModelProfile::High)?;
        let prior = a.cancellation();
        let d = s.dispatch_check(a, 1)?;
        let (d, flag, task) = if assistance {
            s.finish_check(d.id(), high())?;
            s.release_check(&prior);
            let task = s.create_task("saved intent")?;
            let p = s.prepare_assistance(AssistanceRequest {
                task: TaskVersion {
                    task_id: task.id,
                    revision: task.revision,
                },
                workspace_revision: 1,
                mode: AssistanceMode::NewContext,
                selected_evidence: vec![],
                reply: None,
            })?;
            let a = s.begin_assistance(p.request_id)?;
            let flag = a.cancellation();
            (s.dispatch_check(a, 2)?, flag, Some(task))
        } else {
            (d, prior, None)
        };
        let locked = Connection::open(&path)?;
        locked.execute_batch("BEGIN IMMEDIATE")?;
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        *WAIT.lock().map_err(|_| "test lock")? = Some((entered.clone(), release.clone()));
        s.connection.busy_handler(Some(blocked))?;
        let id = d.id();
        let worker = std::thread::spawn(move || {
            let receipt = if let Some(task) = task {
                s.finish_assistance(
                    id,
                    high(),
                    Some(AssistanceAnswer {
                        proposals: vec![ProposalAnswer {
                            proposal_id: None,
                            kind: AssistanceKind::Suggestion,
                            message: "late copy".into(),
                            rationale: "Saved intent".into(),
                            evidence_ids: vec![format!("task:{}", task.id.0)],
                            step_id: None,
                        }],
                        rationale: "Full set".into(),
                    }),
                )?
            } else {
                s.finish_check(id, high())?;
                None
            };
            Ok::<_, StoreError>((s, receipt))
        });
        entered.wait();
        flag.store(true, Ordering::Release);
        locked.execute_batch("ROLLBACK")?;
        release.wait();
        let (mut s, receipt) = worker.join().map_err(|_| "worker")??;
        assert!(receipt.is_none());
        if !assistance {
            assert!(!s.assistance_available()?);
        }
        assert_eq!(
            s.check_summary()?.latest.map(|r| r.result.outcome),
            Some(CheckOutcome::Cancelled)
        );
        assert_eq!(s.check_summary()?.total, if assistance { 22 } else { 11 });
        s.release_check(&flag);
        drop(s);
        drop(locked);
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[test]
fn persistent_workspace_schema_is_seventeen() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("ambient-schema8-{}.sqlite3", std::process::id()));
    let s = Store::open(&path)?;
    let version: i32 = s
        .connection
        .pragma_query_value(None, "user_version", |r| r.get(0))?;
    drop(s);
    std::fs::remove_file(path)?;
    assert_eq!(version, 17, "the observation core requires schema17");
    Ok(())
}
