#[path = "support/assistance.rs"]
mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use rusqlite::Connection;

trait JiraSteps {
    fn permit(&mut self, op: u64, now: i64) -> Result<JiraRequest, StoreError>;
    fn save(
        &mut self,
        id: u64,
        expected: u64,
        name: &str,
        cloud: &str,
    ) -> Result<IntegrationProfile, StoreError>;
}
impl JiraSteps for Store {
    fn permit(&mut self, op: u64, now: i64) -> Result<JiraRequest, StoreError> {
        match self.permit_integration_step(op, now)?.request {
            IntegrationRequest::Jira(r) => Ok(r),
            _ => Err(StoreError::InvalidInput),
        }
    }
    fn save(
        &mut self,
        id: u64,
        expected: u64,
        name: &str,
        cloud: &str,
    ) -> Result<IntegrationProfile, StoreError> {
        self.save_integration_profile(
            id,
            expected,
            name,
            Some(cloud),
            &IntegrationConfig::Jira(JiraConfig::default()),
        )
    }
}

#[test]
fn jira_store_uses_schema_sixteen() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-jira-schema-{}.sqlite3",
        std::process::id()
    ));
    let store = Store::open(&path)?;
    drop(store);
    let connection = Connection::open(&path)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, 16);
    drop(connection);
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn jira_rejects_schema_ten_without_writing() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-jira-old-schema-{}.sqlite3",
        std::process::id()
    ));
    let connection = Connection::open(&path)?;
    connection.execute_batch(include_str!("fixtures/schema-ten.sql"))?;
    connection.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=10;")?;
    drop(connection);
    let before = std::fs::read(&path)?;
    assert!(matches!(
        Store::open(&path),
        Err(ambient_core::StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&path)?, before);
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn jira_site_exclusion_survives_profile_deletion_and_bumps_aba_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-jira-profile-{}.sqlite3",
        std::process::id()
    ));
    let mut s = Store::open(&path)?;
    let p = s.save(0, 0, "Work", "Exact-Cloud-ID")?;
    assert!(s.save(0, 0, "Duplicate", "Exact-Cloud-ID").is_err());
    assert!(
        s.save(p.id, p.settings_revision, "Move", "Other-ID")
            .is_err()
    );
    s.set_integration_scope_excluded(IntegrationKind::Jira, "Exact-Cloud-ID", true)?;
    let excluded = s.integration_profile(p.id)?;
    assert!(excluded.settings_revision > p.settings_revision);
    assert_eq!(excluded.credential_generation, p.credential_generation);
    s.set_integration_scope_excluded(IntegrationKind::Jira, "Exact-Cloud-ID", false)?;
    let allowed = s.integration_profile(p.id)?;
    assert!(allowed.settings_revision > excluded.settings_revision);
    assert_eq!(allowed.credential_generation, p.credential_generation);
    s.set_integration_scope_excluded(IntegrationKind::Jira, "Exact-Cloud-ID", true)?;
    let p = s.integration_profile(p.id)?;
    let revoked = s.replace_integration_credential(p.id, p.settings_revision)?;
    assert!(revoked.credential_generation > p.credential_generation);
    s.finish_integration_profile_deletion(
        revoked.id,
        revoked.settings_revision,
        revoked.credential_generation,
    )?;
    let new = s.save(0, 0, "New", "Exact-Cloud-ID")?;
    assert!(new.id > revoked.credential_generation);
    drop(s);
    let s = Store::open(&path)?;
    assert_eq!(
        s.integration_exclusions(IntegrationKind::Jira)?,
        vec!["Exact-Cloud-ID".to_owned()]
    );
    assert_eq!(s.integration_profile(new.id)?, new);
    drop(s);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn generic_source_cannot_bypass_jira_metadata_and_authorization()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-jira-bypass-{}.sqlite3",
        std::process::id()
    ));
    let mut s = Store::open(&path)?;
    let t = s.create_task("Selected issue")?;
    let key = SourceKey {
        namespace: JIRA_NAMESPACE.into(),
        scope: "Site".into(),
        object_id: "opaque".into(),
    };
    let fact = SourceEvidence {
        text: "Jira fact".into(),
        observed_at_utc: None,
        completeness: Completeness::Partial,
        kind: AssertionKind::SourceFact,
    };
    assert_eq!(
        s.bind_source(t.id, t.revision, &key, Some("hash"), &fact, 1),
        Err(StoreError::InvalidInput)
    );
    drop(s);
    std::fs::remove_file(path)?;
    Ok(())
}
fn response(v: serde_json::Value) -> IntegrationResponse {
    IntegrationResponse {
        status: 200,
        body: serde_json::to_vec(&v).unwrap_or_default(),
        headers: IntegrationHeaders::default(),
    }
}
fn issue() -> serde_json::Value {
    serde_json::json!({"id":"issue-id","key":"NEW-2","fields":{"summary":"Initial","description":null,"subtasks":[],"issuelinks":[],"parent":null}})
}
#[test]
fn jira_read_anchors_id_replaces_final_fields_and_atomically_saves()
-> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::temp_dir().join(format!("ambient-jira-read-{}.sqlite3", std::process::id()));
    let mut s = Store::open(&path)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", "Site")?;
    let t = s.create_task("Issue discussion")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    assert_eq!(s.permit(op, 100)?.issue, "OLD-1");
    s.accept_integration_response(op, response(issue()), 100)?;
    let q = s.permit(op, 101)?;
    assert_eq!(q.issue, "issue-id");
    assert_eq!(q.category, 1);
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
        101,
    )?;
    let q = s.permit(op, 102)?;
    assert_eq!(q.category, 2);
    assert_eq!(q.max_results, 1);
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":1,"total":0,"values":[]})),
        102,
    )?;
    assert_eq!(s.permit(op, 103)?.category, 0);
    let mut final_issue = issue();
    final_issue["fields"]["summary"] = serde_json::json!("Final");
    s.accept_integration_response(op, response(final_issue), 103)?;
    assert!(s.integration_preview(op, &[0]).is_err());
    s.finish_integration_transport(op)?;
    let preview = s.integration_preview(op, &[0])?;
    assert!(preview.contains("Final"));
    assert!(!preview.contains("Initial"));
    let result = s.bind_integration_objects(op, &[0], 104)?;
    assert_eq!(result.evidence_ids.len(), 1);
    assert!(result.task.revision > t.revision);
    s.release_integration_read(op)?;
    drop(s);
    let s = Store::open(&path)?;
    assert!(
        s.integration_saved_metadata(IntegrationKind::Jira, t.id, 0)?
            .contains("Site")
    );
    assert_eq!(s.evidence(t.id, 0, 50)?.items[0].text, "Final");
    drop(s);
    std::fs::remove_file(path)?;
    Ok(())
}

struct DB(std::path::PathBuf);
impl DB {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(std::env::temp_dir().join(format!(
            "ambient-jira-flow-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )))
    }
}
impl Drop for DB {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn window(
    s: &mut Store,
    task: TaskId,
    p: &IntegrationProfile,
    summary: &str,
) -> Result<u64, StoreError> {
    let t = s.task(task)?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: task,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(issue()), 100)?;
    s.permit(op, 101)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
        101,
    )?;
    s.permit(op, 102)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":1,"total":0,"values":[]})),
        102,
    )?;
    s.permit(op, 103)?;
    let mut v = issue();
    v["fields"]["summary"] = serde_json::json!(summary);
    s.accept_integration_response(op, response(v), 103)?;
    s.finish_integration_transport(op)?;
    Ok(op)
}
#[test]
fn uncited_jira_grounds_revoke_every_assistance_action_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    for stage in ["prepare", "dispatch", "result", "copy", "reply"] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", "Site")?;
        let t = s.create_task("Discuss")?;
        let op = window(&mut s, t.id, &p, "Title")?;
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
        assert!(preview.context.contains("Title"));
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
fn ordinary_jira_refresh_retains_stale_dialogue_after_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", "Site")?;
    let t = s.create_task("Discuss")?;
    let op = window(&mut s, t.id, &p, "Title")?;
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
    let op = window(&mut s, t.id, &p, "Changed title")?;
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
    s.save(p.id, p.settings_revision, "Renamed", "Site")?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    Ok(())
}

#[test]
fn jira_exclusion_does_not_hide_storage_errors() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let c = Connection::open(&db.0)?;
    c.execute("DROP TABLE integration_exclusions", [])?;
    drop(c);
    assert!(matches!(
        s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", true),
        Err(StoreError::Storage)
    ));
    Ok(())
}
#[test]
fn jira_comment_order_is_verified_and_subsecond_loss_stays_explicit()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", "Site")?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(issue()), 100)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op,response(serde_json::json!({"startAt":0,"maxResults":50,"total":2,"comments":[
 {"id":"c1","body":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"First"}]}]},"created":"2021-01-17T12:00:00.123+0000"},
 {"id":"c2","body":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"Second"}]}]},"created":"2021-01-17T13:00:00+0000"}
 ]})),100)?;
    s.permit(op, 100)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":1,"total":0,"values":[]})),
        100,
    )?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(issue()), 100)?;
    s.finish_integration_transport(op)?;
    let page: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    assert!(
        page["categories"][1]["reasons"]
            .as_array()
            .ok_or("reasons")?
            .iter()
            .any(|v| v == "unstable_order")
    );
    assert!(
        page["objects"][0]["metadata"]["content"]["fields"]["created"]["value"]["precision_lost"]
            == true
    );
    Ok(())
}

#[test]
fn site_exclusion_revokes_all_uncited_jira_grounds_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    for stage in ["prepare", "dispatch", "result", "copy", "reply"] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let p = s.save(0, 0, "Work", "Site")?;
        let t = s.create_task("Discuss")?;
        let op = window(&mut s, t.id, &p, "Title")?;
        s.integration_preview(op, &[0])?;
        let e = s.bind_integration_objects(op, &[0], 102)?.evidence_ids[0];
        s.release_integration_read(op)?;
        if stage == "prepare" {
            s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", true)?;
            let rev = s.task(t.id)?.revision;
            assert!(prepare_new(&mut s, t.id, rev, &[e]).is_err());
            continue;
        }
        let rev = s.task(t.id)?.revision;
        let preview = prepare_new(&mut s, t.id, rev, &[e])?;
        assert!(preview.context.contains("current_state_unverified"));
        assert!(preview.context.contains("Title"));
        let a = s.begin_assistance(preview.request_id)?;
        let flag = a.cancellation();
        if stage == "dispatch" {
            s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", true)?;
            assert!(s.dispatch_check(a, 3).is_err());
            s.release_check(&flag);
            continue;
        }
        let d = s.dispatch_check(a, 3)?;
        if stage == "result" {
            s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", true)?;
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
        s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", true)?;
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

fn comment(id: usize, bytes: usize) -> serde_json::Value {
    serde_json::json!({"id":format!("c{id}"),"body":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"x".repeat(bytes)}]}]},"created":"2021-01-17T12:00:00+0000"})
}
fn start_read(s: &mut Store) -> Result<(IntegrationProfile, Task, u64), StoreError> {
    ready(s)?;
    let p = s.save(0, 0, "Work", "Site")?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(issue()), 100)?;
    Ok((p, t, op))
}
fn finish_categories(
    s: &mut Store,
    op: u64,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    loop {
        let request = s.permit(op, 110)?;
        let v = if request.category == 0 {
            issue()
        } else {
            serde_json::json!({"startAt":request.start_at,"maxResults":request.max_results,"total":0,"values":[]})
        };
        s.accept_integration_response(op, response(v), 110)?;
        if request.category == 0 {
            break;
        }
    }
    s.finish_integration_transport(op)?;
    Ok(serde_json::from_str(&s.integration_window_page(op, 0)?)?)
}
#[test]
fn jira_paging_never_calls_a_byte_limit_a_repeated_page() -> Result<(), Box<dyn std::error::Error>>
{
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let (_, _, op) = start_read(&mut s)?;
    for start in [0, 50] {
        let request = s.permit(op, 101)?;
        if request.category != 1 {
            break;
        }
        s.accept_integration_response(op, response(serde_json::json!({"startAt":start,"maxResults":50,"total":100,"comments":(start..start+50).map(|i|comment(i,5000)).collect::<Vec<_>>()})), 101)?;
    }
    let page = finish_categories(&mut s, op)?;
    let reasons = page["categories"][1]["reasons"]
        .as_array()
        .ok_or("reasons")?;
    assert!(reasons.iter().any(|v| v == "window_byte_limit"));
    assert!(!reasons.iter().any(|v| v == "repeated_page"));
    Ok(())
}
#[test]
fn jira_inconsistent_total_is_partial_and_backward_time_cannot_shrink_interval()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let (_, _, op) = start_read(&mut s)?;
    s.permit(op, 101)?;
    s.accept_integration_response(
        op,
        response(
            serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[comment(0,2)]}),
        ),
        101,
    )?;
    let page = finish_categories(&mut s, op)?;
    assert_eq!(page["categories"][1]["status"], "partial");
    assert!(
        page["categories"][1]["reasons"]
            .as_array()
            .ok_or("reasons")?
            .iter()
            .any(|v| v == "invalid_pagination")
    );
    s.release_integration_read(op)?;
    let p = s.integration_profiles(IntegrationKind::Jira)?[0].clone();
    let t = s.tasks(0, 50)?.items[0].clone();
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 105)?;
    s.accept_integration_response(op, response(issue()), 105)?;
    s.permit(op, 105)?;
    assert!(
        s.accept_integration_response(
            op,
            response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
            104
        )
        .is_err()
    );
    s.finish_integration_transport(op)?;
    assert!(s.integration_preview(op, &[0]).is_err());
    Ok(())
}

#[test]
fn jira_matching_401_revokes_after_task_change_but_cannot_revoke_new_generation()
-> Result<(), Box<dyn std::error::Error>> {
    for replace in [false, true] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (p, t, op) = start_read(&mut s)?;
        s.permit(op, 101)?;
        s.edit_task(t.id, t.revision, "Changed while pending")?;
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
        assert_eq!(
            after.credential_generation == current.credential_generation,
            replace
        );
        assert_eq!(s.release_integration_read(op), Err(StoreError::Busy));
        s.finish_integration_transport(op)?;
        s.release_integration_read(op)?;
        drop(s);
        assert_eq!(Store::open(&db.0)?.integration_profile(p.id)?, after);
    }
    Ok(())
}
#[test]
fn jira_cooldown_stops_final_get_and_survives_restart_without_guessing_unknown_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, retry, ambiguous, expected) in [
        (429, Some("120"), false, Some(221)),
        (503, Some("120"), false, Some(221)),
        (429, None, false, None),
        (429, Some("+120"), false, None),
        (429, Some("99999999999999999999999999"), false, None),
        (503, None, true, None),
        (
            429,
            Some("Sun, 06 Nov 1994 08:49:37 GMT"),
            false,
            Some(784111777),
        ),
    ] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (p, t, op) = start_read(&mut s)?;
        s.permit(op, 101)?;
        assert!(
            s.accept_integration_response(
                op,
                IntegrationResponse {
                    status,
                    body: vec![],
                    headers: IntegrationHeaders {
                        retry_after: retry.map(str::to_owned),
                        rate_ambiguous: ambiguous,
                        ..IntegrationHeaders::default()
                    }
                },
                101
            )
            .is_err()
        );
        assert!(s.permit(op, 102).is_err());
        s.finish_integration_transport(op)?;
        let after = s.integration_profile(p.id)?;
        assert_eq!(after.not_before_utc, expected);
        assert_eq!(after.version(), p.version());
        assert_eq!(after.server_deadline_known, expected.is_some());
        drop(s);
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        assert_eq!(s.integration_profile(p.id)?, after);
        if let Some(until) = expected {
            assert_eq!(
                s.begin_integration_read(
                    TaskVersion {
                        task_id: t.id,
                        revision: t.revision
                    },
                    p.version(),
                    "OLD-1",
                    until - 1,
                    until + 100
                ),
                Err(StoreError::Busy)
            );
        }
    }
    Ok(())
}
#[test]
fn jira_cancel_expiry_session_pause_and_site_aba_keep_transport_busy_until_finish()
-> Result<(), Box<dyn std::error::Error>> {
    for mode in [
        "cancel", "expiry", "session", "report", "pause", "site-aba", "rename", "high",
    ] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (p, t, op) = start_read(&mut s)?;
        s.permit(op, 101)?;
        match mode {
            "cancel" => s.cancel_integration_read(op),
            "expiry" => {
                assert!(s.validate_integration_session_time(op, 1000).is_err());
            }
            "session" => s.set_model_session(10, true, true)?,
            "report" => s.set_model_session(9, true, false)?,
            "pause" => {
                s.set_manual_pause(true)?;
            }
            "site-aba" => {
                s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", true)?;
                s.set_integration_scope_excluded(IntegrationKind::Jira, "Site", false)?;
            }
            "rename" => {
                s.save(p.id, p.settings_revision, "Other name", "Site")?;
            }
            _ => {
                let a = s.prepare_check(true, ModelProfile::High)?;
                let f = a.cancellation();
                let d = s.dispatch_check(a, 1)?;
                s.finish_check(
                    d.id(),
                    ModelCheckResult {
                        outcome: CheckOutcome::Provider,
                        actual: ActualConfig {
                            model: ActualModel::Missing,
                            effort: ActualEffort::Missing,
                            tier: ActualTier::Missing,
                        },
                        usage: None,
                    },
                )?;
                s.release_check(&f);
            }
        }
        assert!(
            s.accept_integration_response(
                op,
                response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
                102
            )
            .is_err(),
            "{mode}"
        );
        assert_eq!(s.release_integration_read(op), Err(StoreError::Busy));
        s.finish_integration_transport(op)?;
        assert!(s.integration_preview(op, &[0]).is_err());
        s.release_integration_read(op)?;
        assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
    }
    Ok(())
}
#[test]
fn jira_category_denial_is_partial_but_final_denial_or_wrong_id_cannot_bind()
-> Result<(), Box<dyn std::error::Error>> {
    for status in [403, 404] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (_, _, op) = start_read(&mut s)?;
        s.permit(op, 101)?;
        s.accept_integration_response(
            op,
            IntegrationResponse {
                status,
                body: vec![],
                headers: IntegrationHeaders::default(),
            },
            101,
        )?;
        let page = finish_categories(&mut s, op)?;
        assert_eq!(page["categories"][1]["status"], "partial");
        assert_eq!(page["objects"].as_array().ok_or("objects")?.len(), 1);
    }
    for status in [200, 403, 404] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (_, _, op) = start_read(&mut s)?;
        for category in [1, 2] {
            let q = s.permit(op, 101)?;
            assert_eq!(q.category, category);
            s.accept_integration_response(op,response(serde_json::json!({"startAt":0,"maxResults":q.max_results,"total":0,if category==1{"comments"}else{"values"}:[]})),101)?;
        }
        s.permit(op, 102)?;
        let mut v = issue();
        v["id"] = serde_json::json!("wrong");
        let mut r = response(v);
        r.status = status;
        assert!(s.accept_integration_response(op, r, 102).is_err());
        s.finish_integration_transport(op)?;
        assert!(s.integration_preview(op, &[0]).is_err());
    }
    Ok(())
}
#[test]
fn jira_offsets_and_thirteen_gets_are_checked_with_final_request_reserved()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let (_, _, op) = start_read(&mut s)?;
    for n in 0..5 {
        let q = s.permit(op, 101)?;
        assert_eq!((q.category, q.start_at), (1, n * 50));
        s.accept_integration_response(op,response(serde_json::json!({"startAt":q.start_at,"maxResults":50,"total":999,"comments":(0..50).map(|i|comment((n*50+i)as usize,1)).collect::<Vec<_>>()})),101)?;
    }
    let q = s.permit(op, 102)?;
    assert_eq!((q.category, q.max_results), (2, 1));
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":1,"total":1000,"values":[{"id":"probe","items":[{"field":"status","fieldtype":"jira","toString":"Done"}]}]})),
        102,
    )?;
    for n in 0..5 {
        let q = s.permit(op, 103)?;
        assert_eq!((q.category, q.start_at), (2, 750 + n * 50));
        s.accept_integration_response(op,response(serde_json::json!({"startAt":q.start_at,"maxResults":50,"total":1000,"values":(0..50).map(|i|serde_json::json!({"id":format!("h{}",n*50+i),"created":"2021-01-17T12:00:00+0000","items":[{"field":"status","fieldtype":"jira","toString":"Done"}]})).collect::<Vec<_>>()})),103)?;
    }
    let q = s.permit(op, 104)?;
    assert_eq!(q.category, 0);
    s.accept_integration_response(op, response(issue()), 104)?;
    s.finish_integration_transport(op)?;
    let page: serde_json::Value = serde_json::from_str(&s.integration_window_page(op, 0)?)?;
    assert_eq!(page["categories"][2]["omitted_prefix"], true);
    assert!(s.permit(op, 105).is_err());
    s.release_integration_read(op)?;
    let p = s.integration_profiles(IntegrationKind::Jira)?[0].clone();
    let t = s.tasks(0, 50)?.items[0].clone();
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, response(issue()), 100)?;
    s.permit(op, 100)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
        100,
    )?;
    s.permit(op, 100)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":1,"total":i64::MAX,"values":[{"id":"probe","items":[{"field":"status","fieldtype":"jira","toString":"Done"}]}]})),
        100,
    )?;
    assert_eq!(s.permit(op, 101)?.category, 0);
    s.accept_integration_response(op, response(issue()), 101)?;
    s.finish_integration_transport(op)?;
    assert!(s.integration_window_page(op, 0)?.contains("offset_limit"));
    Ok(())
}
#[test]
fn jira_preview_batch_is_exact_atomic_and_preserves_manual_edits_and_tombstones()
-> Result<(), Box<dyn std::error::Error>> {
    for protect in ["manual", "deleted", "profile"] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (p, t, op) = start_read(&mut s)?;
        s.permit(op, 101)?;
        s.accept_integration_response(op,response(serde_json::json!({"startAt":0,"maxResults":50,"total":1,"comments":[comment(0,5)]})),101)?;
        finish_categories(&mut s, op)?;
        assert!(s.integration_preview(op, &[]).is_err());
        assert!(s.integration_preview(op, &[0, 0]).is_err());
        assert!(s.integration_preview(op, &[0; 33]).is_err());
        assert!(s.bind_integration_objects(op, &[0, 1], 111).is_err());
        let before = s.task(t.id)?;
        s.integration_preview(op, &[0, 1])?;
        assert_eq!(s.task(t.id)?, before);
        assert!(s.evidence(t.id, 0, 50)?.items.is_empty());
        assert!(s.bind_integration_objects(op, &[1, 0], 111).is_err());
        let saved = s.bind_integration_objects(op, &[0, 1], 111)?;
        s.release_integration_read(op)?;
        let current = match protect {
            "manual" => {
                s.edit_evidence(
                    saved.evidence_ids[1],
                    t.id,
                    saved.task.revision,
                    "User correction",
                    112,
                )?;
                p.clone()
            }
            "deleted" => {
                s.delete_evidence(saved.evidence_ids[1], t.id, saved.task.revision, &[])?;
                p.clone()
            }
            _ => {
                let r = s.replace_integration_credential(p.id, p.settings_revision)?;
                s.finish_integration_profile_deletion(
                    r.id,
                    r.settings_revision,
                    r.credential_generation,
                )?;
                s.save(0, 0, "New", "Site")?
            }
        };
        let before = s.evidence(t.id, 0, 50)?;
        let metadata = s.integration_saved_metadata(IntegrationKind::Jira, t.id, 0)?;
        let rev = s.task(t.id)?.revision;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: rev,
            },
            current.version(),
            "OLD-1",
            100,
            1000,
        )?;
        s.start_integration_transport(op)?;
        s.permit(op, 100)?;
        s.accept_integration_response(op, response(issue()), 100)?;
        s.permit(op, 101)?;
        s.accept_integration_response(op,response(serde_json::json!({"startAt":0,"maxResults":50,"total":1,"comments":[comment(0,7)]})),101)?;
        finish_categories(&mut s, op)?;
        // The issue is the second member; its rejection must roll back the preceding changed comment.
        s.integration_preview(op, &[0, 1])?;
        assert!(
            s.bind_integration_objects(op, &[0, 1], 113).is_err(),
            "{protect}"
        );
        assert_eq!(s.task(t.id)?.revision, rev);
        assert_eq!(s.evidence(t.id, 0, 50)?, before);
        assert_eq!(
            s.integration_saved_metadata(IntegrationKind::Jira, t.id, 0)?,
            metadata
        );
        s.release_integration_read(op)?;
        drop(s);
        Store::open(&db.0)?;
    }
    Ok(())
}
#[test]
fn jira_open_rejects_tampered_content_hash_without_writing()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", "Site")?;
    let t = s.create_task("Discuss")?;
    let op = window(&mut s, t.id, &p, "Title")?;
    s.integration_preview(op, &[0])?;
    s.bind_integration_objects(op, &[0], 111)?;
    s.release_integration_read(op)?;
    drop(s);
    let c = Connection::open(&db.0)?;
    c.execute(
        "UPDATE integration_metadata SET data=json_set(data,'$.content.observed_issue_key','TAMPERED-1')",
        [],
    )?;
    drop(c);
    let before = std::fs::read(&db.0)?;
    assert!(Store::open(&db.0).is_err());
    assert_eq!(std::fs::read(&db.0)?, before);
    Ok(())
}

#[test]
fn jira_body_budget_reserves_final_get_at_eight_actual_mebibytes()
-> Result<(), Box<dyn std::error::Error>> {
    fn padded(v: serde_json::Value) -> IntegrationResponse {
        let mut r = response(v);
        r.body.resize(MAX_INTEGRATION_BODY, b' ');
        r
    }
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save(0, 0, "Work", "Site")?;
    let t = s.create_task("Discuss")?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        p.version(),
        "OLD-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    s.permit(op, 100)?;
    s.accept_integration_response(op, padded(issue()), 100)?;
    for n in 0..5 {
        let q = s.permit(op, 101)?;
        assert_eq!(q.category, 1);
        s.accept_integration_response(op,padded(serde_json::json!({"startAt":q.start_at,"maxResults":50,"total":1000,"comments":[comment(n,1)]})),101)?;
    }
    let q = s.permit(op, 102)?;
    assert_eq!((q.category, q.max_results), (2, 1));
    s.accept_integration_response(op,padded(serde_json::json!({"startAt":0,"maxResults":1,"total":1000,"values":[{"id":"probe","items":[{"field":"status","fieldtype":"jira","toString":"Done"}]}]})),102)?;
    assert_eq!(s.permit(op, 103)?.category, 0);
    s.accept_integration_response(op, padded(issue()), 103)?;
    s.finish_integration_transport(op)?;
    assert!(s.integration_window_page(op, 0)?.contains("read_limit"));
    s.integration_preview(op, &[0])?;
    s.bind_integration_objects(op, &[0], 104)?;
    Ok(())
}

#[test]
fn jira_empty_history_probe_is_partial_and_goes_directly_to_final_get()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let (_, _, op) = start_read(&mut s)?;
    s.permit(op, 101)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
        101,
    )?;
    s.permit(op, 102)?;
    s.accept_integration_response(
        op,
        response(serde_json::json!({"startAt":0,"maxResults":1,"total":2,"values":[]})),
        102,
    )?;
    assert_eq!(s.permit(op, 103)?.category, 0);
    s.accept_integration_response(op, response(issue()), 103)?;
    s.finish_integration_transport(op)?;
    let page = s.integration_window_page(op, 0)?;
    assert!(page.contains("empty_page"));
    Ok(())
}

#[test]
fn jira_nonempty_history_probe_uses_nonnegative_tail_and_reuses_single_item()
-> Result<(), Box<dyn std::error::Error>> {
    for total in [1, 2, 249, 250, 251] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        let (_, _, op) = start_read(&mut s)?;
        s.permit(op, 101)?;
        s.accept_integration_response(
            op,
            response(serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]})),
            101,
        )?;
        s.permit(op, 102)?;
        s.accept_integration_response(op,response(serde_json::json!({"startAt":0,"maxResults":1,"total":total,"values":[{"id":"probe","items":[{"field":"status","fieldtype":"jira","toString":"Done"}]}]})),102)?;
        let q = s.permit(op, 103)?;
        if total == 1 {
            assert_eq!(q.category, 0);
        } else {
            assert_eq!(q.category, 2);
            assert_eq!(q.start_at, (total - 250).max(0));
        }
        s.cancel_integration_read(op);
        s.finish_integration_transport(op)?;
        s.release_integration_read(op)?;
    }
    Ok(())
}
