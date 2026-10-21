#[path = "support/assistance.rs"]
pub mod assistance_support;
use ambient_core::*;
use assistance_support::*;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct DB(PathBuf);
impl DB {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-todoist-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    /// Every database file a rejected or completed operation could have written.
    fn bytes(&self) -> Vec<u8> {
        ["", "-journal", "-wal", "-shm"]
            .iter()
            .flat_map(|suffix| {
                std::fs::read(format!("{}{suffix}", self.0.display())).unwrap_or_default()
            })
            .collect()
    }
}
impl Drop for DB {
    fn drop(&mut self) {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}
fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}
fn todoist() -> IntegrationConfig {
    IntegrationConfig::Todoist(TodoistConfig::default())
}
fn ok(v: Value) -> IntegrationResponse {
    IntegrationResponse {
        status: 200,
        body: serde_json::to_vec(&v).unwrap_or_default(),
        headers: IntegrationHeaders::default(),
    }
}
/// A response body of exactly the 1 MiB per-body limit, padded with an ignored field.
fn full(mut v: Value) -> Result<IntegrationResponse, Box<dyn std::error::Error>> {
    v["padding"] = json!("");
    let room = MAX_INTEGRATION_BODY - serde_json::to_vec(&v)?.len();
    v["padding"] = json!("x".repeat(room));
    let r = ok(v);
    assert_eq!(r.body.len(), MAX_INTEGRATION_BODY);
    Ok(r)
}
fn status(status: u16) -> IntegrationResponse {
    IntegrationResponse {
        status,
        body: Vec::new(),
        headers: IntegrationHeaders::default(),
    }
}
/// `/user` carries a token, an email and a websocket URL; none of them may be kept.
fn user(id: Value) -> Value {
    json!({"id":id,"email":"canary-email@example.invalid","full_name":"Canary Name","token":"CANARY-USER-TOKEN","websocket_url":"wss://canary.invalid/ws"})
}
fn task(content: &str) -> Value {
    json!({"id":"task-1","user_id":"owner-9","project_id":"project-1","content":content,"description":"","parent_id":null,"section_id":null,"added_by_uid":"owner-9","assigned_by_uid":null,"responsible_uid":null,"completed_by_uid":null,"checked":false,"is_deleted":false,"added_at":"2026-10-20T10:00:00Z","updated_at":"2026-10-20T11:00:00.5Z","completed_at":null,"due":null,"deadline":null,"duration":null,"labels":[],"priority":1})
}
fn project() -> Value {
    json!({"id":"project-1","name":"Home","description":"","parent_id":null,"is_archived":false,"is_deleted":false,"created_at":"2026-01-01T00:00:00Z","updated_at":null,"color":"red"})
}
fn comment(id: &str, content: Value) -> Value {
    json!({"id":id,"content":content,"posted_uid":"owner-9","posted_at":"2026-10-20T12:00:00Z","is_deleted":false,"file_attachment":null,"reactions":null,"uids_to_notify":null})
}
fn page(results: Vec<Value>, cursor: Value) -> Value {
    json!({"results":results,"next_cursor":cursor})
}
fn permit(s: &mut Store, op: u64, now: i64) -> Result<TodoistRequest, StoreError> {
    match s.permit_integration_step(op, now)?.request {
        IntegrationRequest::Todoist(r) => Ok(r),
        _ => Err(StoreError::InvalidInput),
    }
}
fn begin(s: &mut Store, p: &IntegrationProfile, t: TaskId) -> Result<u64, StoreError> {
    let task = s.task(t)?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t,
            revision: task.revision,
        },
        p.version(),
        "task-1",
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    Ok(op)
}
/// Sends each response for the next permitted request and returns the requests made.
fn script(
    s: &mut Store,
    op: u64,
    responses: Vec<IntegrationResponse>,
) -> Result<Vec<TodoistRequest>, StoreError> {
    let mut requests = Vec::new();
    for (i, r) in responses.into_iter().enumerate() {
        requests.push(permit(s, op, 100 + i as i64)?);
        s.accept_integration_response(op, r, 100 + i as i64)?;
    }
    Ok(requests)
}
fn standard(account: &str, comments: Vec<Value>) -> Vec<IntegrationResponse> {
    vec![
        ok(user(json!(account))),
        ok(task("Initial")),
        ok(project()),
        ok(page(comments, Value::Null)),
        ok(task("Final **Markdown** [link](https://example.invalid)")),
    ]
}
fn window(s: &Store, op: u64) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&s.integration_window_page(op, 0)?)?)
}
fn reasons(w: &Value, category: usize) -> Vec<String> {
    w["categories"][category]["reasons"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn first_read_pins_the_account_once_and_keeps_only_its_id() -> Result<(), Box<dyn std::error::Error>>
{
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    assert_eq!(p.scope, None);
    let t = s.create_task("Selected Todoist task")?;
    let op = begin(&mut s, &p, t.id)?;
    assert_eq!(permit(&mut s, op, 100)?, TodoistRequest::User);
    s.accept_integration_response(op, ok(user(json!("account-1"))), 100)?;
    let pinned = s.integration_profile(p.id)?;
    assert_eq!(pinned.scope.as_deref(), Some("account-1"));
    assert_eq!(pinned.version(), p.version());
    assert!(!s.integration_read_cancellation(op)?.load(Ordering::Acquire));
    assert_eq!(
        permit(&mut s, op, 101)?,
        TodoistRequest::Task {
            id: "task-1".into()
        }
    );
    // The UI still holds the pre-pin revision: a rename passes, with the stored account only.
    assert_eq!(
        s.save_integration_profile(p.id, p.settings_revision, "Other", None, &todoist())
            .err(),
        Some(StoreError::InvalidInput)
    );
    let renamed = s.save_integration_profile(
        p.id,
        p.settings_revision,
        "Renamed",
        Some("account-1"),
        &todoist(),
    )?;
    assert_eq!(renamed.scope.as_deref(), Some("account-1"));
    // The rename changed the settings fence, so this read ends; a new read uses the pinned profile.
    assert!(
        s.accept_integration_response(op, ok(task("Initial")), 101)
            .is_err()
    );
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    let op = begin(&mut s, &renamed, t.id)?;
    let requests = script(
        &mut s,
        op,
        standard("account-1", vec![comment("c-1", json!("Note"))]),
    )?;
    assert_eq!(
        requests,
        vec![
            TodoistRequest::User,
            TodoistRequest::Task {
                id: "task-1".into()
            },
            TodoistRequest::Project {
                id: "project-1".into()
            },
            TodoistRequest::Comments {
                task_id: "task-1".into(),
                cursor: None
            },
            // D67: the read ends with the task, without a second `/user`.
            TodoistRequest::Task {
                id: "task-1".into()
            },
        ]
    );
    s.finish_integration_transport(op)?;
    assert_eq!(s.integration_read_status(op)?, 200);
    let w = window(&s, op)?;
    assert_eq!(w["objects"].as_array().map(Vec::len), Some(3));
    s.integration_preview(op, &[0, 1, 2])?;
    let saved = s.bind_integration_objects(op, &[0, 1, 2], 200)?;
    s.release_integration_read(op)?;
    assert_eq!(saved.evidence_ids.len(), 3);
    assert_eq!(
        s.integration_profile(p.id)?.scope.as_deref(),
        Some("account-1")
    );
    let metadata = s.integration_saved_metadata(IntegrationKind::Todoist, t.id, 0)?;
    assert!(metadata.contains("\"account\":\"account-1\""));
    drop(s);
    let bytes = db.bytes();
    for canary in [
        "canary-email@example.invalid",
        "CANARY-USER-TOKEN",
        "wss://canary.invalid",
        "Canary Name",
    ] {
        assert!(!contains(&bytes, canary), "{canary}");
        assert!(!metadata.contains(canary), "{canary}");
    }
    // Saved facts, metadata, hashes and sizes are validated again at open.
    let s = Store::open(&db.0)?;
    assert_eq!(s.evidence(t.id, 0, 50)?.items.len(), 3);
    Ok(())
}

#[test]
fn excluded_duplicate_or_malformed_accounts_fail_with_409_before_task_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let first = s.save_integration_profile(0, 0, "First", None, &todoist())?;
    let op = begin(&mut s, &first, t.id)?;
    script(&mut s, op, vec![ok(user(json!("account-1")))])?;
    s.cancel_integration_read(op);
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    s.set_integration_scope_excluded(IntegrationKind::Todoist, "excluded", true)?;
    let second = s.save_integration_profile(0, 0, "Second", None, &todoist())?;
    for (account, outcome) in [
        (json!("excluded"), 409),
        (json!("account-1"), 409),
        (json!("a".repeat(129)), 409),
        (json!("bad\naccount"), 409),
        (json!(""), 409),
        (json!(42), 422),
    ] {
        let op = begin(&mut s, &second, t.id)?;
        permit(&mut s, op, 100)?;
        assert!(
            s.accept_integration_response(op, ok(user(account.clone())), 100)
                .is_err()
        );
        assert_eq!(s.integration_read_status(op)?, outcome, "{account}");
        // No task request follows the refused identity.
        assert!(permit(&mut s, op, 101).is_err());
        s.finish_integration_transport(op)?;
        s.release_integration_read(op)?;
        assert_eq!(s.integration_profile(second.id)?, second, "{account}");
    }
    assert_eq!(
        s.integration_profile(first.id)?.scope.as_deref(),
        Some("account-1")
    );
    Ok(())
}

#[test]
fn pinned_profile_rejects_another_account_and_a_later_exclusion_cancels_the_read()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    script(&mut s, op, vec![ok(user(json!("account-1")))])?;
    // The account written by this read can be excluded later: the revision moves and the read stops.
    s.set_integration_scope_excluded(IntegrationKind::Todoist, "account-1", true)?;
    assert!(s.integration_read_cancellation(op)?.load(Ordering::Acquire));
    assert!(permit(&mut s, op, 101).is_err());
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    s.set_integration_scope_excluded(IntegrationKind::Todoist, "account-1", false)?;
    let pinned = s.integration_profile(p.id)?;
    let op = begin(&mut s, &pinned, t.id)?;
    permit(&mut s, op, 100)?;
    assert!(
        s.accept_integration_response(op, ok(user(json!("account-2"))), 100)
            .is_err()
    );
    assert_eq!(s.integration_read_status(op)?, 409);
    assert!(permit(&mut s, op, 101).is_err());
    s.finish_integration_transport(op)?;
    assert_eq!(s.integration_profile(p.id)?, pinned);
    Ok(())
}

#[test]
fn cancellation_before_the_user_response_writes_nothing_and_stays_set()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    permit(&mut s, op, 100)?;
    s.cancel_integration_read(op);
    assert_eq!(
        s.accept_integration_response(op, ok(user(json!("account-1"))), 100),
        Err(StoreError::Cancelled)
    );
    assert!(s.integration_read_cancellation(op)?.load(Ordering::Acquire));
    assert_eq!(s.integration_profile(p.id)?.scope, None);
    Ok(())
}

#[test]
fn a_storage_error_while_pinning_is_not_turned_into_409() -> Result<(), Box<dyn std::error::Error>>
{
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    permit(&mut s, op, 100)?;
    let c = Connection::open(&db.0)?;
    c.execute_batch("CREATE TRIGGER refuse BEFORE UPDATE OF scope ON integration_profiles BEGIN SELECT RAISE(ABORT, 'storage refused'); END;")?;
    drop(c);
    assert_eq!(
        s.accept_integration_response(op, ok(user(json!("account-1"))), 100),
        Err(StoreError::Storage)
    );
    assert_ne!(s.integration_read_status(op)?, 409);
    assert_eq!(s.integration_profile(p.id)?.scope, None);
    Ok(())
}

#[test]
fn nine_requests_cover_five_comment_pages_and_the_final_task()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    let mut responses = vec![
        ok(user(json!("account-1"))),
        ok(task("Initial")),
        ok(project()),
    ];
    for n in 1..=5 {
        responses.push(ok(page(
            vec![comment(&format!("c-{n}"), json!(format!("Comment {n}")))],
            json!(format!("cursor{n}.part")),
        )));
    }
    responses.push(ok(task("Final")));
    let requests = script(&mut s, op, responses)?;
    assert_eq!(requests.len(), 9);
    assert_eq!(
        requests[4],
        TodoistRequest::Comments {
            task_id: "task-1".into(),
            cursor: Some("cursor1.part".into())
        }
    );
    assert!(permit(&mut s, op, 120).is_err());
    s.finish_integration_transport(op)?;
    let w = window(&s, op)?;
    assert_eq!(w["categories"][2]["pages"], 5);
    assert!(reasons(&w, 2).contains(&"page_limit".into()));
    assert_eq!(w["objects"].as_array().map(Vec::len), Some(7));
    s.release_integration_read(op)?;

    // Six full 1 MiB bodies after `/user` cross the 6 MiB threshold before the fifth comment
    // page: the category stops and the final task check keeps its reserve.
    let pinned = s.integration_profile(p.id)?;
    let op = begin(&mut s, &pinned, t.id)?;
    let mut responses = vec![
        ok(user(json!("account-1"))),
        full(task("Initial"))?,
        full(project())?,
    ];
    for n in 1..=4 {
        responses.push(full(page(
            vec![comment(&format!("c-{n}"), json!("Text"))],
            json!(format!("cursor{n}.part")),
        ))?);
    }
    let requests = script(&mut s, op, responses)?;
    assert_eq!(requests.len(), 7);
    assert_eq!(
        permit(&mut s, op, 200)?,
        TodoistRequest::Task {
            id: "task-1".into()
        }
    );
    s.accept_integration_response(op, full(task("Final"))?, 200)?;
    s.finish_integration_transport(op)?;
    assert_eq!(s.integration_read_status(op)?, 200);
    assert!(reasons(&window(&s, op)?, 2).contains(&"read_limit".into()));
    Ok(())
}

#[test]
fn cursor_loops_missing_cursors_empty_and_repeated_pages_end_comments_as_partial()
-> Result<(), Box<dyn std::error::Error>> {
    let note = |id: &str, text: &str| comment(id, json!(text));
    for (pages, expected, objects) in [
        (
            vec![
                page(vec![note("c-1", "A")], json!("same.cursor")),
                page(vec![note("c-2", "B")], json!("same.cursor")),
            ],
            "repeated_cursor",
            4,
        ),
        (
            vec![json!({"results":[note("c-1","A")]})],
            "invalid_pagination",
            3,
        ),
        (
            vec![page(vec![note("c-1", "A")], json!("no-dot"))],
            "invalid_pagination",
            3,
        ),
        (
            vec![page(
                vec![note("c-1", "A")],
                json!(format!("{}.x", "a".repeat(4095))),
            )],
            "invalid_pagination",
            3,
        ),
        (vec![page(vec![], json!("next.cursor"))], "empty_page", 2),
        (
            vec![
                page(vec![note("c-1", "A")], json!("one.cursor")),
                page(vec![note("c-1", "A")], json!("two.cursor")),
            ],
            "repeated_page",
            3,
        ),
        (
            vec![
                page(vec![note("c-1", "A")], json!("one.cursor")),
                page(vec![note("c-1", "Changed")], Value::Null),
            ],
            "conflicting_duplicate",
            3,
        ),
        (vec![json!({"comments":[]})], "malformed_page", 2),
    ] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("Todoist")?;
        let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
        let op = begin(&mut s, &p, t.id)?;
        let mut responses = vec![
            ok(user(json!("account-1"))),
            ok(task("Initial")),
            ok(project()),
        ];
        responses.extend(pages.into_iter().map(ok));
        responses.push(ok(task("Final")));
        script(&mut s, op, responses)?;
        s.finish_integration_transport(op)?;
        assert_eq!(s.integration_read_status(op)?, 200, "{expected}");
        let w = window(&s, op)?;
        assert_eq!(w["categories"][2]["status"], "partial", "{expected}");
        assert!(reasons(&w, 2).contains(&expected.into()), "{expected}");
        assert_eq!(
            w["objects"].as_array().map(Vec::len),
            Some(objects),
            "{expected}"
        );
        if expected == "conflicting_duplicate" {
            let conflicting = w["objects"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|o| o["metadata"]["content"]["object_id"] == "c-1")
                .ok_or("comment")?;
            assert_eq!(conflicting["bindable"], false);
            let index = w["objects"]
                .as_array()
                .and_then(|a| a.iter().position(|o| o == conflicting))
                .ok_or("index")?;
            assert!(s.integration_preview(op, &[index]).is_err());
        }
    }
    Ok(())
}

#[test]
fn anchors_fail_the_whole_read_while_categories_fail_alone()
-> Result<(), Box<dyn std::error::Error>> {
    let mut moved = task("Final");
    moved["project_id"] = json!("project-2");
    let mut other = task("Final");
    other["id"] = json!("task-2");
    let mut checked = task("Final");
    checked["checked"] = json!(true);
    let mut unknown = task("Final");
    unknown["is_deleted"] = json!("no");
    let mut no_project = task("Initial");
    no_project
        .as_object_mut()
        .ok_or("task")?
        .remove("project_id");
    let mut wrong_project = project();
    wrong_project["id"] = json!("project-9");
    let fatal = |responses: Vec<IntegrationResponse>, code: u32| (responses, Some(code), "");
    let partial =
        |responses: Vec<IntegrationResponse>, reason: &'static str| (responses, None, reason);
    let u = || ok(user(json!("account-1")));
    let comments = || ok(page(vec![], Value::Null));
    for (responses, failure, reason) in [
        fatal(
            vec![
                u(),
                ok(task("Initial")),
                ok(project()),
                comments(),
                ok(moved),
            ],
            409,
        ),
        fatal(
            vec![
                u(),
                ok(task("Initial")),
                ok(project()),
                comments(),
                ok(other.clone()),
            ],
            409,
        ),
        fatal(vec![u(), ok(other)], 409),
        fatal(vec![u(), ok(checked)], 409),
        fatal(vec![u(), ok(unknown)], 422),
        fatal(vec![u(), ok(no_project)], 422),
        fatal(vec![u(), status(404)], 404),
        fatal(
            vec![
                u(),
                ok(task("Initial")),
                ok(project()),
                comments(),
                status(403),
            ],
            403,
        ),
        fatal(vec![status(403)], 403),
        fatal(vec![u(), ok(task("Initial")), status(500)], 500),
        partial(
            vec![
                u(),
                ok(task("Initial")),
                status(403),
                comments(),
                ok(task("Final")),
            ],
            "forbidden",
        ),
        partial(
            vec![
                u(),
                ok(task("Initial")),
                status(404),
                comments(),
                ok(task("Final")),
            ],
            "unavailable",
        ),
        partial(
            vec![
                u(),
                ok(task("Initial")),
                ok(wrong_project),
                comments(),
                ok(task("Final")),
            ],
            "identity_mismatch",
        ),
        partial(
            vec![
                u(),
                ok(task("Initial")),
                ok(project()),
                status(404),
                ok(task("Final")),
            ],
            "unavailable",
        ),
    ] {
        let db = DB::new();
        let mut s = Store::open(&db.0)?;
        ready(&mut s)?;
        let t = s.create_task("Todoist")?;
        let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
        let op = begin(&mut s, &p, t.id)?;
        let n = responses.len();
        for (i, r) in responses.into_iter().enumerate() {
            permit(&mut s, op, 100 + i as i64)?;
            let accepted = s.accept_integration_response(op, r, 100 + i as i64);
            assert_eq!(
                accepted.is_err(),
                failure.is_some() && i + 1 == n,
                "{failure:?} {reason}"
            );
        }
        s.finish_integration_transport(op)?;
        match failure {
            Some(code) => {
                assert_eq!(s.integration_read_status(op)?, code);
                assert!(s.integration_window_page(op, 0).is_err());
            }
            None => {
                let w = window(&s, op)?;
                let category = if reasons(&w, 1).contains(&reason.into()) {
                    1
                } else {
                    2
                };
                assert!(reasons(&w, category).contains(&reason.into()), "{reason}");
                // The task and the other category stay available.
                assert!(
                    w["objects"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|o| o["text"] == "Final"))
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_401_revokes_only_the_generation_it_was_sent_with() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    permit(&mut s, op, 100)?;
    let replaced = s.replace_integration_credential(p.id, p.settings_revision)?;
    assert!(s.accept_integration_response(op, status(401), 100).is_err());
    assert_eq!(s.integration_profile(p.id)?, replaced);
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    let op = begin(&mut s, &replaced, t.id)?;
    script(&mut s, op, vec![ok(user(json!("account-1")))])?;
    permit(&mut s, op, 101)?;
    assert!(s.accept_integration_response(op, status(401), 101).is_err());
    assert_eq!(s.integration_read_status(op)?, 401);
    let revoked = s.integration_profile(p.id)?;
    assert!(revoked.credential_generation > replaced.credential_generation);
    assert_eq!(revoked.scope.as_deref(), Some("account-1"));
    Ok(())
}

#[test]
fn a_rate_limit_body_sets_the_pause_and_its_raw_text_is_kept_nowhere()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    script(&mut s, op, vec![ok(user(json!("account-1")))])?;
    permit(&mut s, op, 101)?;
    let limited = IntegrationResponse {
        status: 429,
        body: serde_json::to_vec(
            &json!({"error":"CANARY-429-BODY","error_code":35,"error_tag":"LIMITS_REACHED","http_code":429,"error_extra":{"retry_after":30,"event_id":"CANARY-EVENT"}}),
        )?,
        headers: IntegrationHeaders::default(),
    };
    assert!(s.accept_integration_response(op, limited, 101).is_err());
    assert_eq!(s.integration_read_status(op)?, 429);
    let paused = s.integration_profile(p.id)?;
    assert_eq!(paused.not_before_utc, Some(131));
    assert!(paused.server_deadline_known);
    s.finish_integration_transport(op)?;
    assert!(s.integration_window_page(op, 0).is_err());
    let settings = s.integration_settings_json(IntegrationKind::Todoist)?;
    s.release_integration_read(op)?;
    // The saved pause blocks the next read until the deadline.
    let task = s.task(t.id)?;
    assert_eq!(
        s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: task.revision
            },
            paused.version(),
            "task-1",
            130,
            1000
        ),
        Err(StoreError::Busy)
    );
    drop(s);
    for canary in ["CANARY-429-BODY", "CANARY-EVENT", "LIMITS_REACHED"] {
        assert!(!contains(&db.bytes(), canary), "{canary}");
        assert!(!settings.contains(canary), "{canary}");
    }
    Ok(())
}

#[test]
fn projection_keeps_literal_text_presence_losses_and_unambiguous_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Todoist")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let quoted = "t\"1\\";
    let mut initial = task("Initial");
    initial["id"] = json!(quoted);
    initial["project_id"] = json!(quoted);
    let mut last = initial.clone();
    last["content"] = json!("Buy **milk** <b>now</b> [x](https://example.invalid)");
    last["description"] = json!("Line one\n- item");
    last["due"] = json!({"date":"2026-10-21","timezone":null,"is_recurring":false,"string":"tomorrow","lang":"en","extra":1});
    last["deadline"] = json!({"date":"2026-10-30T10:00:00"});
    last["duration"] = json!({"amount":-5,"unit":"minute"});
    last["added_at"] = json!("not a date");
    last.as_object_mut()
        .ok_or("task")?
        .remove("responsible_uid");
    let mut shared_project = project();
    shared_project["id"] = json!(quoted);
    shared_project["workspace_id"] = json!("workspace-1");
    shared_project
        .as_object_mut()
        .ok_or("project")?
        .remove("parent_id");
    let mut attached = comment("c-2", json!("With file"));
    attached["file_attachment"] = json!({"file_url":"https://example.invalid/secret.png"});
    let mut attachment_only = comment("c-3", json!(""));
    attachment_only["file_attachment"] = json!({"file_name":"a.png"});
    let mut deleted = comment("c-4", json!("Gone"));
    deleted["is_deleted"] = json!(true);
    let mut missing = comment("c-6", json!(""));
    missing.as_object_mut().ok_or("comment")?.remove("content");
    let op = begin(&mut s, &p, t.id)?;
    let task_selection = s.task(t.id)?;
    s.cancel_integration_read(op);
    s.finish_integration_transport(op)?;
    s.release_integration_read(op)?;
    let op = s.begin_integration_read(
        TaskVersion {
            task_id: t.id,
            revision: task_selection.revision,
        },
        p.version(),
        quoted,
        100,
        1000,
    )?;
    s.start_integration_transport(op)?;
    script(
        &mut s,
        op,
        vec![
            ok(user(json!("account-1"))),
            ok(initial),
            ok(shared_project),
            ok(page(
                vec![
                    comment("c-1", json!("Plain")),
                    attached,
                    attachment_only,
                    deleted,
                    comment("c-5", Value::Null),
                    missing,
                ],
                Value::Null,
            )),
            ok(last),
        ],
    )?;
    s.finish_integration_transport(op)?;
    let w = window(&s, op)?;
    let objects = w["objects"].as_array().ok_or("objects")?;
    let keys: Vec<&str> = objects
        .iter()
        .filter_map(|o| o["key"]["object_id"].as_str())
        .collect();
    assert!(keys.contains(&r#"["task","t\"1\\"]"#));
    assert!(keys.contains(&r#"["project","t\"1\\"]"#));
    assert!(keys.contains(&r#"["comment","t\"1\\","c-1"]"#));
    assert!(objects.iter().all(|o| o["key"]["scope"] == "account-1"));
    let saved_task = objects
        .iter()
        .find(|o| o["metadata"]["content"]["fields"]["kind"] == "task")
        .ok_or("task object")?;
    assert_eq!(
        saved_task["text"],
        "Buy **milk** <b>now</b> [x](https://example.invalid)\n\nLine one\n- item"
    );
    let fields = &saved_task["metadata"]["content"]["fields"];
    assert_eq!(fields["due"]["value"]["date"]["value"], "2026-10-21");
    assert_eq!(fields["due"]["value"]["timezone"]["presence"], "null");
    assert_eq!(
        fields["deadline"]["value"]["date"]["value"],
        "2026-10-30T10:00:00"
    );
    assert_eq!(fields["deadline"]["value"]["lang"]["presence"], "missing");
    assert_eq!(fields["duration"]["value"]["amount"]["presence"], "invalid");
    assert_eq!(fields["added_at"]["presence"], "invalid");
    assert_eq!(fields["responsible_uid"]["presence"], "missing");
    assert_eq!(fields["updated_at"]["value"]["precision_lost"], true);
    let losses = &saved_task["metadata"]["content"]["losses"];
    for loss in [
        "unknown:due",
        "missing:deadline.lang",
        "invalid:duration.amount",
        "invalid:added_at",
        "missing:responsible_uid",
        "precision_lost",
    ] {
        assert!(
            losses.as_array().is_some_and(|a| a.contains(&json!(loss))),
            "{loss}"
        );
    }
    let project_fields = objects
        .iter()
        .find(|o| o["metadata"]["content"]["fields"]["kind"] == "project")
        .map(|o| &o["metadata"]["content"]["fields"])
        .ok_or("project object")?;
    assert_eq!(project_fields["workspace_id"]["value"], "workspace-1");
    assert_eq!(project_fields["parent_id"]["presence"], "missing");
    let attached = objects
        .iter()
        .find(|o| o["metadata"]["content"]["object_id"] == "c-2")
        .ok_or("attached comment")?;
    assert_eq!(
        attached["metadata"]["content"]["fields"]["file_attachment"],
        true
    );
    assert!(!attached.to_string().contains("secret.png"));
    assert_eq!(objects.len(), 4);
    let comments = reasons(&w, 2);
    for reason in ["attachment_only", "deleted_object", "empty_object"] {
        assert!(comments.contains(&reason.into()), "{reason}");
    }
    assert_eq!(w["categories"][2]["unavailable"], 4);
    // The content hash follows the content: a later read of changed text gets another version.
    let version = saved_task["version"].as_str().ok_or("version")?.to_owned();
    let index = objects
        .iter()
        .position(|o| o == saved_task)
        .ok_or("index")?;
    s.integration_preview(op, &[index])?;
    s.bind_integration_objects(op, &[index], 300)?;
    s.release_integration_read(op)?;
    drop(s);
    let c = Connection::open(&db.0)?;
    let data: String = c.query_row("SELECT data FROM integration_metadata", [], |r| r.get(0))?;
    c.execute(
        "UPDATE integration_metadata SET data=?1",
        [data.replace("Line one", "Line 1!!")],
    )?;
    drop(c);
    assert!(
        Store::open(&db.0).is_err(),
        "tampered metadata must not open"
    );
    assert_eq!(version.len(), 64);
    Ok(())
}

#[test]
fn saved_todoist_facts_reach_08b_under_their_kind_and_exclusion_revokes_them()
-> Result<(), Box<dyn std::error::Error>> {
    let db = DB::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let t = s.create_task("Plan the week")?;
    let p = s.save_integration_profile(0, 0, "Personal", None, &todoist())?;
    let op = begin(&mut s, &p, t.id)?;
    script(
        &mut s,
        op,
        standard("account-1", vec![comment("c-1", json!("Comment"))]),
    )?;
    s.finish_integration_transport(op)?;
    s.integration_preview(op, &[0, 1, 2])?;
    let saved = s.bind_integration_objects(op, &[0, 1, 2], 200)?;
    s.release_integration_read(op)?;
    let preview = prepare_new(&mut s, t.id, saved.task.revision, &saved.evidence_ids)?;
    let context: Value = serde_json::from_str(&preview.context)?;
    let sources = context["sources"].as_array().ok_or("sources")?;
    assert_eq!(sources.len(), 3);
    for source in sources {
        assert_eq!(source["source"]["namespace"], TODOIST_NAMESPACE);
        assert_eq!(source["source"]["scope"], "account-1");
        assert_eq!(source["todoist"]["observation"], "non_atomic_observation");
        assert_eq!(
            source["todoist"]["current_state"],
            "current_state_unverified"
        );
    }
    let ground = sources[0]["evidence_id"]
        .as_str()
        .ok_or("evidence id")?
        .to_owned();
    let a = s.begin_assistance(preview.request_id)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 3)?;
    let mut answer = answer(t.id, None);
    answer.proposals[0].evidence_ids = vec![ground];
    let target = finish_single(&mut s, d.id(), confirmed(), Some(answer))?.ok_or("published")?;
    s.release_check(&flag);
    let details = serde_json::to_value(s.proposal_details(target)?)?;
    assert_eq!(details["citations"][0]["integration"]["kind"], "todoist");
    assert_eq!(
        details["citations"][0]["integration"]["metadata"]["account"],
        "account-1"
    );
    s.set_integration_scope_excluded(IntegrationKind::Todoist, "account-1", true)?;
    assert!(s.proposal_feed()?.proposals.is_empty());
    assert!(s.copy_proposal(target).is_err());
    Ok(())
}
