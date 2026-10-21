use ambient_core::*;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct DB(PathBuf);
impl DB {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-integration-{}-{}.sqlite3",
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
fn jira() -> IntegrationConfig {
    IntegrationConfig::Jira(JiraConfig::default())
}
fn ok(v: serde_json::Value) -> IntegrationResponse {
    IntegrationResponse {
        status: 200,
        body: serde_json::to_vec(&v).unwrap_or_default(),
        headers: IntegrationHeaders::default(),
    }
}
fn issue() -> serde_json::Value {
    serde_json::json!({"id":"issue-id","key":"NEW-2","fields":{"summary":"Summary","description":null,"subtasks":[],"issuelinks":[],"parent":null}})
}
fn pause(status: u16, retry: Option<&str>, ambiguous: bool) -> IntegrationResponse {
    IntegrationResponse {
        status,
        body: vec![],
        headers: IntegrationHeaders {
            retry_after: retry.map(str::to_owned),
            rate_ambiguous: ambiguous,
            ..IntegrationHeaders::default()
        },
    }
}
fn begin(s: &mut Store, p: &IntegrationProfile, task: &Task, now: i64) -> Result<u64, StoreError> {
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: task.id,
            revision: task.revision,
        },
        p.version(),
        "OLD-1",
        now,
        now + 1000,
    )?;
    s.start_integration_transport(op)?;
    Ok(op)
}

fn instance(s: &Store) -> Result<String, Box<dyn std::error::Error>> {
    let v: serde_json::Value =
        serde_json::from_str(&s.integration_settings_json(IntegrationKind::Jira)?)?;
    Ok(v["instance"].as_str().ok_or("instance")?.to_owned())
}

#[test]
fn store_instance_is_random_stable_hex_and_required_at_open()
-> Result<(), Box<dyn std::error::Error>> {
    let a = DB::new();
    let b = DB::new();
    let first = instance(&Store::open(&a.0)?)?;
    assert_eq!(first.len(), 32);
    assert!(
        first
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    );
    assert_eq!(instance(&Store::open(&a.0)?)?, first);
    assert_ne!(instance(&Store::open(&b.0)?)?, first);
    let c = Connection::open(&a.0)?;
    c.execute("DELETE FROM store_identity", [])?;
    drop(c);
    let before = std::fs::read(&a.0)?;
    assert!(matches!(
        Store::open(&a.0),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&a.0)?, before);
    Ok(())
}

#[test]
fn settings_envelope_carries_kind_instance_nested_config_scope_and_exclusions()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let p = s.save_integration_profile(0, 0, "Work", Some("Site-1"), &jira())?;
    s.set_integration_scope_excluded(IntegrationKind::Jira, "Other-Site", true)?;
    let v: serde_json::Value =
        serde_json::from_str(&s.integration_settings_json(IntegrationKind::Jira)?)?;
    assert_eq!(v["kind"], "jira");
    assert_eq!(v["instance"].as_str().map(str::len), Some(32));
    assert_eq!(v["exclusions"], serde_json::json!(["Other-Site"]));
    assert_eq!(v["profiles"][0]["id"], p.id);
    assert_eq!(v["profiles"][0]["scope"], "Site-1");
    assert_eq!(v["profiles"][0]["config"], serde_json::json!({}));
    for n in 2..=8 {
        s.save_integration_profile(0, 0, "More", Some(&format!("Site-{n}")), &jira())?;
    }
    assert_eq!(
        s.save_integration_profile(0, 0, "Ninth", Some("Site-9"), &jira()),
        Err(StoreError::ResourceLimit)
    );
    assert_eq!(
        s.set_integration_scope_excluded(IntegrationKind::Jira, "bad site", true),
        Err(StoreError::InvalidInput)
    );
    Ok(())
}

#[test]
fn saving_by_an_existing_id_of_another_kind_is_invalid_input()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    let p = s.save_integration_profile(0, 0, "Work", Some("Site-1"), &jira())?;
    let c = Connection::open(&db.0)?;
    c.execute(
        "INSERT INTO integration_profiles VALUES(?1,'github',?1,?1,'Other',NULL,'[]',NULL,0)",
        [p.id as i64 + 1],
    )?;
    drop(c);
    assert_eq!(
        s.save_integration_profile(p.id + 1, p.id + 1, "Work", Some("Site-2"), &jira()),
        Err(StoreError::InvalidInput)
    );
    Ok(())
}

#[test]
fn second_pause_keeps_the_later_deadline_and_its_known_flag()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save_integration_profile(0, 0, "Work", Some("Site"), &jira())?;
    let t = s.create_task("Discuss")?;
    let paused = |s: &mut Store, now: i64, r: IntegrationResponse| -> Result<_, StoreError> {
        let op = begin(s, &p, &t, now)?;
        s.permit_integration_step(op, now)?;
        assert!(s.accept_integration_response(op, r, now).is_err());
        s.finish_integration_transport(op)?;
        s.release_integration_read(op)?;
        let after = s.integration_profile(p.id)?;
        assert_eq!(after.version(), p.version());
        Ok((after.not_before_utc, after.server_deadline_known))
    };
    assert_eq!(
        paused(&mut s, 100, pause(429, Some("120"), false))?,
        (Some(220), true)
    );
    // A response without a deadline leaves the stored deadline and its trust unchanged.
    assert_eq!(
        paused(&mut s, 230, pause(503, None, true))?,
        (Some(220), true)
    );
    // An earlier deadline loses to the stored one.
    assert_eq!(
        paused(
            &mut s,
            231,
            pause(429, Some("Thu, 01 Jan 1970 00:01:40 GMT"), false)
        )?,
        (Some(220), true)
    );
    assert_eq!(
        paused(&mut s, 232, pause(429, Some("100"), false))?,
        (Some(332), true)
    );
    Ok(())
}

#[test]
fn github_and_jira_windows_coexist_and_saving_one_keeps_the_other()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let j = s.save_integration_profile(0, 0, "Work", Some("Site"), &jira())?;
    let repo = GitHubRepository {
        owner: "Example".into(),
        repository: "Project".into(),
    };
    let g =
        s.save_integration_profile(0, 0, "Work", None, &IntegrationConfig::GitHub(vec![repo]))?;
    let t = s.create_task("Discuss both")?;
    let jira_op = begin(&mut s, &j, &t, 100)?;
    let github_op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: t.revision,
        },
        g.version(),
        "https://github.com/Example/Project/pull/3",
        100,
        1100,
    )?;
    s.start_integration_transport(github_op)?;
    let pr = serde_json::json!({"id":20,"number":3,"title":"Title","body":null,"user":{"login":"author"},"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","state":"open","merged":false,"head":{"sha":"opaque"},"base":{"sha":"base","repo":{"id":10,"full_name":"Example/Project"}}});
    let github_pages = [
        pr.clone(),
        serde_json::json!([]),
        serde_json::json!([]),
        serde_json::json!([]),
        pr,
    ];
    let jira_pages = [
        issue(),
        serde_json::json!({"startAt":0,"maxResults":50,"total":0,"comments":[]}),
        serde_json::json!({"startAt":0,"maxResults":1,"total":0,"values":[]}),
        issue(),
    ];
    for (i, v) in github_pages.into_iter().enumerate() {
        if let Some(v) = jira_pages.get(i) {
            s.permit_integration_step(jira_op, 101)?;
            s.accept_integration_response(jira_op, ok(v.clone()), 101)?;
        }
        s.permit_integration_step(github_op, 101)?;
        s.accept_integration_response(github_op, ok(v), 101)?;
    }
    s.finish_integration_transport(jira_op)?;
    s.finish_integration_transport(github_op)?;
    s.integration_preview(jira_op, &[0])?;
    s.integration_preview(github_op, &[0])?;
    let saved = s.bind_integration_objects(jira_op, &[0], 102)?;
    s.release_integration_read(jira_op)?;
    // The task revision moved, so the GitHub window is now stale but was not displaced by Jira.
    assert_eq!(
        s.integration_window_page(github_op, 0),
        Err(StoreError::Cancelled)
    );
    assert_eq!(s.integration_read_status(github_op)?, 499);
    s.release_integration_read(github_op)?;
    assert_eq!(saved.evidence_ids.len(), 1);
    Ok(())
}
