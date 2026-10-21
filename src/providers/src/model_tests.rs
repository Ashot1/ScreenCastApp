use super::*;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
fn run_http(token: &str, flag: Arc<AtomicBool>, options: Options) -> ModelCheckResult {
    run_model_http(token, flag, ModelProfile::Low, None, options).check
}
fn item() -> Value {
    json!({"id":"message-test","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"AMBIENT_OK","annotations":[]}]})
}
fn terminal(tier: &str, output: Value) -> Value {
    json!({"type":"response.completed","response":{"status":"completed","model":"gpt-6-luna","reasoning":{"effort":"low"},"service_tier":tier,"output":output,"usage":{"input_tokens":8,"output_tokens":3,"total_tokens":11,"input_tokens_details":{"cached_tokens":2},"output_tokens_details":{"reasoning_tokens":1}}}})
}
fn event(v: Value) -> Vec<u8> {
    format!("data: {}\r\n\r\n", v).into_bytes()
}
fn stream(tier: &str, empty: bool) -> Vec<u8> {
    let mut b = event(json!({"type":"response.output_item.done","output_index":0,"item":item()}));
    b.extend(event(terminal(
        tier,
        if empty { json!([]) } else { json!([item()]) },
    )));
    b
}
fn decode(b: &[u8], step: usize) -> ModelCheckResult {
    let mut d = SseDecoder::default();
    for c in b.chunks(step) {
        if d.push(c).is_err() {
            d.invalid = true;
            break;
        }
    }
    d.finish().check
}
#[test]
fn completed_and_empty_terminal_output_confirm_only_actual_fast_and_keep_usage_on_mismatch() {
    for empty in [false, true] {
        for tier in ["priority", "fast", "default"] {
            let r = decode(&stream(tier, empty), 1);
            assert_eq!(
                r.outcome,
                if tier == "default" {
                    CheckOutcome::Mismatch
                } else {
                    CheckOutcome::Confirmed
                }
            );
            assert_eq!(r.usage.map(|v| v.total()), Some(11));
        }
    }
    let mut t = terminal("priority", json!([item()]));
    t["response"]["model"] = json!("different");
    let mut b = event(json!({"type":"response.output_item.done","output_index":0,"item":item()}));
    b.extend(event(t));
    let r = decode(&b, 7);
    assert_eq!(r.outcome, CheckOutcome::Mismatch);
    assert_eq!(r.actual.model, ActualModel::Other);
}
#[test]
fn malformed_incomplete_duplicate_conflicting_and_unfinished_items_are_never_success() {
    let done = json!({"type":"response.output_item.done","output_index":0,"item":item()});
    let mut cases = vec![
        event(done.clone()),
        b"data: {bad}\n\n".to_vec(),
        b"data: \xff\n\n".to_vec(),
    ];
    let mut duplicate = event(done.clone());
    duplicate.extend(event(done.clone()));
    duplicate.extend(event(terminal("priority", json!([]))));
    cases.push(duplicate);
    let mut conflict = item();
    conflict["content"][0]["text"] = json!("WRONG");
    let mut b = event(done.clone());
    b.extend(event(terminal("priority", json!([conflict]))));
    cases.push(b);
    let mut b = event(
        json!({"type":"response.output_item.added","output_index":1,"item":{"id":"unfinished","type":"message"}}),
    );
    b.extend(stream("priority", true));
    cases.push(b);
    let mut b = stream("priority", true);
    b.extend(b"data: {partial");
    cases.push(b);
    let mut b = event(json!({"type":"response.output_item.done","output_index":1,"item":item()}));
    b.extend(event(terminal("priority", json!([]))));
    cases.push(b);
    for b in cases {
        assert_eq!(decode(&b, 3).outcome, CheckOutcome::Invalid);
    }
}
#[test]
fn usage_validation_is_independent_of_functional_result() {
    for usage in [
        json!(null),
        json!({"input_tokens":-1,"output_tokens":3,"total_tokens":2}),
        json!({"input_tokens":8,"output_tokens":3,"total_tokens":12}),
        json!({"input_tokens":8,"output_tokens":3,"total_tokens":11,"output_tokens_details":{"reasoning_tokens":4}}),
        json!({"input_tokens":8.0,"output_tokens":3,"total_tokens":11}),
    ] {
        let mut t = terminal("default", json!([item()]));
        t["response"]["usage"] = usage;
        let mut b =
            event(json!({"type":"response.output_item.done","output_index":0,"item":item()}));
        b.extend(event(t));
        let r = decode(&b, 11);
        assert_eq!(r.outcome, CheckOutcome::Mismatch);
        assert_eq!(r.usage, None);
    }
    let mut wrong = item();
    wrong["content"][0]["text"] = json!("WRONG");
    let mut b =
        event(json!({"type":"response.output_item.done","output_index":0,"item":wrong.clone()}));
    b.extend(event(terminal("priority", json!([wrong]))));
    let r = decode(&b, 5);
    assert_eq!(r.outcome, CheckOutcome::Invalid);
    assert_eq!(r.usage.map(|v| v.total()), Some(11));
}
#[test]
fn sse_line_endings_multiline_data_utf8_and_limits_are_bounded() {
    let text = String::from_utf8(stream("priority", true)).unwrap_or_default();
    for ending in ["\r", "\n", "\r\n"] {
        assert_eq!(
            decode(text.replace("\r\n", ending).as_bytes(), 1).outcome,
            CheckOutcome::Confirmed
        );
    }
    let mut b =
        b": comment \xf0\x9f\x8c\xbf\n\ndata: {\ndata: \"type\":\"response.created\"}\n\n".to_vec();
    b.extend(stream("priority", true));
    assert_eq!(decode(&b, 1).outcome, CheckOutcome::Confirmed);
    for b in [
        format!("data: {}\n\n", "x".repeat(512 * 1024)).into_bytes(),
        b":x\n\n".repeat(500_000),
        event(json!({"type":"response.created"})).repeat(4097),
    ] {
        assert_eq!(decode(&b, 16384).outcome, CheckOutcome::Invalid);
    }
}
#[test]
fn sse_quota_failure_is_closed_error_with_no_success() {
    let r = decode(
        &event(
            json!({"type":"response.failed","response":{"error":{"code":"insufficient_quota"}}}),
        ),
        7,
    );
    assert_eq!(r.outcome, CheckOutcome::Quota);
    assert_eq!(r.usage, None);
}
fn server(
    body: Vec<u8>,
    mime: &str,
    status: &str,
    stall: bool,
) -> Result<(String, thread::JoinHandle<Vec<u8>>), std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/responses", listener.local_addr()?);
    let mime = mime.to_owned();
    let status = status.to_owned();
    let worker = thread::spawn(move || {
        let Ok((mut s, _)) = listener.accept() else {
            return vec![];
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut input = vec![];
        let mut buf = [0; 4096];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
            input.extend_from_slice(&buf[..n]);
            if let Some(p) = input.windows(4).position(|v| v == b"\r\n\r\n") {
                let h = String::from_utf8_lossy(&input[..p]).to_lowercase();
                let len = h
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if input.len() >= p + 4 + len {
                    break;
                }
            }
        }
        let header = format!(
            "HTTP/1.1 {status}\r\n{mime}Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
        );
        let _ = s.write_all(header.as_bytes());
        if stall {
            thread::sleep(Duration::from_millis(250));
        } else {
            for part in body.chunks(17) {
                if write!(s, "{:x}\r\n", part.len()).is_err()
                    || s.write_all(part).is_err()
                    || s.write_all(b"\r\n").is_err()
                {
                    break;
                }
            }
            let _ = s.write_all(b"0\r\n\r\n");
        }
        drop(s);
        let _ = listener.set_nonblocking(true);
        thread::sleep(Duration::from_millis(30));
        assert!(listener.accept().is_err());
        input
    });
    Ok((url, worker))
}
#[test]
fn real_http_uses_fixed_request_and_never_follows_redirect_or_retries()
-> Result<(), Box<dyn std::error::Error>> {
    for (mime, status, want) in [
        (
            "Content-Type: text/event-stream\r\n",
            "200 OK",
            CheckOutcome::Confirmed,
        ),
        ("", "200 OK", CheckOutcome::Confirmed),
        (
            "Content-Type: application/json\r\n",
            "200 OK",
            CheckOutcome::Invalid,
        ),
        (
            "Location: http://127.0.0.1:1/no\r\n",
            "307 Temporary Redirect",
            CheckOutcome::Provider,
        ),
        ("", "401 Unauthorized", CheckOutcome::Provider),
    ] {
        let (url, w) = server(stream("priority", true), mime, status, false)?;
        let r = run_http(
            "test-access",
            Arc::new(AtomicBool::new(false)),
            Options::test(url),
        );
        assert_eq!(r.outcome, want);
        let req = w.join().map_err(|_| "server")?;
        let p = req
            .windows(4)
            .position(|v| v == b"\r\n\r\n")
            .ok_or("header")?;
        let h = String::from_utf8_lossy(&req[..p]).to_lowercase();
        assert!(h.starts_with("post /responses "));
        assert!(h.contains("authorization: bearer test-access"));
        assert!(!h.contains("openai-organization"));
        assert!(!h.contains("openai-project"));
        let v: Value = serde_json::from_slice(&req[p + 4..])?;
        assert_eq!(
            v,
            json!({"model":"gpt-6-luna","reasoning":{"effort":"low"},"service_tier":"priority","store":false,"stream":true,"tools":[],"input":[{"role":"user","content":"Reply exactly: AMBIENT_OK"}]})
        );
    }
    Ok(())
}
#[test]
fn real_http_stall_cancel_and_read_deadline_return_only_after_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    for cancel in [false, true] {
        let (url, w) = server(
            vec![],
            "Content-Type: text/event-stream\r\n",
            "200 OK",
            true,
        )?;
        let flag = Arc::new(AtomicBool::new(false));
        let c = flag.clone();
        let worker = thread::spawn(move || {
            if cancel {
                thread::sleep(Duration::from_millis(40));
                c.store(true, Ordering::Release);
            }
        });
        let r = run_http("test-access", flag, Options::test(url));
        assert_eq!(
            r.outcome,
            if cancel {
                CheckOutcome::Cancelled
            } else {
                CheckOutcome::Timeout
            }
        );
        worker.join().map_err(|_| "cancel")?;
        w.join().map_err(|_| "server")?;
    }
    Ok(())
}

#[test]
fn unfinished_delta_is_rejected_and_invalid_item_keeps_later_valid_usage() {
    let mut b =
        event(json!({"type":"response.output_text.delta","output_index":1,"delta":"unfinished"}));
    b.extend(stream("priority", true));
    assert_eq!(decode(&b, 7).outcome, CheckOutcome::Invalid);
    let mut bad = item();
    bad["status"] = json!("in_progress");
    let mut b =
        event(json!({"type":"response.output_item.done","output_index":0,"item":bad.clone()}));
    b.extend(event(terminal("priority", json!([bad]))));
    let r = decode(&b, 7);
    assert_eq!(r.outcome, CheckOutcome::Invalid);
    assert_eq!(r.usage.map(|u| u.total()), Some(11));
}

#[test]
fn actual_http_oversize_and_quota_errors_remain_bounded() -> Result<(), Box<dyn std::error::Error>>
{
    for (body, status, want) in [
        (
            b"data: "
                .iter()
                .copied()
                .chain(std::iter::repeat_n(b'x', 512 * 1024))
                .collect(),
            "200 OK",
            CheckOutcome::Invalid,
        ),
        (
            br#"{"error":{"code":"insufficient_quota"}}"#.to_vec(),
            "429 Too Many Requests",
            CheckOutcome::Quota,
        ),
        (
            vec![b'x'; 16385],
            "500 Internal Server Error",
            CheckOutcome::Provider,
        ),
    ] {
        let (url, w) = server(body, "Content-Type: text/event-stream\r\n", status, false)?;
        // The 512 KiB body arrives in 17-byte chunks; deadlines are covered by other tests, so a
        // loaded host must not turn this outcome check into a Timeout.
        let mut options = Options::test(url);
        options.timeout = Duration::from_secs(30);
        options.read = Duration::from_secs(10);
        assert_eq!(
            run_http("test-access", Arc::new(AtomicBool::new(false)), options).outcome,
            want
        );
        w.join().map_err(|_| "server")?;
    }
    Ok(())
}
#[test]
fn cancellation_before_connect_and_total_deadline_deny_more_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/responses", listener.local_addr()?);
    listener.set_nonblocking(true)?;
    assert_eq!(
        run_http(
            "test-access",
            Arc::new(AtomicBool::new(true)),
            Options::test(url)
        )
        .outcome,
        CheckOutcome::Cancelled
    );
    assert!(listener.accept().is_err());
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/responses", listener.local_addr()?);
    listener.set_nonblocking(true)?;
    let worker = thread::spawn(move || -> std::io::Result<bool> {
        // A client can time out before connecting under scheduler load; fixture cleanup must still be bounded.
        let accept_deadline = Instant::now() + Duration::from_secs(2);
        let (mut s, _) = loop {
            match listener.accept() {
                Ok(value) => break value,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < accept_deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e),
            }
        };
        s.set_nonblocking(false)?;
        s.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut b = [0; 4096];
        if s.read(&mut b)? == 0 {
            return Ok(false);
        }
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\nConnection: close\r\n\r\n")?;
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(15));
            if s.write_all(b":").is_err() {
                break;
            }
        }
        Ok(true)
    });
    let mut options = Options::test(url);
    options.timeout = Duration::from_millis(400);
    options.read = Duration::from_millis(200);
    assert_eq!(
        run_http("test-access", Arc::new(AtomicBool::new(false)), options).outcome,
        CheckOutcome::Timeout
    );
    assert!(
        worker.join().map_err(|_| "server")??,
        "deadline test must reach the response stream"
    );
    Ok(())
}
#[test]
fn high_assistance_has_a_strict_bounded_body_and_never_uses_a_low_payload() {
    let body = build_body(ModelProfile::High, Some("{\"saved\":\"exact 🌿\"}"))
        .unwrap_or_else(|_| panic!("body"));
    let v: Value = serde_json::from_str(&body).unwrap_or_default();
    assert_eq!(v["reasoning"]["effort"], "high");
    assert_eq!(v["service_tier"], "priority");
    assert_eq!(v["input"][0]["content"], "{\"saved\":\"exact 🌿\"}");
    assert_eq!(v["text"]["format"]["strict"], true);
    assert_eq!(v["text"]["format"]["schema"]["additionalProperties"], false);
    assert_eq!(
        v["text"]["format"]["schema"]["required"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert!(v.get("max_output_tokens").is_none());
    assert_eq!(v["tools"], json!([]));
    assert_eq!(v["store"], false);
    assert!(build_body(ModelProfile::Low, Some("{}")).is_err());
    assert!(
        build_body(
            ModelProfile::High,
            Some(&"x".repeat(MAX_ASSISTANCE_CONTEXT + 1))
        )
        .is_err()
    );
}
fn assistance_stream(text: &str, effort: &str, tier: &str) -> Vec<u8> {
    let mut i = item();
    i["content"][0]["text"] = json!(text);
    let mut t = terminal(tier, json!([i.clone()]));
    t["response"]["reasoning"]["effort"] = json!(effort);
    let mut b = event(json!({"type":"response.output_item.done","output_index":0,"item":i}));
    b.extend(event(t));
    b
}
#[test]
fn high_assistance_decodes_json_above_low_limit_and_preserves_usage_on_bad_answers() {
    let text=json!({"proposals":[{"proposal_id":null,"kind":"suggestion","message":"x".repeat(300),"rationale":"Saved note","evidence_ids":["note:1"],"step_id":null}],"rationale":"Full set"}).to_string();
    let mut d = SseDecoder::new(ModelProfile::High, true);
    for c in assistance_stream(&text, "high", "fast").chunks(3) {
        assert!(d.push(c).is_ok());
    }
    let r = d.finish();
    assert_eq!(r.check.outcome, CheckOutcome::Confirmed);
    assert_eq!(
        r.answer
            .as_ref()
            .and_then(|a| a.proposals.first())
            .map(|p| &p.message)
            .map(String::len),
        Some(300)
    );
    for text in [
   "{\"kind\":\"silence\",\"message\":null,\"rationale\":\"none\",\"evidence_ids\":[],\"step_id\":null,\"extra\":1}".to_owned(),
   "{\"kind\":\"silence\",\"kind\":\"silence\",\"message\":null,\"rationale\":\"none\",\"evidence_ids\":[],\"step_id\":null}".to_owned(),
   "{\"kind\":\"silence\",\"rationale\":\"none\",\"evidence_ids\":[],\"step_id\":null}".to_owned(),
   json!({"kind":"suggestion","message":"go","rationale":"reason","evidence_ids":["note:1","note:1"],"step_id":null}).to_string(),
   json!({"kind":"silence","message":"not silence","rationale":"reason","evidence_ids":[],"step_id":null}).to_string(),
   "x".repeat(MAX_ASSISTANCE_TEXT+1),
 ] {
   let mut d=SseDecoder::new(ModelProfile::High,true);let _=d.push(&assistance_stream(&text,"high","fast"));let r=d.finish();
   assert_eq!(r.check.outcome,CheckOutcome::Invalid);assert_eq!(r.check.usage.map(|u|u.total()),Some(11));assert!(r.answer.is_none());
 }
    let mut d = SseDecoder::new(ModelProfile::High, true);
    assert!(d.push(&assistance_stream(&text, "low", "priority")).is_ok());
    let r = d.finish();
    assert_eq!(r.check.outcome, CheckOutcome::Mismatch);
    assert!(r.answer.is_none());
}
#[test]
fn high_connectivity_requires_exact_marker_and_actual_high_fast() {
    for (effort, tier, want) in [
        ("high", "fast", CheckOutcome::Confirmed),
        ("low", "fast", CheckOutcome::Mismatch),
        ("high", "default", CheckOutcome::Mismatch),
    ] {
        let mut d = SseDecoder::new(ModelProfile::High, false);
        assert!(
            d.push(&assistance_stream("AMBIENT_OK", effort, tier))
                .is_ok()
        );
        assert_eq!(d.finish().check.outcome, want);
    }
}
#[test]
fn real_http_high_assistance_sends_the_exact_context_and_accepts_one_result()
-> Result<(), Box<dyn std::error::Error>> {
    let text=json!({"proposals":[{"proposal_id":null,"kind":"clarification","message":"What is missing?","rationale":"Saved partial plan","evidence_ids":["task:1"],"step_id":null}],"rationale":"Full set"}).to_string();
    let (url, w) = server(
        assistance_stream(&text, "high", "priority"),
        "Content-Type: text/event-stream\r\n",
        "200 OK",
        false,
    )?;
    let context = "{\"saved\":\"exact 🌿\"}";
    let r = run_model_http(
        "test-access",
        Arc::new(AtomicBool::new(false)),
        ModelProfile::High,
        Some(context),
        Options::test(url),
    );
    assert_eq!(r.check.outcome, CheckOutcome::Confirmed);
    assert_eq!(
        r.answer.and_then(|a| a.proposals.first().map(|p| p.kind)),
        Some(AssistanceKind::Clarification)
    );
    let req = w.join().map_err(|_| "server")?;
    let p = req
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .ok_or("header")?;
    let body: Value = serde_json::from_slice(&req[p + 4..])?;
    assert_eq!(body["input"][0]["content"], context);
    assert_eq!(body["reasoning"]["effort"], "high");
    Ok(())
}
#[test]
fn serialized_body_limit_is_checked_after_context_escaping_before_network() {
    let context = "\\".repeat(200 * 1024);
    assert!(context.len() < MAX_ASSISTANCE_CONTEXT);
    assert!(build_body(ModelProfile::High, Some(&context)).is_err());
}

#[test]
fn complete_proposal_wire_rejects_missing_duplicate_extra_fields_and_enforces_full_set_bounds() {
    let p = json!({"proposal_id":null,"kind":"clarification","message":"Question?","rationale":"Saved","evidence_ids":["task:1"],"step_id":null});
    for count in [0, 16] {
        let text = json!({"proposals":vec![p.clone();count],"rationale":"Complete"}).to_string();
        assert_eq!(
            parse_assistance(&text).map(|a| a.proposals.len()),
            Ok(count)
        );
    }
    let mut missing = p.clone();
    missing
        .as_object_mut()
        .unwrap_or_else(|| panic!("fixture"))
        .remove("proposal_id");
    let mut extra = p.clone();
    extra["visible"] = json!(true);
    let mut float = p.clone();
    float["proposal_id"] = json!(1.0);
    for text in [
        json!({"proposals":vec![p.clone();17],"rationale":"Complete"}).to_string(),
        json!({"proposals":[missing],"rationale":"Complete"}).to_string(),
        json!({"proposals":[extra],"rationale":"Complete"}).to_string(),
        json!({"proposals":[float],"rationale":"Complete"}).to_string(),
        "{\"proposals\":[],\"proposals\":[],\"rationale\":\"Complete\"}".to_owned(),
        "{\"proposals\":[]}".to_owned(),
    ] {
        assert!(parse_assistance(&text).is_err(), "{text}");
    }
}
fn body(step: &ModelStep<'_>) -> Result<String, Box<dyn std::error::Error>> {
    step_body(step).map_err(|e| format!("{e:?}").into())
}
/// Core prepare → body for an assistance request without plugins, frozen before 24A.
fn no_plugin_assistance_body() -> Result<String, Box<dyn std::error::Error>> {
    use ambient_core::{
        AssistanceMode, AssistanceRequest, ModelProfile as Profile, Store, TaskVersion,
    };
    let dir = std::env::temp_dir().join(format!(
        "ambient-golden-{}-{:?}",
        std::process::id(),
        thread::current().id()
    ));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("memory.sqlite");
    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let mut s = Store::open(&path)?;
        let task = s.create_task("Подготовить отчёт 🌿 for Friday")?;
        let note = s.add_note(
            task.id,
            task.revision,
            "Draft is in the shared folder",
            1_700_000_000,
        )?;
        s.set_model_session(9, true, true)?;
        let a = s.prepare_check(true, Profile::High)?;
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
        let revision = s.task(task.id)?.revision;
        let preview = s.prepare_assistance(AssistanceRequest {
            task: TaskVersion {
                task_id: task.id,
                revision,
            },
            workspace_revision: s.assistant_workspace_revision(task.id)?,
            mode: AssistanceMode::NewContext,
            selected_evidence: vec![note.id],
            reply: None,
        })?;
        // The production path: the plain step of a plugin loop with no enabled plugin.
        let a = s.begin_assistance(preview.request_id)?;
        let mut l = s.begin_model_loop(a, &[])?;
        let step = s.dispatch_model_step(&mut l, 2)?;
        assert_eq!(step.mode, StepMode::Plain);
        body(&step)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}
#[test]
fn assistance_body_without_plugins_is_byte_identical_to_the_pre_plugin_golden()
-> Result<(), Box<dyn std::error::Error>> {
    let body = no_plugin_assistance_body()?;
    if let Some(dir) = std::env::var_os("AMBIENT_GOLDEN_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("no-plugin-assistance-body.json"),
            &body,
        )?;
    }
    assert_eq!(
        body.as_bytes(),
        include_bytes!("../tests/fixtures/no-plugin-assistance-body.json")
    );
    Ok(())
}
fn reasoning() -> Value {
    json!({"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque"})
}
fn function_call(arguments: &str) -> Value {
    json!({"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"search","namespace":"plugins","arguments":arguments})
}
fn call_stream(arguments: &str, tier: &str) -> Vec<u8> {
    let mut b =
        event(json!({"type":"response.output_item.done","output_index":0,"item":reasoning()}));
    let mut added = function_call("");
    added["status"] = json!("in_progress");
    b.extend(event(
        json!({"type":"response.output_item.added","output_index":1,"item":added}),
    ));
    b.extend(event(json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":1,"delta":arguments})));
    b.extend(event(json!({"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":1,"arguments":arguments})));
    b.extend(event(json!({"type":"response.output_item.done","output_index":1,"item":function_call(arguments)})));
    let mut t = terminal(tier, json!([reasoning(), function_call(arguments)]));
    t["response"]["reasoning"]["effort"] = json!("high");
    b.extend(event(t));
    b
}
fn decode_tools(b: &[u8], tools: bool) -> ModelResponse {
    let mut d = SseDecoder::new(ModelProfile::High, true).tools(tools);
    for c in b.chunks(5) {
        if d.push(c).is_err() {
            d.invalid = true;
            break;
        }
    }
    d.finish()
}
#[test]
fn tool_steps_decode_calls_and_forward_reasoning_and_call_items_unchanged() {
    let r = decode_tools(&call_stream(r#"{"q":"plan","limit":3}"#, "priority"), true);
    assert_eq!(r.check.outcome, CheckOutcome::Confirmed);
    assert!(r.answer.is_none());
    assert_eq!(r.check.usage.map(|u| u.total()), Some(11));
    assert_eq!(
        r.calls,
        vec![ToolCall {
            call_id: "call_1".into(),
            name: "search".into(),
            arguments: json!({"limit":3,"q":"plan"})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        }]
    );
    assert_eq!(
        r.forwarded,
        vec![reasoning(), function_call(r#"{"q":"plan","limit":3}"#)]
    );
    // A mismatched actual tier keeps usage but never yields calls.
    let r = decode_tools(&call_stream("{}", "default"), true);
    assert_eq!(r.check.outcome, CheckOutcome::Mismatch);
    assert!(r.calls.is_empty());
}
#[test]
fn calls_without_offered_tools_and_malformed_arguments_are_invalid() {
    let r = decode_tools(&call_stream("{}", "priority"), false);
    assert_eq!(r.check.outcome, CheckOutcome::Invalid);
    assert!(r.calls.is_empty());
    for arguments in [
        "[]".to_owned(),
        "{bad".to_owned(),
        format!(r#"{{"q":"{}"}}"#, "x".repeat(8 * 1024)),
    ] {
        let r = decode_tools(&call_stream(&arguments, "priority"), true);
        assert_eq!(r.check.outcome, CheckOutcome::Invalid, "{arguments:.20}");
        assert!(r.calls.is_empty());
    }
}
fn step<'a>(
    mode: StepMode,
    functions: &'a [StepFunction],
    skills: &'a [StepSkill],
    history: &'a [HistoryItem],
) -> Result<ModelStep<'a>, Box<dyn std::error::Error>> {
    Ok(ModelStep {
        id: AttemptId::from_raw(1)?,
        mode,
        context: "{\"saved\":\"exact 🌿\"}",
        functions,
        skills,
        history,
        timeout: Duration::from_secs(60),
        cancellation: Arc::new(AtomicBool::new(false)),
    })
}
#[test]
fn step_bodies_keep_plain_exact_and_send_tools_history_and_a_final_post_without_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let plain = body(&step(StepMode::Plain, &[], &[], &[])?)?;
    assert_eq!(
        plain,
        build_body(ModelProfile::High, Some("{\"saved\":\"exact 🌿\"}")).map_err(|_| "body")?
    );
    let functions = [StepFunction {
        name: "search".into(),
        description: Some("Search documents".into()),
        parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
    }];
    let skills = [StepSkill {
        plugin: "Docs".into(),
        name: "search-docs".into(),
        description: "Find documents".into(),
        text: "Search first.".into(),
    }];
    let history = [
        HistoryItem::Forwarded(reasoning()),
        HistoryItem::Forwarded(function_call("{}")),
        HistoryItem::Output {
            call_id: "call_1".into(),
            output: r#"{"tool_result_id":"tool:1"}"#.into(),
        },
    ];
    let tools: Value = serde_json::from_str(&body(&step(
        StepMode::Tools,
        &functions,
        &skills,
        &history,
    )?)?)?;
    assert_eq!(
        tools["tools"],
        json!([{"type":"namespace","name":"plugins","description":"Read-only tools of user-installed plugins","tools":[{"type":"function","name":"search","description":"Search documents","parameters":{"type":"object","properties":{"q":{"type":"string"}}},"strict":false}]}])
    );
    assert_eq!(
        tools["input"][0],
        json!({"role":"user","content":"{\"saved\":\"exact 🌿\"}"})
    );
    assert_eq!(tools["input"][1], reasoning());
    assert_eq!(tools["input"][2], function_call("{}"));
    assert_eq!(
        tools["input"][3],
        json!({"type":"function_call_output","call_id":"call_1","output":"{\"tool_result_id\":\"tool:1\"}"})
    );
    let instructions = tools["instructions"].as_str().unwrap_or_default();
    assert!(instructions.contains("tool:N"));
    assert!(instructions.contains("search-docs"));
    assert!(instructions.contains("untrusted"));
    assert_eq!(
        tools["text"],
        serde_json::from_str::<Value>(&plain)?["text"]
    );
    let last: Value =
        serde_json::from_str(&body(&step(StepMode::Final, &[], &skills, &history)?)?)?;
    assert_eq!(last["tools"], json!([]));
    assert_eq!(last["input"].as_array().map(Vec::len), Some(4));
    assert!(
        last["instructions"]
            .as_str()
            .unwrap_or_default()
            .contains("No tools are available")
    );
    Ok(())
}
#[test]
fn real_http_tool_step_posts_the_history_and_returns_the_calls()
-> Result<(), Box<dyn std::error::Error>> {
    let (url, w) = server(
        call_stream(r#"{"q":"plan"}"#, "priority"),
        "Content-Type: text/event-stream\r\n",
        "200 OK",
        false,
    )?;
    let functions = [StepFunction {
        name: "search".into(),
        description: None,
        parameters: json!({"type":"object"}),
    }];
    let history = [HistoryItem::Forwarded(reasoning())];
    let s = step(StepMode::Tools, &functions, &[], &history)?;
    let r = run_step_http(&s, "test-access", Options::test(url));
    assert_eq!(r.check.outcome, CheckOutcome::Confirmed);
    assert_eq!(r.calls.len(), 1);
    let req = w.join().map_err(|_| "server")?;
    let p = req
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .ok_or("header")?;
    let body: Value = serde_json::from_slice(&req[p + 4..])?;
    assert_eq!(body["input"][1], reasoning());
    assert_eq!(body["tools"][0]["tools"][0]["name"], "search");
    Ok(())
}
