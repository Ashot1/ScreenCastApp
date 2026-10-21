use super::*;
use std::sync::atomic::AtomicU64;

static NEXT: AtomicU64 = AtomicU64::new(1);
fn text(s: &str) -> AmbientText {
    AmbientText {
        bytes: s.as_ptr(),
        length: s.len() as u32,
    }
}
type Ready = (u64, std::path::PathBuf, AmbientIntegrationProfileRef, Task);
fn ready() -> Result<Ready, Box<dyn std::error::Error>> {
    ready_with(|h, p| unsafe {
        ambient_integration_v1_jira_profile_save(h, 0, 0, text("Work"), text("Site"), p)
    })
}
fn ready_github() -> Result<Ready, Box<dyn std::error::Error>> {
    let repository = [AmbientGitHubRepository {
        owner: text("Example"),
        repository: text("Project"),
    }];
    ready_with(|h, p| unsafe {
        ambient_integration_v1_github_profile_save(h, 0, 0, text("Work"), repository.as_ptr(), 1, p)
    })
}
fn ready_todoist() -> Result<Ready, Box<dyn std::error::Error>> {
    ready_with(|h, p| unsafe {
        ambient_integration_v1_todoist_profile_save(h, 0, 0, text("Personal"), p)
    })
}
/// Opens memory through the ABI and makes assistance current through the Core test path.
fn ready_with(
    save: impl FnOnce(u64, &mut AmbientIntegrationProfileRef) -> u32,
) -> Result<Ready, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-integration-ffi-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let b = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    assert_eq!(
        unsafe { crate::ambient_v3_open(b.as_ptr(), b.len() as u32, &mut h) },
        0
    );
    let mut p = AmbientIntegrationProfileRef::default();
    assert_eq!(save(h, &mut p), 0);
    let slot = get_store(h).map_err(|_| "memory")?;
    let mut s = slot.store.lock().map_err(|_| "store")?;
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
    let task = s.create_task("Discuss")?;
    Ok((h, path, p, task))
}
fn begin(h: u64, p: AmbientIntegrationProfileRef, task: &Task) -> Result<u64, u32> {
    begin_target(h, p, task, "OLD-1")
}
const PR: &str = "https://github.com/Example/Project/pull/3";
fn begin_target(
    h: u64,
    p: AmbientIntegrationProfileRef,
    task: &Task,
    target: &str,
) -> Result<u64, u32> {
    let mut op = 0;
    let read = AmbientIntegrationRead {
        task_id: task.id.0 as u64,
        task_revision: task.revision,
        profile: p,
        target: text(target),
        session_expires_at: now()? + 1000,
    };
    match unsafe { ambient_integration_v1_begin(h, &read, &mut op) } {
        0 => Ok(op),
        e => Err(e),
    }
}
#[test]
fn wrong_secret_form_is_rejected_before_network_and_aborts_the_unused_read()
-> Result<(), Box<dyn std::error::Error>> {
    let (h, path, p, task) = ready()?;
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    for (first, second) in [
        (text("bad:email"), text("token")),
        (text("fixture@example.invalid"), text("bad token")),
        (text("fixture@example.invalid"), none),
    ] {
        let op = begin(h, p, &task).map_err(|e| format!("begin {e}"))?;
        let mut outcome = 0xdead;
        assert_eq!(
            unsafe { ambient_integration_v1_dispatch(h, op, first, second, &mut outcome) },
            INVALID_ARGUMENT
        );
        assert_eq!(outcome, 0xdead);
        let slot = get_store(h).map_err(|_| "memory")?;
        assert_eq!(
            slot.store
                .lock()
                .map_err(|_| "store")?
                .integration_read_status(op)?,
            499
        );
        assert_eq!(ambient_integration_v1_release(h, op), 0);
    }
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn github_dispatch_takes_one_token_and_rejects_a_second_part_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let (h, path, p, task) = ready_github()?;
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    for (first, second) in [(text("token"), text("second")), (text("bad token"), none)] {
        let op = begin_target(h, p, &task, PR).map_err(|e| format!("begin {e}"))?;
        let mut outcome = 0xdead;
        assert_eq!(
            unsafe { ambient_integration_v1_dispatch(h, op, first, second, &mut outcome) },
            INVALID_ARGUMENT
        );
        assert_eq!(outcome, 0xdead);
        let slot = get_store(h).map_err(|_| "memory")?;
        assert_eq!(
            slot.store
                .lock()
                .map_err(|_| "store")?
                .integration_read_status(op)?,
            499
        );
        assert_eq!(ambient_integration_v1_release(h, op), 0);
    }
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn todoist_dispatch_takes_one_token_and_rejects_a_second_part_before_network()
-> Result<(), Box<dyn std::error::Error>> {
    let (h, path, p, task) = ready_todoist()?;
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    for (first, second) in [(text("token"), text("second")), (text("bad token"), none)] {
        let op = begin_target(h, p, &task, "task-1").map_err(|e| format!("begin {e}"))?;
        let mut outcome = 0xdead;
        assert_eq!(
            unsafe { ambient_integration_v1_dispatch(h, op, first, second, &mut outcome) },
            INVALID_ARGUMENT
        );
        assert_eq!(outcome, 0xdead);
        let slot = get_store(h).map_err(|_| "memory")?;
        assert_eq!(
            slot.store
                .lock()
                .map_err(|_| "store")?
                .integration_read_status(op)?,
            499
        );
        assert_eq!(ambient_integration_v1_release(h, op), 0);
    }
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn cancel_by_operation_id_does_not_wait_for_the_store_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let (h, path, p, task) = ready()?;
    let op = begin(h, p, &task).map_err(|e| format!("begin {e}"))?;
    let slot = get_store(h).map_err(|_| "memory")?;
    let held = slot.store.lock().map_err(|_| "store")?;
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sent.send(ambient_integration_v1_cancel(h, op));
    });
    assert_eq!(received.recv_timeout(std::time::Duration::from_secs(2))?, 0);
    assert_eq!(held.integration_read_status(op)?, 499);
    drop(held);
    assert_eq!(ambient_integration_v1_abort(h, op), 0);
    assert_eq!(ambient_integration_v1_release(h, op), 0);
    assert_eq!(ambient_integration_v1_cancel(h, op), 11);
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[cfg(feature = "model-test-support")]
#[test]
fn loopback_dispatch_runs_until_core_ends_the_read_at_thirteen_requests()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let route = format!("http://{}/", listener.local_addr()?);
    let server = std::thread::spawn(move || -> std::io::Result<Vec<String>> {
        let mut paths = Vec::new();
        let issue = r#"{"id":"issue-id","key":"NEW-2","fields":{"summary":"Summary","description":null,"subtasks":[],"issuelinks":[],"parent":null}}"#.to_owned();
        while paths.len() < 13 {
            let (socket, _) = listener.accept()?;
            let mut reader = BufReader::new(socket.try_clone()?);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header)?;
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
            let start = path
                .split("startAt=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let item = |kind: &str, i: i64| {
                format!(
                    r#"{{"id":"{kind}{i}","created":"2021-01-17T12:00:00+0000","body":{{"type":"doc","version":1,"content":[{{"type":"paragraph","content":[{{"type":"text","text":"x"}}]}}]}},"items":[{{"field":"status","fieldtype":"jira","toString":"Done"}}]}}"#
                )
            };
            let body = if path.contains("/comment?") {
                let items = (start..start + 50)
                    .map(|i| item("c", i))
                    .collect::<Vec<_>>();
                format!(
                    r#"{{"startAt":{start},"maxResults":50,"total":999,"comments":[{}]}}"#,
                    items.join(",")
                )
            } else if path.contains("/changelog?") && path.contains("maxResults=1") {
                format!(
                    r#"{{"startAt":0,"maxResults":1,"total":1000,"values":[{}]}}"#,
                    item("probe", 0)
                )
            } else if path.contains("/changelog?") {
                let items = (start..start + 50)
                    .map(|i| item("h", i))
                    .collect::<Vec<_>>();
                format!(
                    r#"{{"startAt":{start},"maxResults":50,"total":1000,"values":[{}]}}"#,
                    items.join(",")
                )
            } else {
                issue.clone()
            };
            let mut socket = reader.into_inner();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            paths.push(path);
        }
        Ok(paths)
    });
    let (h, path, p, task) = ready()?;
    let op = begin(h, p, &task).map_err(|e| format!("begin {e}"))?;
    let mut outcome = 0;
    assert_eq!(
        unsafe {
            ambient_integration_v1_test_dispatch(
                h,
                op,
                text("fixture@example.invalid"),
                text("fixture-token"),
                text(&route),
                &mut outcome,
            )
        },
        0
    );
    assert_eq!(outcome, 200);
    let paths = server.join().map_err(|_| "server thread")??;
    assert_eq!(paths.len(), 13);
    assert!(paths[12].starts_with("/ex/jira/Site/rest/api/3/issue/issue-id?"));
    let slot = get_store(h).map_err(|_| "memory")?;
    let page = slot
        .store
        .lock()
        .map_err(|_| "store")?
        .integration_window_page(op, 0)?;
    assert!(page.contains("omitted_prefix"));
    assert_eq!(ambient_integration_v1_release(h, op), 0);
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[cfg(feature = "model-test-support")]
#[test]
fn github_loopback_dispatch_runs_until_core_ends_the_read_at_seventeen_requests()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let route = format!("http://{}/", listener.local_addr()?);
    let server = std::thread::spawn(move || -> std::io::Result<Vec<String>> {
        let mut paths = Vec::new();
        let pr = r#"{"id":20,"number":3,"title":"Title","body":null,"user":{"login":"author"},"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","state":"open","merged":false,"head":{"sha":"opaque"},"base":{"sha":"base","repo":{"id":10,"full_name":"Example/Project"}}}"#;
        while paths.len() < 17 {
            let (socket, _) = listener.accept()?;
            let mut reader = BufReader::new(socket.try_clone()?);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header)?;
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
            let (resource, query) = path.split_once('?').unwrap_or((&path, ""));
            let (body, link) = if query.is_empty() {
                (pr.to_owned(), String::new())
            } else {
                let page = query
                    .split("page=")
                    .last()
                    .and_then(|p| p.parse::<u32>().ok())
                    .unwrap_or(1);
                (
                    "[]".to_owned(),
                    format!(
                        "Link: <https://api.github.com{resource}?per_page=100&page={}>; rel=\"next\"\r\n",
                        page + 1
                    ),
                )
            };
            let mut socket = reader.into_inner();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{link}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            paths.push(path);
        }
        Ok(paths)
    });
    let (h, path, p, task) = ready_github()?;
    let op = begin_target(h, p, &task, PR).map_err(|e| format!("begin {e}"))?;
    let mut outcome = 0;
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    assert_eq!(
        unsafe {
            ambient_integration_v1_test_dispatch(
                h,
                op,
                text("fixture-token"),
                none,
                text(&route),
                &mut outcome,
            )
        },
        0
    );
    assert_eq!(outcome, 200);
    let paths = server.join().map_err(|_| "server thread")??;
    assert_eq!(paths.len(), 17);
    assert_eq!(paths[0], "/repos/Example/Project/pulls/3");
    assert_eq!(
        paths[5],
        "/repos/Example/Project/pulls/3/reviews?per_page=100&page=5"
    );
    assert_eq!(paths[16], "/repos/Example/Project/pulls/3");
    let slot = get_store(h).map_err(|_| "memory")?;
    let page = slot
        .store
        .lock()
        .map_err(|_| "store")?
        .integration_window_page(op, 0)?;
    assert!(page.contains("page_limit"));
    assert_eq!(ambient_integration_v1_release(h, op), 0);
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[cfg(feature = "model-test-support")]
#[test]
fn todoist_loopback_pins_the_account_and_ends_at_nine_requests()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let route = format!("http://{}/", listener.local_addr()?);
    let server = std::thread::spawn(move || -> std::io::Result<Vec<String>> {
        let mut paths = Vec::new();
        let task = r#"{"id":"task-1","project_id":"project-1","content":"Task","description":"","checked":false,"is_deleted":false}"#;
        while paths.len() < 9 {
            let (socket, _) = listener.accept()?;
            let mut reader = BufReader::new(socket.try_clone()?);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let mut authorization = String::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header)?;
                if header.to_ascii_lowercase().starts_with("authorization:") {
                    authorization = header.trim_end().to_owned();
                }
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            assert_eq!(
                authorization.split_once(':').map(|(_, v)| v.trim()),
                Some("Bearer fixture-token")
            );
            let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
            let body = if path == "/api/v1/user" {
                r#"{"id":"account-1","email":"fixture@example.invalid","token":"CANARY"}"#
                    .to_owned()
            } else if path.starts_with("/api/v1/comments?") {
                let n = paths.len();
                format!(
                    r#"{{"results":[{{"id":"c-{n}","content":"Note","posted_uid":null,"posted_at":null,"is_deleted":false,"file_attachment":null}}],"next_cursor":"page{n}.next"}}"#
                )
            } else if path == "/api/v1/projects/project-1" {
                r#"{"id":"project-1","name":"Home","description":"","is_archived":false,"is_deleted":false}"#.to_owned()
            } else {
                task.to_owned()
            };
            let mut socket = reader.into_inner();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            paths.push(path);
        }
        Ok(paths)
    });
    let (h, path, p, task) = ready_todoist()?;
    let op = begin_target(h, p, &task, "task-1").map_err(|e| format!("begin {e}"))?;
    let mut outcome = 0;
    let none = AmbientText {
        bytes: std::ptr::null(),
        length: 0,
    };
    assert_eq!(
        unsafe {
            ambient_integration_v1_test_dispatch(
                h,
                op,
                text("fixture-token"),
                none,
                text(&route),
                &mut outcome,
            )
        },
        0
    );
    assert_eq!(outcome, 200);
    let paths = server.join().map_err(|_| "server thread")??;
    assert_eq!(paths.len(), 9);
    assert_eq!(paths[0], "/api/v1/user");
    assert_eq!(paths[1], "/api/v1/tasks/task-1");
    assert_eq!(paths[2], "/api/v1/projects/project-1");
    assert_eq!(paths[3], "/api/v1/comments?task_id=task-1&limit=50");
    assert_eq!(
        paths[4],
        "/api/v1/comments?task_id=task-1&limit=50&cursor=page3.next"
    );
    assert_eq!(paths[8], "/api/v1/tasks/task-1");
    assert_eq!(ambient_integration_v1_release(h, op), 0);
    let slot = get_store(h).map_err(|_| "memory")?;
    let pinned = slot
        .store
        .lock()
        .map_err(|_| "store")?
        .integration_profile(p.id)?;
    assert_eq!(pinned.scope.as_deref(), Some("account-1"));
    assert_eq!(pinned.settings_revision, p.settings_revision);
    // The export reads the stored account itself, so a rename after the pin passes.
    let mut renamed = AmbientIntegrationProfileRef::default();
    assert_eq!(
        unsafe {
            ambient_integration_v1_todoist_profile_save(
                h,
                p.id,
                p.settings_revision,
                text("Renamed"),
                &mut renamed,
            )
        },
        0
    );
    let after = slot
        .store
        .lock()
        .map_err(|_| "store")?
        .integration_profile(p.id)?;
    assert_eq!(after.scope.as_deref(), Some("account-1"));
    assert_eq!(after.name, "Renamed");
    assert_eq!(crate::ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
