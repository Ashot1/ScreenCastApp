#[path = "support/assistance.rs"]
pub mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
const ALL: [BuiltInIntegration; 5] = [
    BuiltInIntegration::LocalProjects,
    BuiltInIntegration::Codex,
    BuiltInIntegration::GitHub,
    BuiltInIntegration::Jira,
    BuiltInIntegration::Todoist,
];
const PR: &str = "https://github.com/Example/Project/pull/3";

struct DB(PathBuf);
impl DB {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-switches-{}-{}.sqlite3",
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
fn version(t: &Task) -> TaskVersion {
    TaskVersion {
        task_id: t.id,
        revision: t.revision,
    }
}
fn ok(v: serde_json::Value) -> IntegrationResponse {
    IntegrationResponse {
        status: 200,
        body: serde_json::to_vec(&v).unwrap_or_default(),
        headers: IntegrationHeaders::default(),
    }
}
fn jira_profile(s: &mut Store) -> Result<IntegrationProfile, StoreError> {
    s.save_integration_profile(
        0,
        0,
        "Work",
        Some("Site"),
        &IntegrationConfig::Jira(JiraConfig::default()),
    )
}
fn github_profile(s: &mut Store) -> Result<IntegrationProfile, StoreError> {
    s.save_integration_profile(
        0,
        0,
        "Code",
        None,
        &IntegrationConfig::GitHub(vec![GitHubRepository {
            owner: "Example".into(),
            repository: "Project".into(),
        }]),
    )
}
fn issue(summary: &str) -> serde_json::Value {
    json!({"id":"issue-id","key":"NEW-2","fields":{"summary":summary,"description":null,"subtasks":[],"issuelinks":[],"parent":null}})
}
fn pr() -> serde_json::Value {
    json!({"id":20,"number":3,"title":"Exact title","body":null,"user":{"login":"author"},"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","state":"open","merged":false,"head":{"sha":"opaque-sha"},"base":{"sha":"base","repo":{"id":10,"full_name":"Example/Project"}}})
}
fn begin(
    s: &mut Store,
    t: TaskId,
    p: &IntegrationProfile,
    target: &str,
) -> Result<u64, StoreError> {
    let task = s.task(t)?;
    s.begin_integration_read(version(&task), p.version(), target, 100, 1000)
}
/// Reads one Jira issue window: issue, empty comments, empty history, final issue.
fn jira_window(
    s: &mut Store,
    t: TaskId,
    p: &IntegrationProfile,
    summary: &str,
) -> Result<u64, StoreError> {
    let op = begin(s, t, p, "OLD-1")?;
    s.start_integration_transport(op)?;
    for v in [
        issue(summary),
        json!({"startAt":0,"maxResults":50,"total":0,"comments":[]}),
        json!({"startAt":0,"maxResults":1,"total":0,"values":[]}),
        issue(summary),
    ] {
        s.permit_integration_step(op, 101)?;
        s.accept_integration_response(op, ok(v), 101)?;
    }
    s.finish_integration_transport(op)?;
    Ok(op)
}
fn github_window(s: &mut Store, t: TaskId, p: &IntegrationProfile) -> Result<u64, StoreError> {
    let op = begin(s, t, p, PR)?;
    s.start_integration_transport(op)?;
    for v in [pr(), json!([]), json!([]), json!([]), pr()] {
        s.permit_integration_step(op, 101)?;
        s.accept_integration_response(op, ok(v), 101)?;
    }
    s.finish_integration_transport(op)?;
    Ok(op)
}
fn save_jira(s: &mut Store, t: TaskId, p: &IntegrationProfile) -> Result<EvidenceId, StoreError> {
    let op = jira_window(s, t, p, "Title")?;
    s.integration_preview(op, &[0])?;
    let e = s.bind_integration_objects(op, &[0], 102)?.evidence_ids[0];
    s.release_integration_read(op)?;
    Ok(e)
}
fn local_files() -> Vec<LocalGitFile> {
    [
        (
            ".git/config",
            "[core]\nrepositoryformatversion=0\n".to_owned(),
        ),
        (".git/HEAD", "ref: refs/heads/main\n".to_owned()),
        (
            ".git/refs/heads/main",
            "1111111111111111111111111111111111111111\n".to_owned(),
        ),
    ]
    .into_iter()
    .map(|(p, b)| LocalGitFile {
        path: p.into(),
        bytes: b.into_bytes(),
    })
    .collect()
}
fn save_local(s: &mut Store, t: TaskId, r: &LocalRootSettings) -> Result<EvidenceId, StoreError> {
    let task = s.task(t)?;
    let op = s.begin_local_read(version(&task), &r.path, r.revision)?;
    s.finish_local_read(
        op,
        &local_files(),
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
fn rollout(text: &str) -> Vec<u8> {
    let header=json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"thread-one","cli_version":"0.163.0-alpha.10.2","history_mode":"paginated"}}).to_string();
    let row=json!({"timestamp":"2026-10-20T08:00:01.000Z","ordinal":1,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-one","turn_id":"turn-one","item":{"type":"UserMessage","id":"one","content":[{"type":"text","text":text}]}}}).to_string();
    format!("{header}\n{row}\n").into_bytes()
}
fn codex_read(s: &mut Store, t: TaskId, c: &CodexSelection) -> Result<u64, StoreError> {
    let task = s.task(t)?;
    let op = s.begin_codex_read(version(&task), c.id, c.revision, 0)?;
    let bytes = rollout("exact prompt");
    let n = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or(StoreError::InvalidInput)?
        + 1;
    s.plan_codex_read(
        op,
        CodexFileIdentity {
            device: 1,
            inode: 2,
            size: bytes.len() as u64,
        },
        &bytes[..n],
    )?;
    s.finish_codex_read(op, &bytes, 100)?;
    Ok(op)
}
fn save_codex(s: &mut Store, t: TaskId, c: &CodexSelection) -> Result<EvidenceId, StoreError> {
    let op = codex_read(s, t, c)?;
    let e = s.bind_codex_event(op, 0, 2)?;
    s.release_codex_read(op)?;
    Ok(e)
}
fn prepare(s: &mut Store, t: TaskId, e: EvidenceId) -> Result<AssistancePreview, StoreError> {
    let rev = s.task(t)?.revision;
    prepare_new(s, t, rev, &[e])
}
/// Publishes one clarification that cites only the task, so the source ground stays uncited.
fn publish(
    s: &mut Store,
    t: TaskId,
    e: EvidenceId,
) -> Result<ProposalTarget, Box<dyn std::error::Error>> {
    let p = prepare(s, t, e)?;
    let a = s.begin_assistance(p.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 3)?;
    let target = finish_single(s, d.id(), confirmed(), Some(answer(t, None)))?.ok_or("receipt")?;
    s.release_check(&flag);
    Ok(target)
}
fn workspaces(db: &DB) -> Result<i64, Box<dyn std::error::Error>> {
    let c = Connection::open(&db.0)?;
    Ok(
        c.query_row("SELECT count(*) FROM assistant_workspaces", [], |r| {
            r.get(0)
        })?,
    )
}

#[test]
fn new_database_enables_all_five_and_open_requires_exactly_five_switches()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let s = Store::open(&db.0)?;
    for kind in ALL {
        assert!(s.integration_enabled(kind)?, "{kind:?}");
    }
    drop(s);
    let c = Connection::open(&db.0)?;
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))?,
        16
    );
    c.execute("DELETE FROM integration_switches WHERE kind='todoist'", [])?;
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
fn schema_twelve_is_rejected_without_writing() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let c = Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-twelve.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); INSERT INTO store_identity VALUES(1,randomblob(16)); PRAGMA application_id=1095582257; PRAGMA user_version=12;")?;
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
fn all_four_integrations_refuse_read_admission_while_disabled()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Discuss")?;
    let root = s.save_local_root("/private/tmp/project", 0, &[])?;
    let codex = s.save_codex_selection(0, 0, "/private/tmp/selected.jsonl")?;
    let github = github_profile(&mut s)?;
    let jira = jira_profile(&mut s)?;
    for kind in ALL {
        s.set_integration_enabled(kind, false)?;
        assert!(!s.integration_enabled(kind)?);
    }
    let t = s.task(t.id)?;
    assert_eq!(
        s.begin_local_read(version(&t), &root.path, root.revision),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(
        s.begin_codex_read(version(&t), codex.id, codex.revision, 0),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(
        begin(&mut s, t.id, &github, PR),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(
        begin(&mut s, t.id, &jira, "OLD-1"),
        Err(StoreError::StaleRevision)
    );
    // Settings of a disabled integration can still change.
    s.save_local_root(&root.path, root.revision, &[])?;
    for kind in ALL {
        s.set_integration_enabled(kind, true)?;
    }
    let root = s.local_root(&root.path)?;
    save_local(&mut s, t.id, &root)?;
    save_codex(&mut s, t.id, &codex)?;
    let op = github_window(&mut s, t.id, &github)?;
    s.release_integration_read(op)?;
    save_jira(&mut s, t.id, &jira)?;
    Ok(())
}

#[test]
fn disabling_cancels_reads_closes_window_preview_and_save_for_every_kind()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Discuss")?;
    let root = s.save_local_root("/private/tmp/project", 0, &[])?;
    let codex = s.save_codex_selection(0, 0, "/private/tmp/selected.jsonl")?;
    let github = github_profile(&mut s)?;
    let jira = jira_profile(&mut s)?;

    let task = s.task(t.id)?;
    let local = s.begin_local_read(version(&task), &root.path, root.revision)?;
    let local_flag = s.local_read_cancellation(local)?;
    s.set_integration_enabled(BuiltInIntegration::LocalProjects, false)?;
    assert!(local_flag.load(Ordering::Acquire));
    assert!(s.permit_local_read(local, "", true).is_err());
    s.abort_local_read(local)?;
    s.release_local_read(local)?;

    let op = codex_read(&mut s, t.id, &codex)?;
    s.set_integration_enabled(BuiltInIntegration::Codex, false)?;
    assert!(s.codex_read_cancellation(op)?.load(Ordering::Acquire));
    assert!(s.bind_codex_event(op, 0, 2).is_err());
    s.release_codex_read(op)?;

    let op = github_window(&mut s, t.id, &github)?;
    s.integration_preview(op, &[0])?;
    s.set_integration_enabled(BuiltInIntegration::GitHub, false)?;
    assert!(s.integration_read_cancellation(op)?.load(Ordering::Acquire));
    assert!(s.integration_window_page(op, 0).is_err());
    assert!(s.integration_preview(op, &[0]).is_err());
    assert!(s.bind_integration_objects(op, &[0], 102).is_err());
    s.release_integration_read(op)?;

    let op = begin(&mut s, t.id, &jira, "OLD-1")?;
    s.start_integration_transport(op)?;
    s.permit_integration_step(op, 101)?;
    s.set_integration_enabled(BuiltInIntegration::Jira, false)?;
    assert!(
        s.accept_integration_response(op, ok(issue("Late")), 101)
            .is_err()
    );
    assert!(s.permit_integration_step(op, 102).is_err());
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    Ok(())
}

#[test]
fn late_response_after_disable_and_enable_is_never_accepted()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Discuss")?;
    let jira = jira_profile(&mut s)?;
    let op = begin(&mut s, t.id, &jira, "OLD-1")?;
    s.start_integration_transport(op)?;
    s.permit_integration_step(op, 101)?;
    s.set_integration_enabled(BuiltInIntegration::Jira, false)?;
    s.set_integration_enabled(BuiltInIntegration::Jira, true)?;
    assert!(
        s.accept_integration_response(op, ok(issue("Late")), 101)
            .is_err()
    );
    assert!(s.permit_integration_step(op, 102).is_err());
    assert!(s.integration_read_cancellation(op)?.load(Ordering::Acquire));
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    // A new read after enabling works normally.
    save_jira(&mut s, t.id, &jira)?;
    Ok(())
}

#[test]
fn disabling_clears_dependent_assistance_keeps_facts_and_enabling_restores_use()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Discuss")?;
    let jira = jira_profile(&mut s)?;
    let e = save_jira(&mut s, t.id, &jira)?;
    let target = publish(&mut s, t.id, e)?;
    let pending = prepare_reply(&mut s, target, "Exact user text")?;
    assert_eq!(workspaces(&db)?, 1);
    s.set_integration_enabled(BuiltInIntegration::Jira, false)?;
    assert_eq!(workspaces(&db)?, 0);
    assert!(s.proposal_feed()?.proposals.is_empty());
    assert!(s.copy_proposal(target).is_err());
    assert!(s.begin_assistance(pending.request_id).is_err());
    assert!(prepare(&mut s, t.id, e).is_err());
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 1);
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert!(!s.integration_enabled(BuiltInIntegration::Jira)?);
    assert!(s.proposal_feed()?.proposals.is_empty());
    ready(&mut s)?;
    s.set_integration_enabled(BuiltInIntegration::Jira, true)?;
    // Saved facts are usable again without a new read or key; the cleared dialogue is not restored.
    assert!(s.proposal_feed()?.proposals.is_empty());
    let p = prepare(&mut s, t.id, e)?;
    assert!(p.context.contains("Title"));
    publish(&mut s, t.id, e)?;
    assert_eq!(s.proposal_feed()?.proposals.len(), 1);
    Ok(())
}

#[test]
fn disabling_local_projects_or_codex_during_assistance_prevents_publishing_and_reopen_works()
-> Result<(), Box<dyn std::error::Error>> {
    for kind in [BuiltInIntegration::LocalProjects, BuiltInIntegration::Codex] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("Discuss")?;
        let e = if kind == BuiltInIntegration::LocalProjects {
            let root = s.save_local_root("/private/tmp/project", 0, &[])?;
            save_local(&mut s, t.id, &root)?
        } else {
            let codex = s.save_codex_selection(0, 0, "/private/tmp/selected.jsonl")?;
            save_codex(&mut s, t.id, &codex)?
        };
        let p = prepare(&mut s, t.id, e)?;
        let a = s.begin_assistance(p.request_id)?;
        let flag = a.cancellation();
        let d = s.dispatch_check(a, 3)?;
        s.set_integration_enabled(kind, false)?;
        let published = finish_single(&mut s, d.id(), confirmed(), Some(answer(t.id, None)))?;
        s.release_check(&flag);
        assert!(published.is_none(), "{kind:?}");
        assert!(s.proposal_feed()?.proposals.is_empty(), "{kind:?}");
        drop(s);
        let mut s = Store::open(&db.0)?;
        assert!(s.proposal_feed()?.proposals.is_empty(), "{kind:?}");
        ready(&mut s)?;
        assert!(prepare(&mut s, t.id, e).is_err(), "{kind:?}");
        s.set_integration_enabled(kind, true)?;
        publish(&mut s, t.id, e)?;
        drop(s);
        Store::open(&db.0)?;
    }
    Ok(())
}

#[test]
fn changes_made_while_disabled_make_facts_stale_after_enabling()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Discuss")?;
    let root = s.save_local_root("/private/tmp/project", 0, &[])?;
    let local = save_local(&mut s, t.id, &root)?;
    let codex = s.save_codex_selection(0, 0, "/private/tmp/selected.jsonl")?;
    let codex_fact = save_codex(&mut s, t.id, &codex)?;
    let jira = jira_profile(&mut s)?;
    let jira_fact = save_jira(&mut s, t.id, &jira)?;
    for kind in ALL {
        s.set_integration_enabled(kind, false)?;
    }
    s.save_local_root(&root.path, root.revision, &[".git/refs/heads/main".into()])?;
    s.save_codex_selection(codex.id, codex.revision, "/private/tmp/other.jsonl")?;
    s.replace_integration_credential(jira.id, jira.settings_revision)?;
    for kind in ALL {
        s.set_integration_enabled(kind, true)?;
    }
    for e in [local, codex_fact, jira_fact] {
        assert!(prepare(&mut s, t.id, e).is_err());
    }
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 3);
    Ok(())
}

#[test]
fn setting_the_same_value_changes_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Discuss")?;
    let jira = jira_profile(&mut s)?;
    let e = save_jira(&mut s, t.id, &jira)?;
    publish(&mut s, t.id, e)?;
    let op = begin(&mut s, t.id, &jira, "OLD-1")?;
    // Another connection disables the switch behind the Store: only an actual change would
    // reach clear_revoked and the read invalidation.
    let c = Connection::open(&db.0)?;
    c.execute(
        "UPDATE integration_switches SET enabled=0 WHERE kind='jira'",
        [],
    )?;
    drop(c);
    s.set_integration_enabled(BuiltInIntegration::Jira, false)?;
    assert_eq!(workspaces(&db)?, 1);
    assert!(!s.integration_read_cancellation(op)?.load(Ordering::Acquire));
    s.set_integration_enabled(BuiltInIntegration::GitHub, true)?;
    assert_eq!(workspaces(&db)?, 1);
    s.abort_unused_integration_read(op)?;
    s.release_integration_read(op)?;
    Ok(())
}
