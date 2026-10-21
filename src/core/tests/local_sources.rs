#[path = "support/assistance.rs"]
pub mod assistance_support;
use ambient_core::*;
use assistance_support::*;
const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
fn root(ex: &[&str]) -> LocalRootSettings {
    LocalRootSettings {
        path: "/private/tmp/project".into(),
        revision: 1,
        exclusions: ex.iter().map(|s| s.to_string()).collect(),
    }
}
fn files(values: &[(&str, String)]) -> Vec<LocalGitFile> {
    values
        .iter()
        .map(|(p, b)| LocalGitFile {
            path: p.to_string(),
            bytes: b.as_bytes().to_vec(),
        })
        .collect()
}
#[test]
fn loose_overrides_packed_and_symbolic_head_is_proven() -> Result<(), StoreError> {
    let f = files(&[
        (".git/config", "[core]\nrepositoryformatversion=0\n".into()),
        (".git/HEAD", "ref: refs/heads/main\n".into()),
        (
            ".git/packed-refs",
            format!("{A} refs/heads/main\n{A} refs/heads/old\n"),
        ),
        (".git/refs/heads/main", format!("{B}\n")),
    ]);
    let o = parse_local_git(&root(&[]), &f, 10, 11, &[])?;
    assert_eq!(o.completeness, Completeness::Complete);
    assert_eq!(
        o.branches,
        vec![
            LocalBranch {
                full_ref: "refs/heads/main".into(),
                oid: Some(B.into()),
                head: BranchHead::Current
            },
            LocalBranch {
                full_ref: "refs/heads/old".into(),
                oid: Some(A.into()),
                head: BranchHead::Other
            }
        ]
    );
    Ok(())
}
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct DB(PathBuf);
impl DB {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-local-{}-{}.sqlite3",
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
fn high() -> ModelCheckResult {
    ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Fast,
        },
        usage: None,
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
fn fixture() -> Vec<LocalGitFile> {
    files(&[
        (".git/config", "[core]\nrepositoryformatversion=0\n".into()),
        (".git/HEAD", "ref: refs/heads/main\n".into()),
        (".git/refs/heads/main", format!("{A}\n")),
    ])
}
fn import(s: &mut Store, t: TaskVersion, r: &LocalRootSettings) -> Result<EvidenceId, StoreError> {
    let op = s.begin_local_read(t, &r.path, r.revision)?;
    s.finish_local_read(
        op,
        &fixture(),
        LocalReadStats {
            bytes: 256,
            files: 3,
            ..Default::default()
        },
        1,
        2,
        &[],
    )?;
    let e = s.bind_local_branch(op, 0, 3)?;
    s.release_local_read(op)?;
    Ok(e)
}
#[test]
fn selected_source_context_and_receipt_follow_current_scope_not_only_task_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("finish chosen work")?;
    let r = s.save_local_root("/private/tmp/project", 0, &[])?;
    let e = import(
        &mut s,
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        &r,
    )?;
    let t = s.task(t.id)?;
    let note = s.add_note(t.id, t.revision, "my independent note", 1)?;
    let t = s.task(t.id)?;
    let preview = prepare_new(&mut s, t.id, t.revision, &[note.id, e])?;
    let v: serde_json::Value = serde_json::from_str(&preview.context)?;
    assert_eq!(
        v["sources"][0]["source"]["namespace"],
        "local-git-branch-v1"
    );
    assert_eq!(v["sources"][0]["source"]["object_id"], "refs/heads/main");
    assert_eq!(v["notes"][0]["text"], "my independent note");
    let a = s.begin_assistance(preview.request_id)?;
    let flag = a.cancellation();
    let r = s.save_local_root(&r.path, r.revision, &[".git/refs/heads/main".into()])?;
    assert!(s.dispatch_check(a, 4).is_err());
    s.release_check(&flag);
    assert_eq!(s.check_summary()?.attempts, 1);
    assert!(prepare_new(&mut s, t.id, t.revision, &[e]).is_err());
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 2);
    let r = s.save_local_root(&r.path, r.revision, &[])?;
    let p = prepare_new(&mut s, t.id, t.revision, &[e])?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 5)?;
    let answer = AssistanceAnswer {
        proposals: vec![ProposalAnswer {
            proposal_id: None,
            kind: AssistanceKind::Suggestion,
            message: "exact source suggestion".into(),
            rationale: "dated source, saved intent".into(),
            evidence_ids: vec![format!("source:{}", e.0)],
            step_id: None,
        }],
        rationale: "Full set".into(),
    };
    let receipt = finish_single(&mut s, d.id(), high(), Some(answer))?.ok_or("receipt")?;
    s.release_check(&flag);
    assert_eq!(s.copy_proposal(receipt)?, "exact source suggestion");
    s.delete_local_root(&r.path, r.revision)?;
    let recreated = s.save_local_root(&r.path, 0, &[])?;
    assert!(recreated.revision > r.revision);
    assert!(s.copy_proposal(receipt).is_err());
    assert_eq!(s.task(t.id)?.revision, t.revision);
    assert_eq!(s.check_summary()?.unknown, 2);
    Ok(())
}
#[test]
fn gate_cancel_and_cleanup_preserve_busy_and_forgetting_is_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("task")?;
    let tv = TaskVersion {
        task_id: t.id,
        revision: t.revision,
    };
    let r = s.save_local_root("/private/tmp/project", 0, &[])?;
    assert!(s.begin_local_read(tv, &r.path, r.revision).is_err());
    ready(&mut s)?;
    let op = s.begin_local_read(tv, &r.path, r.revision)?;
    s.cancel_local_read(op);
    assert_eq!(s.release_local_read(op), Err(StoreError::Busy));
    assert_eq!(
        s.begin_local_read(tv, &r.path, r.revision),
        Err(StoreError::Busy)
    );
    assert_eq!(
        s.permit_local_read(op, ".git/config", false),
        Err(StoreError::Cancelled)
    );
    assert!(
        s.finish_local_read(
            op,
            &fixture(),
            LocalReadStats {
                bytes: 256,
                files: 3,
                ..Default::default()
            },
            1,
            2,
            &[]
        )
        .is_err()
    );
    s.release_local_read(op)?;
    let e = import(&mut s, tv, &r)?;
    let t = s.task(t.id)?;
    s.delete_evidence(e, t.id, t.revision, &[])?;
    let t = s.task(t.id)?;
    let op = s.begin_local_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        &r.path,
        r.revision,
    )?;
    s.finish_local_read(
        op,
        &fixture(),
        LocalReadStats {
            bytes: 256,
            files: 3,
            ..Default::default()
        },
        1,
        2,
        &[],
    )?;
    assert_eq!(s.bind_local_branch(op, 0, 3), Err(StoreError::Deleted));
    let (key, version, ev) = s.local_branch_evidence(op, 0)?;
    assert_eq!(
        s.ingest_source(t.id, t.revision, &key, version.as_deref(), &ev)?,
        IngestOutcome::Forgotten
    );
    s.release_local_read(op)?;
    let other = s.create_task("separate explicit binding")?;
    s.bind_source(other.id, other.revision, &key, version.as_deref(), &ev, 4)?;
    let moved = s.save_local_root("/private/tmp/moved", 0, &[])?;
    let t = s.task(t.id)?;
    let new = import(
        &mut s,
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        &moved,
    )?;
    assert_ne!(new, e);
    assert_ne!(moved.scope(), r.scope());
    Ok(())
}
#[test]
fn exact_exclusions_cover_packed_and_source_metadata_without_prefix_confusion()
-> Result<(), StoreError> {
    let r = root(&[".git/refs/heads/private"]);
    assert!(r.skip_packed());
    assert!(r.excluded(".git/refs/heads/private/x"));
    assert!(!r.excluded(".git/refs/heads/privately"));
    assert!(!r.permits_evidence("refs/heads/private/x"));
    for p in [
        ".git",
        ".git/config",
        ".git/HEAD",
        ".git/packed-refs",
        ".git/refs",
    ] {
        assert!(!root(&[p]).permits_evidence("refs/heads/main"));
    }
    let f = fixture();
    let o = parse_local_git(&r, &f, 1, 2, &[LocalReadReason::Excluded])?;
    assert_eq!(o.completeness, Completeness::Partial);
    assert_eq!(o.branches.len(), 1);
    let mut f = f;
    f.push(LocalGitFile {
        path: ".git/packed-refs".into(),
        bytes: vec![],
    });
    assert!(parse_local_git(&r, &f, 1, 2, &[]).is_err());
    Ok(())
}
#[test]
fn malformed_and_unsupported_inputs_never_claim_empty_complete() -> Result<(), StoreError> {
    for config in [
        "[core]\nrepositoryformatversion=2\n",
        "[core]\nrepositoryformatversion=0\n[include]\npath=/secret\n",
        "[core]\nrepositoryformatversion=1\n[extensions]\nrefstorage=reftable\n",
        "[core]\nrepositoryformatversion=1\n[extensions]\nunknown=true\n",
        "not config",
    ] {
        let mut f = fixture();
        f[0].bytes = config.as_bytes().to_vec();
        let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
        assert_eq!(o.completeness, Completeness::Partial);
        assert!(o.branches.is_empty());
        assert!(!o.reasons.is_empty());
    }
    for packed in [
        format!("{A} refs/heads/z\n{B} refs/heads/a\n"),
        format!("{A} refs/heads/a\n{B} refs/heads/a\n"),
        "broken\n".into(),
    ] {
        let mut f = fixture();
        f.push(LocalGitFile {
            path: ".git/packed-refs".into(),
            bytes: packed.into_bytes(),
        });
        let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
        assert_eq!(o.completeness, Completeness::Partial);
    }
    assert!(parse_local_git(&root(&[]), &fixture(), 2, 1, &[]).is_err());
    Ok(())
}
#[test]
fn sha256_detached_unborn_cycles_and_missing_targets_have_distinct_facts() -> Result<(), StoreError>
{
    let mut f = fixture();
    f[0].bytes = b"[core]\nrepositoryformatversion=1\n[extensions]\nobjectformat=sha256\n".to_vec();
    f[1].bytes = format!("{}\n", "a".repeat(64)).into_bytes();
    f[2].bytes = format!("{}\n", "b".repeat(64)).into_bytes();
    let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
    assert_eq!(o.branches[0].oid, Some("b".repeat(64)));
    assert_eq!(o.branches[0].head, BranchHead::Detached);
    let mut f = fixture();
    f.pop();
    let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
    assert_eq!(
        o.branches,
        vec![LocalBranch {
            full_ref: "refs/heads/main".into(),
            oid: None,
            head: BranchHead::Unborn
        }]
    );
    let o = parse_local_git(&root(&[]), &f, 1, 2, &[LocalReadReason::Unavailable])?;
    assert!(o.branches.is_empty());
    f.push(LocalGitFile {
        path: ".git/refs/heads/main".into(),
        bytes: b"ref: refs/heads/main\n".to_vec(),
    });
    let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
    assert_eq!(o.branches[0].oid, None);
    assert!(o.reasons.contains(&LocalReadReason::Symbolic));
    Ok(())
}
#[test]
fn schema_five_and_wal_are_rejected_without_a_write() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-five.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=5; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO tasks(revision,text) VALUES(1,'preserve');")?;
    let wal = PathBuf::from(format!("{}-wal", db.0.display()));
    let before = std::fs::read(&db.0)?;
    let wal_before = std::fs::read(&wal)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    assert_eq!(std::fs::read(&wal)?, wal_before);
    Ok(())
}
#[test]
fn unborn_head_cannot_exceed_total_branch_limit() -> Result<(), StoreError> {
    let mut f = fixture();
    f.pop();
    f[1].bytes = b"ref: refs/heads/unborn\n".to_vec();
    let packed = (0..256)
        .map(|n| format!("{A} refs/heads/b{n:03}\n"))
        .collect::<String>();
    f.push(LocalGitFile {
        path: ".git/packed-refs".into(),
        bytes: packed.into_bytes(),
    });
    let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
    assert!(o.branches.len() <= 256);
    assert_eq!(o.completeness, Completeness::Partial);
    assert!(o.reasons.contains(&LocalReadReason::Limit));
    Ok(())
}
#[test]
fn source_identity_tracks_location_and_ref_while_oid_refresh_keeps_evidence_id()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("task")?;
    let r = s.save_local_root("/private/tmp/project", 0, &[])?;
    let first = import(
        &mut s,
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        &r,
    )?;
    let task = s.task(t.id)?;
    let op = s.begin_local_read(
        TaskVersion {
            task_id: t.id,
            revision: task.revision,
        },
        &r.path,
        r.revision,
    )?;
    let mut f = fixture();
    f[2].bytes = format!("{B}\n").into_bytes();
    s.finish_local_read(
        op,
        &f,
        LocalReadStats {
            bytes: 256,
            files: 3,
            ..Default::default()
        },
        3,
        4,
        &[],
    )?;
    let (key, version, _) = s.local_branch_evidence(op, 0)?;
    assert_eq!(key.object_id, "refs/heads/main");
    assert_eq!(version.as_deref(), Some(B));
    assert_eq!(key.scope, r.scope());
    assert_eq!(s.bind_local_branch(op, 0, 5)?, first);
    s.release_local_read(op)?;
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 1);
    let task = s.task(t.id)?;
    s.edit_evidence(first, t.id, task.revision, "user correction", 5)?;
    let task = s.task(t.id)?;
    assert_eq!(
        s.ingest_source(
            t.id,
            task.revision,
            &key,
            Some(A),
            &SourceEvidence {
                text: "late history".into(),
                observed_at_utc: Some(1),
                completeness: Completeness::Complete,
                kind: AssertionKind::SourceFact
            }
        )?,
        IngestOutcome::UserProtected
    );
    assert_eq!(s.evidence(t.id, 0, 50)?.items[0].text, "user correction");
    Ok(())
}
#[test]
fn source_settings_limits_validation_and_removed_root_generation_survive_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    for p in ["relative", "/a/../b", "/a//b", "/a/./b", "/a\0b", "/a/"] {
        assert!(s.save_local_root(p, 0, &[]).is_err());
    }
    for p in ["", ".", "..", "a/..", "/a", "a//b", "a/", "a\0b"] {
        assert!(s.save_local_root("/root", 0, &[p.into()]).is_err());
    }
    assert!(
        s.save_local_root("/root", 0, &["same".into(), "same".into()])
            .is_err()
    );
    assert!(
        s.save_local_root(
            "/root",
            0,
            &(0..17).map(|n| format!("p{n}")).collect::<Vec<_>>()
        )
        .is_err()
    );
    assert!(
        s.save_local_root(
            "/root",
            0,
            &(0..16)
                .map(|n| format!("{n}{}", "x".repeat(1021)))
                .collect::<Vec<_>>()
        )
        .is_err()
    );
    let root = s.save_local_root("/root", 0, &["a".into()])?;
    s.delete_local_root(&root.path, root.revision)?;
    drop(s);
    let mut s = Store::open(&db.0)?;
    let recreated = s.save_local_root("/root", 0, &[])?;
    assert!(recreated.revision > root.revision);
    for n in 1..64 {
        s.save_local_root(&format!("/root{n}"), 0, &[])?;
    }
    assert_eq!(
        s.save_local_root("/root64", 0, &[]),
        Err(StoreError::ResourceLimit)
    );
    s.delete_local_root("/root", recreated.revision)?;
    s.save_local_root("/root64", 0, &[])?;
    assert_eq!(s.local_roots()?.len(), 64);
    Ok(())
}
#[test]
fn parser_and_ingress_reject_oversize_and_share_directory_packed_budget()
-> Result<(), Box<dyn std::error::Error>> {
    for (path, size) in [
        (".git/config", 65537),
        (".git/HEAD", 4097),
        (".git/packed-refs", 524289),
        (".git/refs/heads/main", 4097),
    ] {
        let mut f = fixture();
        f.retain(|v| v.path != path);
        f.push(LocalGitFile {
            path: path.into(),
            bytes: vec![b'x'; size],
        });
        assert_eq!(
            parse_local_git(&root(&[]), &f, 1, 2, &[]),
            Err(StoreError::ResourceLimit)
        );
    }
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("task")?;
    let r = s.save_local_root("/root", 0, &[])?;
    let tv = TaskVersion {
        task_id: t.id,
        revision: t.revision,
    };
    let op = s.begin_local_read(tv, &r.path, r.revision)?;
    let mut f = fixture();
    f.push(LocalGitFile {
        path: ".git/packed-refs".into(),
        bytes: format!("{A} refs/heads/packed\n").into_bytes(),
    });
    let o = s.finish_local_read(
        op,
        &f,
        LocalReadStats {
            bytes: 512,
            files: 4,
            directories: 3,
            entries: 4096,
        },
        1,
        2,
        &[],
    )?;
    assert_eq!(o.completeness, Completeness::Partial);
    assert_eq!(o.branches.len(), 1);
    assert!(o.reasons.contains(&LocalReadReason::Limit));
    s.release_local_read(op)?;
    for stats in [
        LocalReadStats {
            bytes: MAX_SOURCE_BYTES + 1,
            files: 3,
            ..Default::default()
        },
        LocalReadStats {
            bytes: 256,
            files: 257,
            ..Default::default()
        },
        LocalReadStats {
            bytes: 256,
            files: 3,
            directories: 257,
            entries: 1,
        },
        LocalReadStats {
            bytes: 256,
            files: 3,
            directories: 1,
            entries: 4097,
        },
    ] {
        let op = s.begin_local_read(tv, &r.path, r.revision)?;
        assert_eq!(
            s.finish_local_read(op, &fixture(), stats, 1, 2, &[]),
            Err(StoreError::ResourceLimit)
        );
        s.release_local_read(op)?;
    }
    Ok(())
}
#[test]
fn task_settings_pause_epoch_and_report_changes_close_read_admission_and_late_binding()
-> Result<(), Box<dyn std::error::Error>> {
    for change in 0..7 {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("task")?;
        let r = s.save_local_root("/root", 0, &[])?;
        let op = s.begin_local_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            &r.path,
            r.revision,
        )?;
        match change {
            0 => {
                s.edit_task(t.id, t.revision, "changed")?;
            }
            1 => {
                s.save_local_root(&r.path, r.revision, &[".git/config".into()])?;
            }
            2 => {
                s.delete_local_root(&r.path, r.revision)?;
                s.save_local_root(&r.path, 0, &[])?;
            }
            3 => s.set_manual_pause(true)?,
            4 => s.set_model_session(43, true, true)?,
            5 => s.set_model_session(42, false, true)?,
            _ => s.set_model_session(42, true, false)?,
        };
        assert!(s.permit_local_read(op, ".git/config", false).is_err());
        assert_eq!(s.release_local_read(op), Err(StoreError::Busy));
        assert!(
            s.finish_local_read(
                op,
                &fixture(),
                LocalReadStats {
                    bytes: 256,
                    files: 3,
                    ..Default::default()
                },
                1,
                2,
                &[]
            )
            .is_err()
        );
        assert!(s.bind_local_branch(op, 0, 3).is_err());
        s.release_local_read(op)?;
        assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    }
    Ok(())
}
#[test]
fn partial_native_loose_walk_never_promotes_a_possibly_overridden_packed_tip()
-> Result<(), StoreError> {
    let mut f = fixture();
    f.pop();
    f.push(LocalGitFile {
        path: ".git/packed-refs".into(),
        bytes: format!("{A} refs/heads/main\n").into_bytes(),
    });
    for reason in [
        LocalReadReason::Unavailable,
        LocalReadReason::Limit,
        LocalReadReason::InvalidName,
    ] {
        let o = parse_local_git(&root(&[]), &f, 1, 2, &[reason])?;
        assert!(o.branches.is_empty());
        assert_eq!(o.completeness, Completeness::Partial);
    }
    Ok(())
}
#[test]
fn symbolic_head_targets_require_utf8_and_the_same_application_name_bounds()
-> Result<(), StoreError> {
    for target in [
        b"refs/heads/\xff".to_vec(),
        format!("refs/heads/{}", "x".repeat(2040)).into_bytes(),
    ] {
        let mut f = fixture();
        f.pop();
        f[1].bytes = [b"ref: ".as_slice(), target.as_slice(), b"\n"].concat();
        let o = parse_local_git(&root(&[]), &f, 1, 2, &[])?;
        assert_eq!(o.completeness, Completeness::Partial);
        assert!(o.reasons.contains(&LocalReadReason::InvalidName));
        assert!(o.branches.is_empty());
    }
    Ok(())
}

#[test]
fn uncited_source_fence_is_inherited_by_reply_before_dispatch_and_copy()
-> Result<(), Box<dyn std::error::Error>> {
    for change in [0, 1, 2] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("intent")?;
        let root = s.save_local_root("/private/tmp/project", 0, &[])?;
        let e = import(
            &mut s,
            TaskVersion {
                task_id: t.id,
                revision: 1,
            },
            &root,
        )?;
        let t = s.task(t.id)?;
        let p = prepare_new(&mut s, t.id, t.revision, &[e])?;
        let a = s.begin_assistance(p.request_id)?;
        let flag = a.cancellation();
        let d = s.dispatch_check(a, 3)?;
        // The model saw the source but cites ONLY the task; no source citation can supply this fence.
        let r = finish_single(
            &mut s,
            d.id(),
            high(),
            Some(AssistanceAnswer {
                proposals: vec![ProposalAnswer {
                    proposal_id: None,
                    kind: AssistanceKind::Clarification,
                    message: "Question about source?".into(),
                    rationale: "Saved intent".into(),
                    evidence_ids: vec![format!("task:{}", t.id.0)],
                    step_id: None,
                }],
                rationale: "Full set".into(),
            }),
        )?
        .ok_or("receipt")?;
        s.release_check(&flag);
        assert!(
            s.proposal_details(r)?
                .citations
                .iter()
                .all(|c| !matches!(c.reference, ProposalCitationRef::Source(_)))
        );
        let p = prepare_reply(&mut s, r, "No")?;
        assert!(p.context.contains("local-git-branch-v1"));
        let a = s.begin_assistance(p.request_id)?;
        let flag = a.cancellation();
        if change == 0 {
            s.delete_local_root(&root.path, root.revision)?;
        }
        if change == 1 {
            s.save_local_root(&root.path, root.revision, &[".git/refs/heads/main".into()])?;
        }
        if change < 2 {
            assert!(s.dispatch_check(a, 4).is_err());
            s.release_check(&flag);
            assert_eq!(s.check_summary()?.attempts, 2);
        } else {
            let d = s.dispatch_check(a, 4)?;
            let reply_id =
                serde_json::from_str::<serde_json::Value>(&p.context)?["replies"][0]["evidence_id"]
                    .as_str()
                    .ok_or("reply id")?
                    .to_owned();
            let r = finish_single(
                &mut s,
                d.id(),
                high(),
                Some(AssistanceAnswer {
                    proposals: vec![ProposalAnswer {
                        proposal_id: None,
                        kind: AssistanceKind::Suggestion,
                        message: "Derived result".into(),
                        rationale: "Temporary reply".into(),
                        evidence_ids: vec![reply_id],
                        step_id: None,
                    }],
                    rationale: "Full set".into(),
                }),
            )?
            .ok_or("derived receipt")?;
            s.release_check(&flag);
            assert_eq!(s.copy_proposal(r)?, "Derived result");
            s.delete_local_root(&root.path, root.revision)?;
            assert!(s.copy_proposal(r).is_err());
            assert!(prepare_reply(&mut s, r, "answer").is_err());
        }
        assert!(s.copy_proposal(r).is_err());
        assert_eq!(s.task(t.id)?.revision, t.revision);
    }
    Ok(())
}

#[test]
fn open_rejects_missing_uncited_source_fence_even_when_workspace_is_stale()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("intent")?;
    let root = s.save_local_root("/private/tmp/project", 0, &[])?;
    let e = import(
        &mut s,
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        &root,
    )?;
    let t = s.task(t.id)?;
    let p = prepare_new(&mut s, t.id, t.revision, &[e])?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_assistance(
        d.id(),
        high(),
        Some(AssistanceAnswer {
            proposals: vec![ProposalAnswer {
                proposal_id: None,
                kind: AssistanceKind::Suggestion,
                message: "derived".into(),
                rationale: "intent".into(),
                evidence_ids: vec![format!("task:{}", t.id.0)],
                step_id: None,
            }],
            rationale: "Full set".into(),
        }),
    )?;
    s.release_check(&flag);
    s.edit_task(t.id, t.revision, "changed")?;
    drop(s);
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute("UPDATE assistant_workspaces SET sources='[]'", [])?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    Ok(())
}
