#[path = "support/assistance.rs"]
pub mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);

#[test]
fn saved_metadata_pages_and_selection_limits_remain_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    for n in 1..=51 {
        let bytes = String::from_utf8(rollout(n, "saved"))?
            .replace("\"id\":\"one\"", &format!("\"id\":\"one-{n}\""))
            .into_bytes();
        let end = bytes.iter().position(|b| *b == b'\n').ok_or("header")? + 1;
        let op = s.begin_codex_read(
            TaskVersion {
                task_id: t.id,
                revision: s.task(t.id)?.revision,
            },
            a.id,
            a.revision,
            0,
        )?;
        s.plan_codex_read(
            op,
            CodexFileIdentity {
                device: 1,
                inode: 2,
                size: bytes.len() as u64,
            },
            &bytes[..end],
        )?;
        s.finish_codex_read(op, &bytes, 1)?;
        s.bind_codex_event(op, 0, 2)?;
        s.release_codex_read(op)?;
    }
    let page = s.codex_metadata(t.id, 0, 50)?;
    assert_eq!(page.items.len(), 50);
    assert!(page.payload_bytes <= 262144);
    let last = s.codex_metadata(t.id, page.next_after.ok_or("continuation")?, 50)?;
    assert_eq!(last.items.len(), 1);
    assert!(last.next_after.is_none());
    assert_eq!(last.items[0].identity.item_id(), Some("one-51"));
    for n in 1..64 {
        s.save_codex_selection(0, 0, &format!("/private/tmp/selected-{n}.jsonl"))?;
    }
    assert_eq!(
        s.save_codex_selection(0, 0, "/private/tmp/overflow.jsonl")
            .err(),
        Some(StoreError::ResourceLimit)
    );
    let max = s
        .codex_selections()?
        .iter()
        .map(|s| s.id)
        .max()
        .ok_or("settings")?;
    s.delete_codex_selection(a.id, a.revision)?;
    let new = s.save_codex_selection(0, 0, &a.path)?;
    assert!(new.id > max);
    assert!(
        {
            let current = s.task(t.id)?.revision;
            prepare_new(&mut s, t.id, current, &[page.items[0].evidence_id])
        }
        .is_err()
    );
    Ok(())
}

#[test]
fn cursor_rejects_replacement_truncation_header_change_and_keeps_append_outside_window()
-> Result<(), Box<dyn std::error::Error>> {
    for change in ["inode", "device", "truncation", "header", "append"] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("intent")?;
        let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
        let task = TaskVersion {
            task_id: t.id,
            revision: 1,
        };
        let data = rollout(1, "text");
        let header_end = data.iter().position(|b| *b == b'\n').ok_or("header")? + 1;
        let mut header = data[..header_end].to_vec();
        let file = CodexFileIdentity {
            device: 1,
            inode: 2,
            size: 2 * 1024 * 1024,
        };
        let op = s.begin_codex_read(task, a.id, a.revision, 0)?;
        s.plan_codex_read(op, file, &header)?;
        let first = s.finish_codex_read(op, &vec![b'\n'; 1024 * 1024 + 1], 1)?;
        assert!(first.cursor > 0 && first.window.next_before > 0);
        let op = s.begin_codex_read(task, a.id, a.revision, first.cursor)?;
        let mut changed = file;
        match change {
            "inode" => changed.inode = 3,
            "device" => changed.device = 3,
            "truncation" => changed.size -= 1,
            "header" => {
                header = String::from_utf8(header)?
                    .replace("thread-one", "thread-two")
                    .into_bytes();
            }
            _ => changed.size += 1024 * 1024,
        }
        let result = s.plan_codex_read(op, changed, &header);
        if change == "append" {
            assert_eq!(result?.end, first.window.next_before);
        } else {
            assert_eq!(result.err(), Some(StoreError::StaleRevision));
        }
        s.abort_codex_read(op)?;
        s.release_codex_read(op)?;
    }
    Ok(())
}
struct DB(PathBuf);
impl DB {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-codex-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for DB {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn ready(s: &mut Store) -> Result<(), StoreError> {
    s.set_model_session(9, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(
        d.id(),
        ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::High,
                tier: ActualTier::Fast,
            },
            usage: None,
        },
    )?;
    s.release_check(&flag);
    Ok(())
}
fn rollout(ordinal: u64, text: &str) -> Vec<u8> {
    let header=json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"thread-one","cli_version":"0.163.0-alpha.10.2","history_mode":"paginated"}}).to_string();
    let row=json!({"timestamp":"2026-10-20T08:00:01.000Z","ordinal":ordinal,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-one","turn_id":"turn-one","item":{"type":"UserMessage","id":"one","content":[{"type":"text","text":text}]}}}).to_string();
    format!("{header}\n{row}\n").into_bytes()
}
fn read(
    s: &mut Store,
    t: TaskId,
    setting: &CodexSelection,
    ordinal: u64,
    text: &str,
) -> Result<u64, StoreError> {
    let task = s.task(t)?;
    let op = s.begin_codex_read(
        TaskVersion {
            task_id: t,
            revision: task.revision,
        },
        setting.id,
        setting.revision,
        0,
    )?;
    let bytes = rollout(ordinal, text);
    let n = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or(StoreError::InvalidInput)?
        + 1;
    let plan = s.plan_codex_read(
        op,
        CodexFileIdentity {
            device: 1,
            inode: 2,
            size: bytes.len() as u64,
        },
        &bytes[..n],
    )?;
    assert_eq!(plan.start, 0);
    assert_eq!(plan.end, bytes.len() as u64);
    s.finish_codex_read(op, &bytes, 100)?;
    Ok(op)
}
#[test]
fn selection_requires_high_and_binding_metadata_survives_reopen_with_fixed_local_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("intent")?;
    let setting = s.save_codex_selection(0, 0, "/private/tmp/selected.jsonl")?;
    assert_eq!(
        s.begin_codex_read(
            TaskVersion {
                task_id: t.id,
                revision: 1
            },
            setting.id,
            setting.revision,
            0
        )
        .err(),
        Some(StoreError::SessionRequired)
    );
    ready(&mut s)?;
    let op = read(&mut s, t.id, &setting, 1, "exact prompt")?;
    let id = s.bind_codex_event(op, 0, 2)?;
    s.release_codex_read(op)?;
    let metadata = s.codex_metadata(t.id, 0, 50)?.items;
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0].evidence_id, id);
    assert_eq!(metadata[0].accepted_ordinal, 1);
    assert_eq!(metadata[0].identity.kind(), CodexEventKind::User);
    drop(s);
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    assert_eq!(s.codex_metadata(t.id, 0, 50)?.items, metadata);
    let p = {
        let current = s.task(t.id)?.revision;
        prepare_new(&mut s, t.id, current, &[id])?
    };
    let v: serde_json::Value = serde_json::from_str(&p.context)?;
    assert_eq!(
        v["sources"][0]["coverage"],
        "local_records_only/local_window"
    );
    assert_eq!(v["sources"][0]["provenance"], "human_unverified");
    assert_eq!(v["sources"][0]["text"], "exact prompt");
    s.delete_codex_selection(setting.id, setting.revision)?;
    assert!(s.assistance_preview(p.request_id).is_err());
    assert!(
        {
            let current = s.task(t.id)?.revision;
            prepare_new(&mut s, t.id, current, &[id])
        }
        .is_err()
    );
    assert_eq!(s.evidence(t.id, 0, 50)?.items[0].text, "exact prompt");
    Ok(())
}
#[test]
fn duplicate_selection_conflict_ordinal_order_and_protection_are_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    let b = s.save_codex_selection(0, 0, "/private/tmp/b.jsonl")?;
    let op = read(&mut s, t.id, &a, 10, "saved")?;
    let id = s.bind_codex_event(op, 0, 2)?;
    s.release_codex_read(op)?;
    let revision = s.task(t.id)?.revision;
    let before = s.codex_metadata(t.id, 0, 50)?;
    let op = read(&mut s, t.id, &b, 11, "copy")?;
    assert!(s.bind_codex_event(op, 0, 3).is_err());
    s.release_codex_read(op)?;
    assert_eq!(s.task(t.id)?.revision, revision);
    assert_eq!(s.codex_metadata(t.id, 0, 50)?, before);
    let a = s.save_codex_selection(a.id, a.revision, "/private/tmp/new.jsonl")?;
    for (ordinal, text) in [(9, "older"), (10, "conflict")] {
        let op = read(&mut s, t.id, &a, ordinal, text)?;
        assert!(s.bind_codex_event(op, 0, 4).is_err());
        s.release_codex_read(op)?;
    }
    let op = read(&mut s, t.id, &a, 10, "saved")?;
    assert_eq!(s.bind_codex_event(op, 0, 5)?, id);
    s.release_codex_read(op)?;
    assert!(s.task(t.id)?.revision > revision);
    assert_eq!(
        s.codex_metadata(t.id, 0, 50)?.items[0].settings_revision,
        a.revision
    );
    s.edit_evidence(id, t.id, s.task(t.id)?.revision, "user correction", 6)?;
    let op = read(&mut s, t.id, &a, 12, "new source")?;
    assert!(s.bind_codex_event(op, 0, 7).is_err());
    s.release_codex_read(op)?;
    assert_eq!(s.evidence(t.id, 0, 50)?.items[0].text, "user correction");
    s.delete_evidence(id, t.id, s.task(t.id)?.revision, &[])?;
    assert!(s.codex_metadata(t.id, 0, 50)?.items.is_empty());
    let op = read(&mut s, t.id, &a, 13, "revive")?;
    assert!(s.bind_codex_event(op, 0, 8).is_err());
    s.release_codex_read(op)?;
    assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    Ok(())
}
#[test]
fn cancellation_and_settings_change_do_not_release_active_io_slot()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let tv = TaskVersion {
        task_id: t.id,
        revision: 1,
    };
    let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    let op = s.begin_codex_read(tv, a.id, a.revision, 0)?;
    s.set_manual_pause(true)?;
    assert!(s.permit_codex_read(op).is_err());
    assert_eq!(s.release_codex_read(op), Err(StoreError::Busy));
    s.set_manual_pause(false)?;
    assert_eq!(
        s.begin_codex_read(tv, a.id, a.revision, 0).err(),
        Some(StoreError::Busy)
    );
    s.abort_codex_read(op)?;
    s.release_codex_read(op)?;
    let op = s.begin_codex_read(tv, a.id, a.revision, 0)?;
    s.save_codex_selection(a.id, a.revision, "/private/tmp/b.jsonl")?;
    assert!(s.permit_codex_read(op).is_err());
    assert_eq!(s.release_codex_read(op), Err(StoreError::Busy));
    s.abort_codex_read(op)?;
    s.release_codex_read(op)?;
    Ok(())
}
#[test]
fn generic_edit_enforces_combined_metadata_limit_without_partial_change()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let setting = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    let op = read(&mut s, t.id, &setting, 1, "saved")?;
    let id = s.bind_codex_event(op, 0, 1)?;
    s.release_codex_read(op)?;
    let revision = s.task(t.id)?.revision;
    assert!(
        s.edit_evidence(id, t.id, revision, &"x".repeat(16384), 2)
            .is_err()
    );
    assert_eq!(s.task(t.id)?.revision, revision);
    assert_eq!(s.evidence(t.id, 0, 50)?.items[0].text, "saved");
    drop(s);
    assert!(Store::open(&db.0).is_ok());
    Ok(())
}
#[test]
fn either_source_settings_change_cancels_both_native_jobs() -> Result<(), Box<dyn std::error::Error>>
{
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let tv = TaskVersion {
        task_id: t.id,
        revision: 1,
    };
    let root = s.save_local_root("/private/tmp/project", 0, &[])?;
    let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    let git = s.begin_local_read(tv, &root.path, root.revision)?;
    let codex = s.begin_codex_read(tv, a.id, a.revision, 0)?;
    let a = s.save_codex_selection(a.id, a.revision, "/private/tmp/b.jsonl")?;
    assert!(s.permit_local_read(git, "", true).is_err());
    assert!(s.permit_codex_read(codex).is_err());
    s.abort_local_read(git)?;
    s.release_local_read(git)?;
    s.abort_codex_read(codex)?;
    s.release_codex_read(codex)?;
    let git = s.begin_local_read(tv, &root.path, root.revision)?;
    let codex = s.begin_codex_read(tv, a.id, a.revision, 0)?;
    s.save_local_root(&root.path, root.revision, &[".git/config".into()])?;
    assert!(s.permit_local_read(git, "", true).is_err());
    assert!(s.permit_codex_read(codex).is_err());
    s.abort_local_read(git)?;
    s.release_local_read(git)?;
    s.abort_codex_read(codex)?;
    s.release_codex_read(codex)?;
    Ok(())
}
#[test]
fn uncited_codex_grounds_are_revoked_before_dispatch_result_copy_and_followup()
-> Result<(), Box<dyn std::error::Error>> {
    for stage in ["dispatch", "result", "copy", "followup"] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("intent")?;
        let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
        let op = read(&mut s, t.id, &a, 1, "saved")?;
        let id = s.bind_codex_event(op, 0, 1)?;
        s.release_codex_read(op)?;
        let p = {
            let current = s.task(t.id)?.revision;
            prepare_new(&mut s, t.id, current, &[id])?
        };
        let admission = s.begin_assistance(p.request_id)?;
        let flag = admission.cancellation();
        if stage == "dispatch" {
            s.delete_codex_selection(a.id, a.revision)?;
            assert!(s.dispatch_check(admission, 2).is_err());
            s.release_check(&flag);
            continue;
        }
        let d = s.dispatch_check(admission, 2)?;
        if stage == "result" {
            s.delete_codex_selection(a.id, a.revision)?;
        }
        let receipt = finish_single(
            &mut s,
            d.id(),
            ModelCheckResult {
                outcome: CheckOutcome::Confirmed,
                actual: ActualConfig {
                    model: ActualModel::Luna,
                    effort: ActualEffort::High,
                    tier: ActualTier::Fast,
                },
                usage: None,
            },
            Some(AssistanceAnswer {
                proposals: vec![ProposalAnswer {
                    proposal_id: None,
                    kind: AssistanceKind::Clarification,
                    message: "model question".into(),
                    rationale: "Saved intent".into(),
                    evidence_ids: vec![format!("task:{}", t.id.0)],
                    step_id: None,
                }],
                rationale: "Full set".into(),
            }),
        )?;
        s.release_check(&flag);
        if stage == "result" {
            assert!(receipt.is_none());
            continue;
        }
        let receipt = receipt.ok_or("receipt")?;
        assert_eq!(s.proposal_details(receipt)?.citations.len(), 1);
        let pending = if stage == "followup" {
            Some(prepare_reply(&mut s, receipt, "independent user text")?)
        } else {
            None
        };
        s.delete_codex_selection(a.id, a.revision)?;
        assert!(s.copy_proposal(receipt).is_err());
        assert!(prepare_reply(&mut s, receipt, "reply").is_err());
        if let Some(p) = pending {
            assert!(s.begin_assistance(p.request_id).is_err());
        }
    }
    Ok(())
}
#[test]
fn combined_retained_metadata_failure_rolls_back_binding_evidence_and_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    let op = read(&mut s, t.id, &a, 1, "saved")?;
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) SELECT ?1,?2,0,1,2,2 FROM n",rusqlite::params![t.id.0,"x".repeat(16384)])?;
    c.execute("INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,0,1,2,2)",rusqlite::params![t.id.0,"x".repeat(16200)])?;
    assert_eq!(s.bind_codex_event(op, 0, 2), Err(StoreError::ResourceLimit));
    assert_eq!(s.task(t.id)?.revision, 1);
    for table in ["source_bindings", "codex_metadata"] {
        assert_eq!(
            c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))?,
            0
        );
    }
    assert_eq!(
        c.query_row("SELECT count(*) FROM evidence", [], |r| r.get::<_, i64>(0))?,
        4096
    );
    s.release_codex_read(op)?;
    Ok(())
}
#[test]
fn metadata_identity_corruption_is_read_only_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let a = s.save_codex_selection(0, 0, "/private/tmp/a.jsonl")?;
    let op = read(&mut s, t.id, &a, 1, "saved")?;
    s.bind_codex_event(op, 0, 1)?;
    s.release_codex_read(op)?;
    drop(s);
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute("UPDATE codex_metadata SET turn_id='different-turn'", [])?;
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
fn ordinary_codex_refresh_retains_stale_workspace_and_dialogue_but_settings_revoke_clears()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let task = s.create_task("intent")?;
    let selection = s.save_codex_selection(0, 0, "/private/tmp/selected.jsonl")?;
    let op = read(&mut s, task.id, &selection, 1, "original source")?;
    let evidence = s.bind_codex_event(op, 0, 1)?;
    s.release_codex_read(op)?;
    let task = s.task(task.id)?;
    let p = prepare_new(&mut s, task.id, task.revision, &[evidence])?;
    let answer = |id| AssistanceAnswer {
        proposals: vec![ProposalAnswer {
            proposal_id: id,
            kind: AssistanceKind::Clarification,
            message: "Unconfirmed premise?".into(),
            rationale: "Saved intent".into(),
            evidence_ids: vec![format!("task:{}", task.id.0)],
            step_id: None,
        }],
        rationale: "Full set".into(),
    };
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 2)?;
    s.finish_assistance(
        d.id(),
        ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::High,
                tier: ActualTier::Fast,
            },
            usage: None,
        },
        Some(answer(None)),
    )?;
    s.release_check(&flag);
    let row = s.proposal_feed()?.proposals.remove(0);
    let p = prepare_reply(&mut s, row.target(), "exact user reply")?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 3)?;
    s.finish_assistance(
        d.id(),
        ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::High,
                tier: ActualTier::Fast,
            },
            usage: None,
        },
        Some(answer(Some(row.id))),
    )?;
    s.release_check(&flag);
    let before = s.assistant_workspace_revision(task.id)?;
    let target = s.proposal_feed()?.proposals[0].target();
    let op = read(&mut s, task.id, &selection, 2, "updated source")?;
    assert_eq!(s.bind_codex_event(op, 0, 4)?, evidence);
    s.release_codex_read(op)?;
    let rows = s.proposal_feed()?.proposals;
    assert_eq!(
        rows.len(),
        1,
        "ordinary source refresh must retain stale help"
    );
    assert_eq!(rows[0].target(), target);
    assert!(rows[0].stale);
    assert_eq!(s.assistant_workspace_revision(task.id)?, before);
    assert!(s.proposal_details(target)?.citations.is_empty());
    assert!(s.copy_proposal(target).is_err());
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert!(s.proposal_feed()?.proposals[0].stale);
    let c = rusqlite::Connection::open(&db.0)?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM assistant_replies", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    drop(c);
    s.save_codex_selection(selection.id, selection.revision, &selection.path)?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    assert_eq!(s.assistant_workspace_revision(task.id)?, before + 1);
    let c = rusqlite::Connection::open(&db.0)?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM assistant_replies", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    Ok(())
}
