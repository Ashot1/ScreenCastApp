use super::*;
use crate::model::{AmbientModelCompletion, AmbientModelSummary};
use std::sync::atomic::AtomicU64;

static NEXT: AtomicU64 = AtomicU64::new(1);
fn memory() -> Result<(u64, std::path::PathBuf), Box<dyn std::error::Error>> {
    let p = std::env::temp_dir().join(format!(
        "ambient-observe-ffi-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let b = p.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    assert_eq!(
        unsafe { crate::ambient_v3_open(b.as_ptr(), b.len() as u32, &mut h) },
        0
    );
    Ok((h, p))
}
/// A frame owned by the test: one string buffer, its lines and words, and the JPEG.
struct Frame {
    strings: Vec<u8>,
    spans: [AmbientSpan; 4],
    lines: Vec<AmbientObservedLine>,
    words: Vec<AmbientObservedWord>,
    pub jpeg: Vec<u8>,
}
fn span(strings: &mut Vec<u8>, text: &str) -> AmbientSpan {
    let s = AmbientSpan {
        offset: strings.len() as u32,
        length: text.len() as u32,
    };
    strings.extend_from_slice(text.as_bytes());
    s
}
impl Frame {
    /// Each line's words are laid out left to right, 0.1 wide, on row `y`.
    pub fn new(app: &str, lines: &[&str]) -> Self {
        let mut strings = Vec::new();
        let spans = [
            span(&mut strings, "display-1"),
            span(&mut strings, "com.example.editor"),
            span(&mut strings, app),
            span(&mut strings, "Report"),
        ];
        let (mut l, mut w) = (Vec::new(), Vec::new());
        for (row, line) in lines.iter().enumerate() {
            let text = span(&mut strings, line);
            let mut count = 0;
            for (i, word) in line.split(' ').enumerate() {
                w.push(AmbientObservedWord {
                    text: span(&mut strings, word),
                    rect: AmbientObservedRect {
                        x: 0.1 * i as f64,
                        y: 0.9 - 0.05 * row as f64,
                        width: 0.1,
                        height: 0.04,
                    },
                });
                count += 1;
            }
            l.push(AmbientObservedLine {
                text,
                word_count: count,
            });
        }
        Self {
            strings,
            spans,
            lines: l,
            words: w,
            jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
        }
    }
    pub fn abi(&self) -> AmbientObservedFrame {
        AmbientObservedFrame {
            strings: AmbientText {
                bytes: self.strings.as_ptr(),
                length: self.strings.len() as u32,
            },
            display_key: self.spans[0],
            bundle_id: self.spans[1],
            app_name: self.spans[2],
            title: self.spans[3],
            lines: self.lines.as_ptr(),
            words: self.words.as_ptr(),
            jpeg: AmbientText {
                bytes: self.jpeg.as_ptr(),
                length: self.jpeg.len() as u32,
            },
            line_count: self.lines.len() as u32,
            word_count: self.words.len() as u32,
            idle_seconds: 0,
            reserved: 0,
        }
    }
}
fn submit(m: u64, t: i64, f: &Frame) -> (u32, AmbientObserveFrameResult) {
    let mut out = AmbientObserveFrameResult::default();
    let frame = f.abi();
    let status =
        unsafe { ambient_observe_v1_submit_frame(m, t, t as u64 * 1000, &frame, &mut out) };
    (status, out)
}
fn begin(m: u64, t: i64) -> (u32, AmbientObserveBegin) {
    let mut out = AmbientObserveBegin::default();
    let status = unsafe { ambient_observe_v1_begin(m, t, t as u64 * 1000, &mut out) };
    (status, out)
}
fn start(m: u64) {
    let policy = br#"{"started":true,"per_hour_limit":20,"credit_acknowledged":true}"#;
    assert_eq!(
        unsafe {
            ambient_observe_v1_set_policy(
                m,
                AmbientText {
                    bytes: policy.as_ptr(),
                    length: policy.len() as u32,
                },
            )
        },
        0
    );
    assert_eq!(crate::ambient_model_v6_session(m, 7, 1, 1), 0);
}
fn json(
    m: u64,
    call: unsafe extern "C" fn(u64, *mut u8, u32, *mut u32) -> u32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut bytes = vec![0u8; 1 << 20];
    let mut length = 0;
    assert_eq!(
        unsafe { call(m, bytes.as_mut_ptr(), bytes.len() as u32, &mut length) },
        0
    );
    Ok(serde_json::from_slice(&bytes[..length as usize])?)
}
unsafe extern "C" fn journal(m: u64, b: *mut u8, c: u32, l: *mut u32) -> u32 {
    unsafe { ambient_observe_v1_journal(m, 100, b, c, l) }
}
const TYPO: &str = "Quarterly teh report is due";

#[test]
fn malformed_frames_are_refused_before_core_use_and_leave_the_output_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    start(m);
    let f = Frame::new("Editor", &[TYPO, "Second line"]);
    let edits: Vec<fn(&mut AmbientObservedFrame)> = vec![
        |v| v.title.offset = v.strings.length,
        |v| v.title.length = u32::MAX,
        |v| v.word_count -= 1,
        |v| v.line_count -= 1,
        |v| v.lines = std::ptr::null(),
        |v| v.reserved = 1,
        |v| v.jpeg.length = 0,
        |v| v.strings.bytes = std::ptr::null(),
    ];
    // A word whose text splits a UTF-8 sequence.
    let mut broken = Frame::new("Редактор", &["Привет мир"]);
    broken.words[0].text.length = 1;
    let mut frames = edits
        .into_iter()
        .map(|edit| {
            let mut frame = f.abi();
            edit(&mut frame);
            frame
        })
        .collect::<Vec<_>>();
    frames.push(broken.abi());
    for frame in frames {
        let mut out = AmbientObserveFrameResult {
            frame_id: 99,
            ..Default::default()
        };
        assert_eq!(
            unsafe { ambient_observe_v1_submit_frame(m, 10, 10_000, &frame, &mut out) },
            1
        );
        assert_eq!(out.frame_id, 99);
    }
    assert_eq!(json(m, journal)?["log_frames"], 0);
    let (status, out) = submit(m, 10, &f);
    assert_eq!((status, out.disposition, out.next.capture), (0, 1, 1));
    assert!(out.frame_id > 0);
    let (status, again) = submit(m, 12, &f);
    assert_eq!((status, again.disposition, again.frame_id), (0, 2, 0));
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
#[test]
fn begin_reports_block_reasons_without_an_operation() -> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    assert_eq!(begin(m, 1).1.block, 1);
    let mut tick = AmbientObserveNext::default();
    assert_eq!(
        unsafe { ambient_observe_v1_tick(m, 1, 1000, 0, 2, &mut tick) },
        1
    );
    assert_eq!(
        unsafe { ambient_observe_v1_tick(m, 1, 1000, 0, 0, &mut tick) },
        0
    );
    assert_eq!(tick.capture, 2);
    start(m);
    assert_eq!(
        unsafe { ambient_observe_v1_tick(m, 1, 1000, 0, 1, &mut tick) },
        0
    );
    assert_eq!((tick.capture, tick.seconds), (1, 0));
    assert_eq!(
        submit(m, 10, &Frame::new("Editor", &[TYPO])).1.disposition,
        1
    );
    // The screen has not settled for 10 s yet.
    assert_eq!(
        begin(m, 15),
        (
            0,
            AmbientObserveBegin {
                operation: 0,
                block: 1,
                reserved: 0
            }
        )
    );
    let (status, ready) = begin(m, 21);
    assert_eq!((status, ready.block), (0, 0));
    assert!(ready.operation > 0);
    assert_eq!(
        begin(m, 22).1,
        AmbientObserveBegin {
            operation: 0,
            block: 2,
            reserved: 0
        }
    );
    assert_eq!(crate::ambient_model_v6_close(ready.operation), 0);
    assert_eq!(crate::ambient_model_v6_session(m, 8, 0, 1), 0);
    assert_eq!(begin(m, 23).1.block, 3);
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
fn answer() -> ModelResponse {
    ModelResponse {
        check: ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::High,
                tier: ActualTier::Default,
            },
            usage: CheckUsage::new(900, 40, 940).ok(),
        },
        ..ModelResponse::unknown(CheckOutcome::Confirmed)
    }
}
#[test]
fn observation_dispatch_finishes_its_own_attempt_and_cancel_keeps_the_manual_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    start(m);
    let preview = crate::model::memory(m, |s| {
        // Manual help needs a confirmed check (unlike observation, D103).
        let a = s
            .prepare_check(true, ModelProfile::High)
            .map_err(crate::model::error)?;
        let flag = a.cancellation();
        let d = s.dispatch_check(a, 1).map_err(crate::model::error)?;
        s.finish_check(d.id(), answer().check)
            .map_err(crate::model::error)?;
        s.release_check(&flag);
        let task = s.create_task("saved intent").map_err(crate::model::error)?;
        s.prepare_assistance(AssistanceRequest {
            task: TaskVersion {
                task_id: task.id,
                revision: task.revision,
            },
            workspace_revision: 1,
            mode: AssistanceMode::NewContext,
            selected_evidence: vec![],
            reply: None,
        })
        .map_err(crate::model::error)
    })
    .map_err(|e| format!("preview {e}"))?;
    assert_eq!(
        submit(m, 10, &Frame::new("Editor", &[TYPO])).1.disposition,
        1
    );
    let op = begin(m, 21).1.operation;
    let mut out = AmbientModelCompletion::default();
    let status = crate::model::dispatch_with(
        op,
        "test",
        &Vec::new(),
        std::path::Path::new("/nonexistent-root"),
        &mut out,
        |_| panic!("observation is not a connectivity check"),
        |d| {
            let r = d
                .observation()
                .map(|r| (r.context().to_owned(), r.images().len()));
            assert!(
                r.as_ref()
                    .is_some_and(|(c, n)| c.contains("teh report") && *n == 1)
            );
            let mut a = answer();
            a.observation = Some(ObservationAnswer { items: vec![] });
            a
        },
        |_| panic!("observation has no plugin loop"),
        |_, _, _| panic!("observation calls no tools"),
    );
    assert_eq!(status, 0);
    assert_eq!(
        (out.attempt.purpose, out.attempt.outcome, out.attempt.stage),
        (3, CheckOutcome::Confirmed as u32, 2)
    );
    assert_eq!(
        (out.attempt.total_tokens, out.attempt.actual_tier),
        (940, ActualTier::Default as u32)
    );
    assert_eq!((out.published_task, out.workspace_revision), (0, 0));
    assert_eq!(crate::ambient_model_v6_cancel(op), 0);
    assert_eq!(crate::ambient_model_v6_close(op), 0);
    // Neither the observation's cancel nor its close discards the user's manual preview.
    crate::model::memory(m, |s| {
        s.assistance_preview(preview.request_id)
            .map(|_| ())
            .map_err(crate::model::error)
    })
    .map_err(|e| format!("preview kept {e}"))?;
    let j = json(m, journal)?;
    assert_eq!(j["sends"][0]["attempt"], out.attempt.id);
    assert_eq!(
        (j["sends"][0]["outcome"].clone(), j["hour_attempts"].clone()),
        (1.into(), 1.into())
    );
    // The summary still shows only connectivity and assistance attempts; totals include this one.
    let mut summary = AmbientModelSummary::default();
    assert_eq!(
        unsafe { crate::ambient_model_v6_summary(m, &mut summary) },
        0
    );
    assert_eq!(
        (
            summary.has_latest,
            summary.latest.purpose,
            summary.total_tokens
        ),
        (1, 1, 1880)
    );
    // An unused observation admission closed without dispatch keeps the preview too.
    assert_eq!(
        submit(
            m,
            40,
            &Frame::new(
                "Mail",
                &[
                    "Meeting notes: budget review moved to Thursday",
                    "afternoon, bring the revised forecast",
                    "and the staffing plan for approval",
                ]
            )
        )
        .1
        .disposition,
        1
    );
    let op = begin(m, 51).1.operation;
    assert!(op > 0);
    assert_eq!(crate::ambient_model_v6_close(op), 0);
    crate::model::memory(m, |s| {
        s.assistance_preview(preview.request_id)
            .map(|_| ())
            .map_err(crate::model::error)
    })
    .map_err(|e| format!("preview kept after close {e}"))?;
    assert_eq!(ambient_observe_v1_delete_log(m), 0);
    assert_eq!(json(m, journal)?["sends"], serde_json::json!([]));
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
#[test]
fn cancel_during_the_post_discards_a_confirmed_answer_and_counts_the_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    start(m);
    let frame = FrameId::from_raw(submit(m, 10, &Frame::new("Editor", &[TYPO])).1.frame_id)?;
    let op = begin(m, 21).1.operation;
    let mut out = AmbientModelCompletion::default();
    let status = crate::model::dispatch_with(
        op,
        "test",
        &Vec::new(),
        std::path::Path::new("/nonexistent-root"),
        &mut out,
        |_| panic!("observation is not a connectivity check"),
        |_| {
            // The user stops while the request is in flight; the model still answers.
            assert_eq!(crate::ambient_model_v6_cancel(op), 0);
            let mut a = answer();
            a.observation = Some(ObservationAnswer {
                items: vec![ObservationItem {
                    task: ObservationTarget::New("Fix the typo".into()),
                    related: vec![],
                    quotes: vec![ObservationQuote {
                        frame,
                        text: "teh report".into(),
                    }],
                    proposal: Some(ObservationProposal {
                        message: "Typo: teh".into(),
                        rationale: "Seen on screen".into(),
                        quotes: vec![0],
                        highlight: None,
                    }),
                }],
            });
            a
        },
        |_| panic!("observation has no plugin loop"),
        |_, _, _| panic!("observation calls no tools"),
    );
    assert_eq!(status, 0);
    assert_eq!(
        (out.attempt.outcome, out.attempt.total_tokens),
        (CheckOutcome::Cancelled as u32, 940)
    );
    assert_eq!(crate::ambient_model_v6_close(op), 0);
    let j = json(m, journal)?;
    assert_eq!(
        (
            j["sends"][0]["outcome"].clone(),
            j["sends"][0]["added_cards"].clone(),
            j["hour_attempts"].clone()
        ),
        (
            serde_json::json!(CheckOutcome::Cancelled as u32),
            serde_json::json!(0),
            serde_json::json!(1)
        )
    );
    let tasks = crate::model::memory(m, |s| s.tasks(0, 50).map_err(crate::model::error))
        .map_err(|e| format!("tasks {e}"))?;
    assert!(tasks.items.is_empty());
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
#[test]
fn json_outputs_report_the_needed_length_when_the_buffer_is_small()
-> Result<(), Box<dyn std::error::Error>> {
    let (m, p) = memory()?;
    let mut bytes = [7u8; 4];
    let mut length = 0;
    assert_eq!(
        unsafe { ambient_observe_v1_highlight(m, bytes.as_mut_ptr(), 4, &mut length) },
        6
    );
    assert_eq!(bytes, [7; 4]);
    assert_eq!(
        json(m, ambient_observe_v1_highlight)?,
        serde_json::json!({"version":0,"highlight":null})
    );
    let needed = serde_json::to_string(&json(m, journal)?)?.len() as u32;
    assert_eq!(unsafe { journal(m, bytes.as_mut_ptr(), 4, &mut length) }, 6);
    assert_eq!(length, needed);
    let bad = b"{\"started\":true}";
    assert_eq!(
        unsafe {
            ambient_observe_v1_set_policy(
                m,
                AmbientText {
                    bytes: bad.as_ptr(),
                    length: bad.len() as u32,
                },
            )
        },
        1
    );
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
/// One scripted SSE answer on loopback; returns the raw request.
#[cfg(feature = "model-test-support")]
fn serve_once(sse: String) -> Result<(String, std::thread::JoinHandle<Vec<u8>>), std::io::Error> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/responses", listener.local_addr()?);
    let worker = std::thread::spawn(move || {
        let Ok((mut s, _)) = listener.accept() else {
            return Vec::new();
        };
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let (mut input, mut buf) = (Vec::new(), [0u8; 65536]);
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
            input.extend_from_slice(&buf[..n]);
            if let Some(p) = input.windows(4).position(|v| v == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&input[..p]).to_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: ")?.parse::<usize>().ok())
                    .unwrap_or(0);
                if input.len() >= p + 4 + len {
                    break;
                }
            }
        }
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
            sse.len()
        );
        input
    });
    Ok((url, worker))
}
#[cfg(feature = "model-test-support")]
#[test]
fn loopback_observation_sends_the_screenshot_and_applies_the_answer_end_to_end()
-> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    let (m, p) = memory()?;
    start(m);
    let mut f = Frame::new("Editor", &[TYPO, "Owner: Dana"]);
    f.jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x10, 0x20, 0xff, 0xd9];
    let (_, first) = submit(m, 10, &f);
    let frame = first.frame_id;
    let ready = begin(m, 21).1;
    assert_eq!(ready.block, 0);
    let text = json!({"items":[{"task_id":null,"new_task":"Fix the quarterly report typo","related_task_ids":[],"quotes":[{"frame_id":frame,"text":"teh report"}],"proposal":{"message":"Typo: «teh» should be «the»","rationale":"Seen in the report title","quote_indexes":[0],"highlight":{"frame_id":frame,"anchor":"teh report"}}}]}).to_string();
    let item = json!({"id":"msg","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]});
    let sse = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"status":"completed","model":"gpt-6-luna","reasoning":{"effort":"high"},"service_tier":"default","output":[item],"usage":{"input_tokens":1200,"output_tokens":60,"total_tokens":1260}}})
    );
    let (url, server) = serve_once(sse)?;
    let mut out = AmbientModelCompletion::default();
    let root = b"/nonexistent-root";
    assert_eq!(
        unsafe {
            crate::model::support::ambient_model_test_dispatch(
                ready.operation,
                b"test-access".as_ptr(),
                11,
                std::ptr::null(),
                0,
                AmbientText {
                    bytes: root.as_ptr(),
                    length: root.len() as u32,
                },
                url.as_ptr(),
                url.len() as u32,
                &mut out,
            )
        },
        0
    );
    assert_eq!(crate::ambient_model_v6_close(ready.operation), 0);
    assert_eq!(
        (
            out.attempt.purpose,
            out.attempt.outcome,
            out.attempt.total_tokens
        ),
        (3, CheckOutcome::Confirmed as u32, 1260)
    );
    // The one POST: no tools, the context, then the labelled screenshot as a data URL.
    let request = server.join().map_err(|_| "server")?;
    let at = request
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .ok_or("head")?;
    let head = String::from_utf8_lossy(&request[..at]).to_lowercase();
    assert!(
        head.starts_with("post /responses ") && head.contains("authorization: bearer test-access")
    );
    let body: serde_json::Value = serde_json::from_slice(&request[at + 4..])?;
    assert_eq!(body["tools"], json!([]));
    assert_eq!(body["text"]["format"]["name"], "screen_observation");
    let content = &body["input"][0]["content"];
    assert!(
        content[0]["text"]
            .as_str()
            .is_some_and(|c| c.contains(TYPO))
    );
    assert_eq!(
        content[1]["text"],
        format!("Screenshot of frame_id {frame}:")
    );
    assert_eq!(
        content[2],
        json!({"type":"input_image","image_url":"data:image/jpeg;base64,/9j/4BAg/9k=","detail":"high"})
    );
    assert_eq!(content.as_array().map(Vec::len), Some(3));
    // The answer is applied: the send journal, the card and the highlight on the anchor's words.
    let j = json(m, journal)?;
    assert_eq!(
        (
            j["sends"][0]["outcome"].clone(),
            j["sends"][0]["added_cards"].clone(),
            j["sends"][0]["images"].clone()
        ),
        (json!(1), json!(1), json!(1))
    );
    let h = json(m, ambient_observe_v1_highlight)?;
    assert_eq!(h["highlight"]["frame_id"], frame);
    assert_eq!(h["highlight"]["anchor"], "teh report");
    assert_eq!(h["highlight"]["display_key"], "display-1");
    let rect = &h["highlight"]["rect"];
    let near =
        |v: &serde_json::Value, want: f64| v.as_f64().is_some_and(|v| (v - want).abs() < 1e-9);
    // The union of the boxes of «teh» and «report».
    assert!(near(&rect["x"], 0.1) && near(&rect["width"], 0.2) && near(&rect["height"], 0.04));
    let mut feed = vec![0u8; MAX_PROPOSAL_FEED];
    let mut length = 0;
    assert_eq!(
        unsafe {
            crate::ambient_model_v6_feed(m, feed.as_mut_ptr(), feed.len() as u32, &mut length)
        },
        0
    );
    let feed = String::from_utf8(feed[..length as usize].to_vec())?;
    assert!(feed.contains("Typo: «teh» should be «the»"));
    // The corrected screen removes the highlight; the frame result's version tells native to look.
    let (_, fixed) = submit(
        m,
        30,
        &Frame::new("Editor", &["Quarterly the report is due", "Owner: Dana"]),
    );
    assert!(fixed.highlight_version > h["version"].as_u64().unwrap_or(u64::MAX));
    assert_eq!(
        json(m, ambient_observe_v1_highlight)?["highlight"],
        serde_json::Value::Null
    );
    assert_eq!(crate::ambient_v3_close(m), 0);
    std::fs::remove_file(p)?;
    Ok(())
}
