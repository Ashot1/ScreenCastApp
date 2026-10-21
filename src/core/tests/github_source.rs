#[path = "support/assistance.rs"]
mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
const PR: &str = "https://github.com/Example/Project/pull/3";

trait GitHubSteps {
    fn permit(&mut self, op: u64, now: i64) -> Result<GitHubRequest, StoreError>;
    fn save(
        &mut self,
        id: u64,
        expected: u64,
        name: &str,
        repositories: &[GitHubRepository],
    ) -> Result<IntegrationProfile, StoreError>;
}
impl GitHubSteps for Store {
    fn permit(&mut self, op: u64, now: i64) -> Result<GitHubRequest, StoreError> {
        match self.permit_integration_step(op, now)?.request {
            IntegrationRequest::GitHub(r) => Ok(r),
            _ => Err(StoreError::InvalidInput),
        }
    }
    fn save(
        &mut self,
        id: u64,
        expected: u64,
        name: &str,
        repositories: &[GitHubRepository],
    ) -> Result<IntegrationProfile, StoreError> {
        self.save_integration_profile(
            id,
            expected,
            name,
            None,
            &IntegrationConfig::GitHub(repositories.to_vec()),
        )
    }
}
struct DB(PathBuf);
impl DB {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-github-{}-{}.sqlite3",
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
#[test]
fn generic_source_cannot_bypass_github_profile_and_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let t = s.create_task("Discuss selected objects")?;
    let key = SourceKey {
        namespace: "github-com-rest-v1".into(),
        scope: "7".into(),
        object_id: "review:8".into(),
    };
    let fact = SourceEvidence {
        text: "Review data".into(),
        observed_at_utc: None,
        completeness: Completeness::Partial,
        kind: AssertionKind::SourceFact,
    };
    assert_eq!(
        s.bind_source(t.id, t.revision, &key, Some("digest"), &fact, 1),
        Err(StoreError::InvalidInput)
    );
    assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    Ok(())
}
fn repo() -> GitHubRepository {
    GitHubRepository {
        owner: "Example".into(),
        repository: "Project".into(),
    }
}
fn pr() -> serde_json::Value {
    serde_json::json!({"id":20,"number":3,"title":"Exact title","body":null,"user":{"login":"author"},"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","state":"future_state","merged":false,"head":{"sha":"opaque-sha"},"base":{"sha":"base","repo":{"id":10,"full_name":"Example/Project"}}})
}
fn response(value: serde_json::Value) -> IntegrationResponse {
    IntegrationResponse {
        status: 200,
        body: serde_json::to_vec(&value).unwrap_or_default(),
        headers: IntegrationHeaders::default(),
    }
}
#[test]
fn profile_generations_do_not_reuse_and_settings_do_not_replace_secret()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let a = s.save(0, 0, "Work", &[repo()])?;
    let b = s.save(a.id, a.settings_revision, "Renamed", &[repo()])?;
    assert_ne!(a.settings_revision, b.settings_revision);
    assert_eq!(a.credential_generation, b.credential_generation);
    assert!(
        s.save(a.id, a.settings_revision, "Late", &[repo()])
            .is_err()
    );
    let revoked = s.replace_integration_credential(b.id, b.settings_revision)?;
    assert_ne!(revoked.credential_generation, b.credential_generation);
    s.finish_integration_profile_deletion(
        revoked.id,
        revoked.settings_revision,
        revoked.credential_generation,
    )?;
    let c = s.save(0, 0, "New", &[repo()])?;
    assert!(c.id > revoked.credential_generation);
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(s.integration_profiles(IntegrationKind::GitHub)?, vec![c]);
    Ok(())
}
#[test]
fn complete_window_final_description_batch_save_and_reopen()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss PR")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    assert_eq!(s.permit(op, 101)?.category, 0);
    s.accept_integration_response(op, response(pr()), 100)?;
    assert_eq!(s.permit(op, 101)?.category, 1);
    s.accept_integration_response(op,response(serde_json::json!([{"id":30,"state":"APPROVED","body":"","user":null,"commit_id":null}])),101)?;
    assert_eq!(s.permit(op, 101)?.category, 2);
    s.accept_integration_response(op, response(serde_json::json!([])), 102)?;
    assert_eq!(s.permit(op, 101)?.category, 3);
    s.accept_integration_response(op, response(serde_json::json!([])), 103)?;
    assert_eq!(s.permit(op, 101)?.category, 0);
    let mut final_pr = pr();
    final_pr["title"] = serde_json::json!("Final title");
    s.accept_integration_response(op, response(final_pr), 104)?;
    assert!(s.permit(op, 101).is_err());
    assert!(s.integration_preview(op, &[0, 1]).is_err());
    s.finish_integration_transport(op)?;
    let before = s.task(t.id)?.revision;
    let preview = s.integration_preview(op, &[0, 1])?;
    assert!(preview.contains("Final title"));
    assert!(!preview.contains("Exact title"));
    assert!(preview.contains("APPROVED"));
    assert!(preview.contains("current_state_unverified"));
    assert_eq!(s.task(t.id)?.revision, before);
    assert!(s.integration_preview(op, &[0, 0]).is_err());
    let saved = s.bind_integration_objects(op, &[0, 1], 105)?;
    assert_eq!(saved.evidence_ids.len(), 2);
    assert!(saved.task.revision > before);
    let metadata = s.integration_saved_metadata(IntegrationKind::GitHub, t.id, 0)?;
    assert!(metadata.contains("non_atomic_observation"));
    assert!(s.integration_preview(op, &[0]).is_err());
    s.release_integration_read(op)?;
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 2);
    assert_eq!(
        s.integration_saved_metadata(IntegrationKind::GitHub, t.id, 0)?,
        metadata
    );
    Ok(())
}
fn window(
    s: &mut Store,
    t: TaskId,
    p: &IntegrationProfile,
    items: serde_json::Value,
) -> Result<u64, StoreError> {
    window_at(s, t, p, items, 100)
}
fn window_at(
    s: &mut Store,
    t: TaskId,
    p: &IntegrationProfile,
    items: serde_json::Value,
    start: i64,
) -> Result<u64, StoreError> {
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t,
            revision: s.task(t)?.revision,
        },
        p.version(),
        PR,
        start,
        start + 1000,
    )?;
    s.start_integration_transport(op)?;
    for value in [
        pr(),
        items,
        serde_json::json!([]),
        serde_json::json!([]),
        pr(),
    ] {
        s.permit(op, start + 1)?;
        s.accept_integration_response(op, response(value), start + 1)?;
    }
    s.finish_integration_transport(op)?;
    Ok(op)
}
#[test]
fn save_requires_the_exact_previous_preview() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Selected")?;
    let op = window(&mut s, t.id, &p, serde_json::json!([]))?;
    assert!(
        s.bind_integration_objects(op, &[0], 102).is_err(),
        "a direct Save must not substitute for preview"
    );
    assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    s.integration_preview(op, &[0])?;
    s.bind_integration_objects(op, &[0], 102)?;
    Ok(())
}
#[test]
fn schema_eight_is_rejected_without_mutating_storage() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-eight.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=8;")?;
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
fn changed_task_does_not_save_a_rejected_credential_and_late_401_cannot_revoke_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    for replace in [false, true] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Selected")?;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            p.version(),
            PR,
            100,
            1000,
        )?;
        s.start_integration_transport(op)?;
        s.permit(op, 101)?;
        s.edit_task(t.id, t.revision, "Changed while GET pending")?;
        let current = if replace {
            s.replace_integration_credential(p.id, p.settings_revision)?
        } else {
            p.clone()
        };
        assert!(
            s.accept_integration_response(
                op,
                IntegrationResponse {
                    status: 401,
                    body: vec![],
                    headers: IntegrationHeaders::default()
                },
                102
            )
            .is_err()
        );
        let after = s.integration_profile(p.id)?;
        if replace {
            assert_eq!(after.credential_generation, current.credential_generation);
        } else {
            assert_ne!(after.credential_generation, p.credential_generation);
        }
        assert_eq!(s.release_integration_read(op), Err(StoreError::Busy));
        s.finish_integration_transport(op)?;
        s.release_integration_read(op)?;
        drop(s);
        assert_eq!(
            Store::open(&db.0)?
                .integration_profile(p.id)?
                .credential_generation,
            after.credential_generation
        );
    }
    Ok(())
}
#[test]
fn rate_stop_persists_cooldown_and_never_permits_the_final_get()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, retry, reset, expected) in [
        (403, "120", None, 220),
        (429, "Sun, 06 Nov 1994 08:49:37 GMT", None, 784111777),
        (429, "Sunday, 06-Nov-94 08:49:37 GMT", None, 784111777),
        (429, "Sun Nov  6 08:49:37 1994", None, 784111777),
        (429, "999999999999999999999999999999", None, 160),
        (429, "10", Some("999"), 999),
    ] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Selected")?;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            p.version(),
            PR,
            100,
            1000,
        )?;
        s.start_integration_transport(op)?;
        s.permit(op, 101)?;
        let headers = IntegrationHeaders {
            retry_after: Some(retry.into()),
            reset: reset.map(str::to_owned),
            remaining: reset.map(|_| "0".into()),
            ..Default::default()
        };
        assert!(
            s.accept_integration_response(
                op,
                IntegrationResponse {
                    status,
                    body: vec![],
                    headers
                },
                100
            )
            .is_err()
        );
        assert!(s.permit(op, 101).is_err());
        s.finish_integration_transport(op)?;
        let after = s.integration_profile(p.id)?;
        assert_eq!(after.not_before_utc, Some(expected));
        assert_eq!(after.settings_revision, p.settings_revision);
        assert_eq!(after.credential_generation, p.credential_generation);
        drop(s);
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        assert!(
            s.begin_integration_read(
                TaskVersion {
                    task_id: t.id,
                    revision: t.revision
                },
                p.version(),
                PR,
                expected - 1,
                expected + 100
            )
            .is_err()
        );
    }
    Ok(())
}
#[test]
fn category_nullability_losses_and_conflicting_duplicates_are_visible()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Selected")?;
    let items = serde_json::json!([
        {"id":30,"state":"APPROVED","body":"first","user":null,"commit_id":null},
        {"id":30,"state":"APPROVED","body":"changed","user":null,"commit_id":null},
        {"id":31,"state":"APPROVED","body":"","user":null,"commit_id":null},
        {"id":32,"state":"COMMENTED","body":"lost required nullable user","commit_id":null},
        {"id":33,"state":"COMMENTED","body":"bad time","user":null,"commit_id":null,"submitted_at":null}
    ]);
    let op = window(&mut s, t.id, &p, items)?;
    let page: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    assert_eq!(page["categories"][0]["ambiguous"], 1);
    assert_eq!(page["categories"][0]["malformed"], 2);
    assert_eq!(page["objects"].as_array().ok_or("objects")?.len(), 3);
    assert!(s.integration_preview(op, &[1]).is_err());
    let preview = s.integration_preview(op, &[2])?;
    assert!(preview.contains("empty body"));
    assert!(preview.contains("APPROVED"));
    Ok(())
}
#[test]
fn cancellation_session_expiry_and_pending_step_never_release_busy_early()
-> Result<(), Box<dyn std::error::Error>> {
    for cancel in [false, true] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Selected")?;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            p.version(),
            PR,
            100,
            102,
        )?;
        s.start_integration_transport(op)?;
        s.permit(op, 101)?;
        assert!(s.permit(op, 101).is_err());
        if cancel {
            s.cancel_integration_read(op);
        }
        assert!(
            s.accept_integration_response(op, response(pr()), 102)
                .is_err()
        );
        assert_eq!(s.release_integration_read(op), Err(StoreError::Busy));
        s.finish_integration_transport(op)?;
        s.release_integration_read(op)?;
    }
    Ok(())
}
#[test]
fn pr_url_cannot_normalize_extra_segments_or_whitespace_into_an_allowed_target()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Selected")?;
    let task = TaskVersion {
        task_id: t.id,
        revision: t.revision,
    };
    for raw in [
        "https://github.com/Example/other/../Project/pull/3",
        " https://github.com/Example/Project/pull/3",
        "https://github.com:443/Example/Project/pull/3",
        "https://github.com/Example/Project/pull/3?x=1",
        "https://user@github.com/Example/Project/pull/3",
        "https://github.com/Example/Project/pull/3#comment",
    ] {
        assert_eq!(
            s.begin_integration_read(task, p.version(), raw, 100, 1000),
            Err(StoreError::InvalidInput),
            "{raw}"
        );
    }
    s.begin_integration_read(task, p.version(), PR, 100, 1000)?;
    Ok(())
}
#[test]
fn malformed_identity_response_is_terminal_and_cannot_be_retried()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Selected")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 101)?;
    let mut v = pr();
    v.as_object_mut().ok_or("pr")?.remove("body");
    assert!(s.accept_integration_response(op, response(v), 101).is_err());
    assert!(
        s.permit(op, 101).is_err(),
        "a malformed PR response cannot retry the GET"
    );
    Ok(())
}
#[test]
fn uncited_github_grounds_revoke_every_assistance_action_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    for stage in ["prepare", "dispatch", "result", "copy", "reply"] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Discuss")?;
        let op = window(&mut s, t.id, &p, serde_json::json!([]))?;
        s.integration_preview(op, &[0])?;
        let e = s.bind_integration_objects(op, &[0], 102)?.evidence_ids[0];
        s.release_integration_read(op)?;
        if stage == "prepare" {
            s.replace_integration_credential(p.id, p.settings_revision)?;
            let rev = s.task(t.id)?.revision;
            assert!(prepare_new(&mut s, t.id, rev, &[e]).is_err());
            continue;
        }
        let rev = s.task(t.id)?.revision;
        let preview = prepare_new(&mut s, t.id, rev, &[e])?;
        assert!(preview.context.contains("current_state_unverified"));
        assert!(preview.context.contains("future_state"));
        let a = s.begin_assistance(preview.request_id)?;
        let flag = a.cancellation();
        if stage == "dispatch" {
            s.replace_integration_credential(p.id, p.settings_revision)?;
            assert!(s.dispatch_check(a, 3).is_err());
            s.release_check(&flag);
            continue;
        }
        let d = s.dispatch_check(a, 3)?;
        if stage == "result" {
            s.replace_integration_credential(p.id, p.settings_revision)?;
        }
        let published = finish_single(&mut s, d.id(), confirmed(), Some(answer(t.id, None)))?;
        s.release_check(&flag);
        if stage == "result" {
            assert!(published.is_none());
            continue;
        }
        let target = s.proposal_feed()?.proposals[0].target();
        let pending = if stage == "reply" {
            Some(prepare_reply(&mut s, target, "Exact user text")?)
        } else {
            None
        };
        s.replace_integration_credential(p.id, p.settings_revision)?;
        assert!(s.proposal_feed()?.proposals.is_empty());
        assert!(s.copy_proposal(target).is_err());
        assert!(suppress(&mut s, target).is_err());
        assert!(prepare_reply(&mut s, target, "Late reply").is_err());
        if let Some(p) = pending {
            assert!(s.begin_assistance(p.request_id).is_err());
        }
        drop(s);
        let s = Store::open(&db.0)?;
        assert!(s.proposal_feed()?.proposals.is_empty());
        assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 1);
    }
    Ok(())
}
#[test]
fn ordinary_github_refresh_retains_stale_dialogue_after_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = window(&mut s, t.id, &p, serde_json::json!([]))?;
    s.integration_preview(op, &[0])?;
    let e = s.bind_integration_objects(op, &[0], 102)?.evidence_ids[0];
    s.release_integration_read(op)?;
    let rev = s.task(t.id)?.revision;
    let preview = prepare_new(&mut s, t.id, rev, &[e])?;
    let a = s.begin_assistance(preview.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 3)?;
    s.finish_assistance(d.id(), confirmed(), Some(answer(t.id, None)))?;
    s.release_check(&flag);
    let row = s.proposal_feed()?.proposals[0].clone();
    let preview = prepare_reply(&mut s, row.target(), "Independent user reply")?;
    let a = s.begin_assistance(preview.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 4)?;
    s.finish_assistance(d.id(), confirmed(), Some(answer(t.id, Some(row.id))))?;
    s.release_check(&flag);
    let before = s.assistant_workspace_revision(t.id)?;
    let op = window_at(&mut s, t.id, &p, serde_json::json!([]), 200)?;
    s.integration_preview(op, &[0])?;
    assert_eq!(
        s.bind_integration_objects(op, &[0], 202)?.evidence_ids[0],
        e
    );
    s.release_integration_read(op)?;
    let rows = s.proposal_feed()?.proposals;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].stale);
    assert_eq!(s.assistant_workspace_revision(t.id)?, before);
    assert!(s.copy_proposal(rows[0].target()).is_err());
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
    s.save(p.id, p.settings_revision, "Renamed", &[repo()])?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    Ok(())
}
#[test]
fn one_protected_object_rolls_back_the_entire_batch_and_task_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let reviews = serde_json::json!([{"id":30,"body":"original","state":"COMMENTED","user":null,"commit_id":null}]);
    let op = window(&mut s, t.id, &p, reviews.clone())?;
    s.integration_preview(op, &[0, 1])?;
    let saved = s.bind_integration_objects(op, &[0, 1], 102)?;
    s.release_integration_read(op)?;
    s.edit_evidence(
        saved.evidence_ids[1],
        t.id,
        saved.task.revision,
        "User correction",
        103,
    )?;
    let before = s.evidence(t.id, 0, 50)?;
    let metadata = s.integration_saved_metadata(IntegrationKind::GitHub, t.id, 0)?;
    let revision = s.task(t.id)?.revision;
    let op = window_at(&mut s, t.id, &p, reviews, 200)?;
    s.integration_preview(op, &[0, 1])?;
    assert!(s.bind_integration_objects(op, &[0, 1], 202).is_err());
    assert_eq!(s.task(t.id)?.revision, revision);
    assert_eq!(s.evidence(t.id, 0, 50)?, before);
    assert_eq!(
        s.integration_saved_metadata(IntegrationKind::GitHub, t.id, 0)?,
        metadata
    );
    s.release_integration_read(op)?;
    drop(s);
    Store::open(&db.0)?;
    Ok(())
}
#[test]
fn canonical_content_excludes_local_interval_settings_and_credential_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = window(&mut s, t.id, &p, serde_json::json!([]))?;
    let a: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    s.release_integration_read(op)?;
    let p = s.replace_integration_credential(p.id, p.settings_revision)?;
    let op = window_at(&mut s, t.id, &p, serde_json::json!([]), 500)?;
    let b: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    assert_eq!(a["objects"][0]["version"], b["objects"][0]["version"]);
    assert_ne!(
        a["objects"][0]["metadata"]["interval_end"],
        b["objects"][0]["metadata"]["interval_end"]
    );
    assert_eq!(
        b["objects"][0]["metadata"]["fields"]["state"]["Unknown"],
        "future_state"
    );
    Ok(())
}

#[test]
fn an_explicit_empty_allowlist_stores_settings_but_never_admits_a_read()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Disabled access", &[])?;
    let t = s.create_task("Discuss")?;
    assert!(
        s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision
            },
            p.version(),
            PR,
            100,
            1000
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn inline_and_general_required_nullable_fields_never_collapse_missing_null_and_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let inline = serde_json::json!({"id":40,"body":"exact\ninline","user":null,"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","pull_request_review_id":null,"path":"file","commit_id":"opaque","original_commit_id":"original","original_start_line":null,"start_side":null,"side":"FUTURE","subject_type":"FUTURE"});
    let mut null_line = inline.clone();
    null_line["id"] = serde_json::json!(41);
    null_line["line"] = serde_json::Value::Null;
    let mut missing_user = inline.clone();
    missing_user["id"] = serde_json::json!(42);
    missing_user.as_object_mut().ok_or("inline")?.remove("user");
    let general = serde_json::json!({"id":50,"user":null,"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z"});
    let mut null_body = general.clone();
    null_body["id"] = serde_json::json!(51);
    null_body["body"] = serde_json::Value::Null;
    let mut empty = general.clone();
    empty["id"] = serde_json::json!(52);
    empty["body"] = serde_json::json!("");
    let mut exact = general.clone();
    exact["id"] = serde_json::json!(53);
    exact["body"] = serde_json::json!("general exact");
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    for v in [
        pr(),
        serde_json::json!([]),
        serde_json::json!([inline, null_line, missing_user]),
        serde_json::json!([general, null_body, empty, exact]),
        pr(),
    ] {
        s.permit(op, 101)?;
        s.accept_integration_response(op, response(v), 101)?;
    }
    s.finish_integration_transport(op)?;
    let page: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    assert_eq!(page["categories"][1]["malformed"], 2);
    assert_eq!(page["categories"][2]["malformed"], 1);
    assert_eq!(page["categories"][2]["empty"], 2);
    assert_eq!(page["objects"].as_array().ok_or("objects")?.len(), 3);
    assert_eq!(page["objects"][1]["text"], "exact\ninline");
    assert_eq!(
        page["objects"][1]["metadata"]["fields"]["side"]["Unknown"],
        "FUTURE"
    );
    Ok(())
}
#[test]
fn object_window_pages_preview_counts_and_final_get_share_one_bounded_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    let mut requests = 0;
    loop {
        let r = s.permit(op, 101)?;
        requests += 1;
        if r.category == 0 {
            s.accept_integration_response(op, response(pr()), 101)?;
            if requests > 1 {
                break;
            }
            continue;
        }
        let items=(1..=100).map(|i|serde_json::json!({"id":r.page as i64*1000+i,"state":"COMMENTED","body":"x","user":null,"commit_id":null})).collect::<Vec<_>>();
        let mut response = response(serde_json::Value::Array(items));
        if r.category == 1 {
            response.headers.link = Some(format!(
                "<https://api.github.com/repos/Example/Project/pulls/3/reviews?per_page=100&page={}>; rel=\"next\"",
                r.page + 1
            ));
        } else {
            response.body=serde_json::to_vec(&serde_json::Value::Array((1..=100).map(|i|serde_json::json!({"id":i,"body":"inline","user":null,"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","pull_request_review_id":null,"path":"file","commit_id":"c","original_commit_id":"o"})).collect())).map_err(|_|"json")?;
        }
        s.accept_integration_response(op, response, 101)?;
        assert!(requests < 17);
    }
    s.finish_integration_transport(op)?;
    assert!(requests <= 17);
    let mut after = 0;
    let mut count = 0;
    loop {
        let page = s.integration_window_page(op, after)?;
        assert!(page.len() <= 256 * 1024);
        let value: serde_json::Value = serde_json::from_str(&page)?;
        let objects = value["objects"].as_array().ok_or("objects")?;
        assert!(objects.len() <= 50);
        count += objects.len();
        if let Some(next) = value["next_after"].as_u64() {
            assert!(next as usize > after);
            after = next as usize;
        } else {
            break;
        }
    }
    assert!(count <= 512);
    assert!(count > 400);
    assert!(s.integration_preview(op, &[]).is_err());
    assert!(
        s.integration_preview(op, &(0..33).collect::<Vec<_>>())
            .is_err()
    );
    let preview = s.integration_preview(op, &(0..32).collect::<Vec<_>>())?;
    assert!(preview.len() <= 512 * 1024);
    let saved = s.bind_integration_objects(op, &(0..32).collect::<Vec<_>>(), 102)?;
    assert_eq!(saved.evidence_ids.len(), 32);
    assert_eq!(saved.task.revision, 33);
    Ok(())
}
#[test]
fn metadata_bytes_join_the_global_limit_and_database_open_validation()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = window(&mut s, t.id, &p, serde_json::json!([]))?;
    s.integration_preview(op, &[0])?;
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) SELECT ?1,?2,0,1,2,2 FROM n",rusqlite::params![t.id.0,"x".repeat(16384)])?;
    c.execute("INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,0,1,2,2)",rusqlite::params![t.id.0,"x".repeat(16000)])?;
    assert_eq!(
        s.bind_integration_objects(op, &[0], 102),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(s.task(t.id)?.revision, 1);
    assert_eq!(
        c.query_row("SELECT count(*) FROM integration_metadata", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    c.execute("DELETE FROM evidence", [])?;
    let saved = s.bind_integration_objects(op, &[0], 102)?;
    s.release_integration_read(op)?;
    drop(s);
    c.execute(
        "UPDATE integration_metadata SET data=replace(data,'future_state','tampered')",
        [],
    )?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    assert_eq!(saved.evidence_ids.len(), 1);
    Ok(())
}
#[test]
fn signed_delta_seconds_are_invalid_and_zero_quota_accepts_checked_decimal_syntax()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, remaining, retry, reset, deadline) in [
        (429, None, "+120", None, 160),
        (200, Some("00"), "1", Some("999"), 999),
    ] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Discuss")?;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            p.version(),
            PR,
            100,
            1000,
        )?;
        s.start_integration_transport(op)?;
        s.permit(op, 101)?;
        let mut r = response(pr());
        r.status = status;
        r.headers = IntegrationHeaders {
            retry_after: Some(retry.into()),
            remaining: remaining.map(str::to_owned),
            reset: reset.map(str::to_owned),
            ..Default::default()
        };
        assert!(s.accept_integration_response(op, r, 100).is_err());
        assert_eq!(s.integration_profile(p.id)?.not_before_utc, Some(deadline));
    }
    Ok(())
}
#[test]
fn stale_workspace_rejects_invalid_saved_github_fence_metadata_without_writing()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = window(&mut s, t.id, &p, serde_json::json!([]))?;
    s.integration_preview(op, &[0])?;
    let e = s.bind_integration_objects(op, &[0], 102)?.evidence_ids[0];
    s.release_integration_read(op)?;
    let rev = s.task(t.id)?.revision;
    let preview = prepare_new(&mut s, t.id, rev, &[e])?;
    let a = s.begin_assistance(preview.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 3)?;
    s.finish_assistance(d.id(), confirmed(), Some(answer(t.id, None)))?;
    s.release_check(&flag);
    let op = window_at(&mut s, t.id, &p, serde_json::json!([]), 200)?;
    s.integration_preview(op, &[0])?;
    s.bind_integration_objects(op, &[0], 202)?;
    s.release_integration_read(op)?;
    drop(s);
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute(
        "UPDATE assistant_workspaces SET sources=replace(sources,'future_state',?1)",
        ["x".repeat(257)],
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

#[test]
fn invalid_link_does_not_shorten_a_valid_rate_deadline() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    let mut r = response(pr());
    r.status = 429;
    r.headers.link_ambiguous = true;
    r.headers.retry_after = Some("600".into());
    assert!(s.accept_integration_response(op, r, 100).is_err());
    assert_eq!(s.integration_profile(p.id)?.not_before_utc, Some(700));
    Ok(())
}

#[test]
fn replacing_metadata_counts_the_final_bytes_at_the_global_limit()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let review = |state: String, body: String| serde_json::json!([{"id":8,"state":state,"body":body,"user":null,"commit_id":null}]);
    let op = window(&mut s, t.id, &p, review("s".repeat(256), "x".into()))?;
    s.integration_preview(op, &[1])?;
    s.bind_integration_objects(op, &[1], 102)?;
    s.release_integration_read(op)?;
    let c = rusqlite::Connection::open(&db.0)?;
    let used:i64=c.query_row("SELECT (SELECT sum(length(CAST(text AS BLOB))) FROM tasks)+(SELECT sum(length(CAST(text AS BLOB))) FROM evidence)+(SELECT sum(length(namespace)+length(scope)+length(object_id)+length(version)) FROM source_bindings)+(SELECT sum(length(CAST(data AS BLOB))) FROM integration_metadata)",[],|r|r.get(0))?;
    c.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) SELECT ?1,?2,0,1,2,2 FROM n",rusqlite::params![t.id.0,"x".repeat(16384)])?;
    let remainder = 64 * 1024 * 1024 - used - 4095 * 16384 - 64;
    assert!(remainder > 0 && remainder <= 16384);
    c.execute("INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,0,1,2,2)",rusqlite::params![t.id.0,"x".repeat(remainder as usize)])?;
    let op = window_at(&mut s, t.id, &p, review("s".into(), "x".repeat(129)), 200)?;
    s.integration_preview(op, &[1])?;
    let saved = s.bind_integration_objects(op, &[1], 202)?;
    assert_eq!(saved.evidence_ids.len(), 1);
    Ok(())
}

#[test]
fn final_get_is_reserved_across_seventeen_requests_and_eight_actual_mebibytes()
-> Result<(), Box<dyn std::error::Error>> {
    for padded in [false, true] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Discuss")?;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            p.version(),
            PR,
            100,
            1000,
        )?;
        s.start_integration_transport(op)?;
        let mut gets = 0;
        let mut actual = 0;
        loop {
            let request = s.permit(op, 101)?;
            gets += 1;
            let mut result = response(if request.category == 0 {
                pr()
            } else {
                serde_json::json!([])
            });
            if request.category != 0 {
                let category = if request.category == 3 {
                    "issues"
                } else {
                    "pulls"
                };
                let suffix = if request.category == 1 {
                    "reviews"
                } else {
                    "comments"
                };
                result.headers.link = Some(format!(
                    "<https://api.github.com/repos/Example/Project/{category}/3/{suffix}?page={}&per_page=100>; rel=\"next\"",
                    request.page + 1
                ));
            }
            if padded {
                result.body.resize(MAX_INTEGRATION_BODY, b' ');
            }
            actual += result.body.len();
            s.accept_integration_response(op, result, 101)?;
            if gets > 1 && request.category == 0 {
                break;
            }
            assert!(gets < 17);
        }
        s.finish_integration_transport(op)?;
        assert_eq!(gets, if padded { 8 } else { 17 });
        if padded {
            assert_eq!(actual, 8 * MAX_INTEGRATION_BODY);
        }
        assert!(s.integration_window_page(op, 0)?.contains("opaque-sha"));
    }
    Ok(())
}

#[test]
fn invalid_reset_does_not_discard_a_valid_retry_after() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    let mut r = response(pr());
    r.status = 429;
    r.headers.rate_ambiguous = true;
    r.headers.retry_after = Some("600".into());
    assert!(s.accept_integration_response(op, r, 100).is_err());
    let profile = s.integration_profile(p.id)?;
    assert_eq!(profile.not_before_utc, Some(700));
    assert!(!profile.server_deadline_known);
    Ok(())
}

#[test]
fn category_losses_remain_partial_after_a_later_valid_page()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        PR,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(pr()), 100)?;
    s.permit(op, 100)?;
    let mut r = response(serde_json::json!([{"id":8}]));
    r.headers.link=Some("<https://api.github.com/repos/Example/Project/pulls/3/reviews?per_page=100&page=2>; rel=\"next\"".into());
    s.accept_integration_response(op, r, 100)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(serde_json::json!([])), 100)?;
    for _ in 0..3 {
        let r = s.permit(op, 100)?;
        s.accept_integration_response(
            op,
            response(if r.category == 0 {
                pr()
            } else {
                serde_json::json!([])
            }),
            100,
        )?;
    }
    s.finish_integration_transport(op)?;
    let page: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    assert_eq!(page["categories"][0]["malformed"], 1);
    assert_eq!(page["categories"][0]["status"], "partial");
    Ok(())
}

#[test]
fn another_profile_and_forgotten_binding_cannot_partially_rebind_a_batch()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let other = s.save(0, 0, "Other", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let reviews = serde_json::json!([{"id":8,"state":"COMMENTED","body":"Exact review","user":null,"commit_id":null}]);
    let op = window(&mut s, t.id, &p, reviews.clone())?;
    s.integration_preview(op, &[0, 1])?;
    let saved = s.bind_integration_objects(op, &[0, 1], 102)?;
    s.release_integration_read(op)?;
    let op = window_at(&mut s, t.id, &other, reviews.clone(), 200)?;
    s.integration_preview(op, &[0, 1])?;
    assert_eq!(
        s.bind_integration_objects(op, &[0, 1], 202),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(s.task(t.id)?.revision, saved.task.revision);
    s.release_integration_read(op)?;
    s.delete_evidence(saved.evidence_ids[1], t.id, saved.task.revision, &[])?;
    let revision = s.task(t.id)?.revision;
    let op = window_at(&mut s, t.id, &p, reviews, 300)?;
    s.integration_preview(op, &[0, 1])?;
    assert_eq!(
        s.bind_integration_objects(op, &[0, 1], 302),
        Err(StoreError::Deleted)
    );
    assert_eq!(s.task(t.id)?.revision, revision);
    let c = rusqlite::Connection::open(&db.0)?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM integration_metadata", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}

fn task(t: &Task) -> TaskVersion {
    TaskVersion {
        task_id: t.id,
        revision: t.revision,
    }
}

#[test]
fn github_profile_is_an_integration_row_without_scope_and_with_a_repository_config()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    assert_eq!(p.kind, IntegrationKind::GitHub);
    assert_eq!(p.scope, None);
    let v: serde_json::Value =
        serde_json::from_str(&s.integration_settings_json(IntegrationKind::GitHub)?)?;
    assert_eq!(v["kind"], "github");
    assert_eq!(v["profiles"][0]["scope"], serde_json::Value::Null);
    assert_eq!(
        v["profiles"][0]["config"],
        serde_json::json!([{"owner":"Example","repository":"Project"}])
    );
    let c = rusqlite::Connection::open(&db.0)?;
    let (kind, scope, config): (String, Option<String>, String) = c.query_row(
        "SELECT kind,scope,config FROM integration_profiles WHERE id=?1",
        [p.id as i64],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(
        (kind.as_str(), scope, config.as_str()),
        (
            "github",
            None,
            r#"[{"owner":"Example","repository":"Project"}]"#
        )
    );
    for repositories in [
        vec![repo(); 2],
        vec![
            repo(),
            GitHubRepository {
                owner: "example".into(),
                repository: "PROJECT".into(),
            },
        ],
        vec![GitHubRepository {
            owner: "..".into(),
            repository: "Project".into(),
        }],
        (0..33)
            .map(|i| GitHubRepository {
                owner: "Example".into(),
                repository: format!("Project{i}"),
            })
            .collect(),
    ] {
        assert_eq!(
            s.save(0, 0, "Bad", &repositories),
            Err(StoreError::InvalidInput)
        );
    }
    assert_eq!(
        s.save_integration_profile(
            0,
            0,
            "Scoped",
            Some("10"),
            &IntegrationConfig::GitHub(vec![repo()])
        ),
        Err(StoreError::InvalidInput)
    );
    Ok(())
}

#[test]
fn github_scope_exclusion_is_invalid_input() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    s.save(0, 0, "Work", &[repo()])?;
    assert_eq!(
        s.set_integration_scope_excluded(IntegrationKind::GitHub, "10", true),
        Err(StoreError::InvalidInput)
    );
    assert!(
        s.integration_exclusions(IntegrationKind::GitHub)?
            .is_empty()
    );
    Ok(())
}

#[test]
fn saving_one_github_profile_cancels_only_reads_of_that_profile()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let reading = s.save(0, 0, "Reading", &[repo()])?;
    let other = s.save(0, 0, "Other", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(task(&t), reading.version(), PR, 100, 1000)?;
    s.start_integration_transport(op)?;
    s.save(other.id, other.settings_revision, "Renamed", &[repo()])?;
    assert_eq!(s.permit(op, 101)?.category, 0);
    s.accept_integration_response(op, response(pr()), 101)?;
    s.save(reading.id, reading.settings_revision, "Renamed", &[repo()])?;
    assert_eq!(s.permit(op, 101), Err(StoreError::Cancelled));
    s.finish_integration_transport(op)?;
    assert_eq!(s.integration_read_status(op)?, 499);
    Ok(())
}

#[test]
fn github_pause_known_flag_follows_the_winning_deadline() -> Result<(), Box<dyn std::error::Error>>
{
    for (first_ambiguous, second_ambiguous) in [(true, false), (false, true)] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", &[repo()])?;
        let t = s.create_task("Discuss")?;
        let mut paused = |begin: i64, at: i64, ambiguous: bool| -> Result<_, StoreError> {
            let op = s.begin_integration_read(task(&t), p.version(), PR, begin, begin + 1000)?;
            s.start_integration_transport(op)?;
            s.permit(op, begin)?;
            let mut r = response(pr());
            r.status = 429;
            r.headers.retry_after = Some("600".into());
            r.headers.rate_ambiguous = ambiguous;
            assert!(s.accept_integration_response(op, r, at).is_err());
            s.finish_integration_transport(op)?;
            s.release_integration_read(op)?;
            let after = s.integration_profile(p.id)?;
            Ok((after.not_before_utc, after.server_deadline_known))
        };
        assert_eq!(
            paused(100, 100, first_ambiguous)?,
            (Some(700), !first_ambiguous)
        );
        // A later response whose clock regressed computes an earlier deadline; the stored
        // deadline wins and keeps its own trust flag.
        assert_eq!(
            paused(800, 50, second_ambiguous)?,
            (Some(700), !first_ambiguous)
        );
    }
    Ok(())
}

#[test]
fn github_begin_reports_the_first_error_in_canonical_order()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", &[repo()])?;
    let t = s.create_task("Discuss")?;
    let stale = IntegrationProfileVersion {
        settings_revision: p.settings_revision + 100,
        ..p.version()
    };
    let other = "https://github.com/Example/Other/pull/3";
    // Admission (allowlist) precedes the profile version check.
    assert_eq!(
        s.begin_integration_read(task(&t), stale, other, 100, 1000),
        Err(StoreError::InvalidInput)
    );
    // The target is parsed before the version check.
    assert_eq!(
        s.begin_integration_read(task(&t), stale, "not a url", 100, 1000),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        s.begin_integration_read(task(&t), stale, PR, 100, 1000),
        Err(StoreError::StaleRevision)
    );
    let op = s.begin_integration_read(task(&t), p.version(), PR, 100, 1000)?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    let mut r = response(pr());
    r.status = 429;
    assert!(s.accept_integration_response(op, r, 100).is_err());
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    // A paused profile reports the disallowed repository first.
    assert_eq!(
        s.begin_integration_read(task(&t), p.version(), other, 101, 1000),
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        s.begin_integration_read(task(&t), p.version(), PR, 101, 1000),
        Err(StoreError::Busy)
    );
    Ok(())
}

#[test]
fn github_store_rejects_schema_eleven_without_writing() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let c = rusqlite::Connection::open(&db.0)?;
    c.execute_batch(include_str!("fixtures/schema-eleven.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); INSERT INTO store_identity VALUES(1,randomblob(16)); PRAGMA application_id=1095582257; PRAGMA user_version=11;")?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(matches!(
        Store::open(&db.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&db.0)?, before);
    Ok(())
}
