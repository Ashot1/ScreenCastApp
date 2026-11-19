use ambient_core::*;
use rusqlite::Connection;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-observation-store-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn at(t: i64) -> ObservationTime {
    ObservationTime {
        utc: 1_798_761_600 + t,
        monotonic_ms: 1_000_000 + 1000 * t as u64,
    }
}
fn frame(text: &str) -> ObservedFrame {
    ObservedFrame {
        display_key: "display-A".into(),
        bundle_id: "com.example.editor".into(),
        app_name: "Editor".into(),
        title: "Draft".into(),
        idle_seconds: 0,
        lines: text
            .lines()
            .map(|l| ObservedLine {
                text: l.into(),
                words: Vec::new(),
            })
            .collect(),
        image: vec![1, 2, 3],
    }
}
fn started(s: &mut Store) -> Result<(), StoreError> {
    s.set_observation_policy(ObservationPolicy {
        started: true,
        per_hour_limit: 20,
        credit_acknowledged: true,
    })?;
    s.set_model_session(1, true, true)
}
fn confirmed() -> ModelCheckResult {
    ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Default,
        },
        usage: Some(CheckUsage::new(3, 2, 5).unwrap_or_else(|e| panic!("usage {e:?}"))),
    }
}
/// One significant frame, settled, sent and finished with the given result.
fn observe(s: &mut Store, t: i64, result: ModelCheckResult) -> Result<Value, StoreError> {
    s.submit_frame(
        frame(&format!("Notes {t}\nalpha{t} bravo{t} charlie{t} delta{t} echo{t} foxtrot{t} golf{t} hotel{t} india{t} juliet{t}")),
        at(t),
    )?;
    let ObservationBegin::Ready(admission) = s.begin_observation(at(t + 11))? else {
        return Err(StoreError::NotFound);
    };
    let flag = admission.cancellation();
    let d = s.dispatch_check(admission, at(t + 11).utc)?;
    let context = serde_json::from_str(d.observation().ok_or(StoreError::NotFound)?.context())
        .map_err(|_| StoreError::InvalidInput)?;
    s.finish_observation(
        d.id(),
        result,
        Some(ObservationAnswer { items: Vec::new() }),
        at(t + 12),
    )?;
    s.release_check(&flag);
    Ok(context)
}

#[test]
fn observation_attempts_count_in_totals_but_never_become_the_latest_check()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(d.id(), confirmed())?;
    s.release_check(&flag);
    observe(&mut s, 10, confirmed())?;
    let summary = s.check_summary()?;
    assert_eq!((summary.attempts, summary.total), (2, 10));
    let latest = summary.latest.ok_or("latest")?;
    assert_eq!(latest.purpose, ModelPurpose::Connectivity);
    Ok(())
}

#[test]
fn folding_old_attempts_keeps_observation_usage_in_its_totals()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    drop(Store::open(&db.0)?);
    let c = Connection::open(&db.0)?;
    // 4096 completed observation rows with unknown usage fill the live journal.
    c.execute_batch("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<4096) INSERT INTO connection_checks SELECT i,3,i,2,3,1,2,1,0,0,0,NULL,NULL,NULL FROM n")?;
    drop(c);
    let mut s = Store::open(&db.0)?;
    s.set_model_session(1, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 5000)?;
    s.finish_check(d.id(), confirmed())?;
    s.release_check(&flag);
    let summary = s.check_summary()?;
    assert_eq!(
        (summary.attempts, summary.unknown, summary.total),
        (4097, 4096, 5)
    );
    Ok(())
}

#[test]
fn policy_is_bounded_at_the_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    assert_eq!(
        s.observation_policy()?,
        ObservationPolicy {
            started: false,
            per_hour_limit: 20,
            credit_acknowledged: false
        }
    );
    for json in [
        r#"{"started":true,"per_hour_limit":0,"credit_acknowledged":true}"#,
        r#"{"started":true,"per_hour_limit":61,"credit_acknowledged":true}"#,
        r#"{"started":true,"per_hour_limit":20,"credit_acknowledged":false}"#,
        r#"{"started":true,"per_hour_limit":20,"credit_acknowledged":true,"extra":1}"#,
        r#"{"started":true,"per_hour_limit":20}"#,
    ] {
        assert_eq!(
            s.set_observation_policy_json(json).err(),
            Some(StoreError::InvalidInput),
            "{json}"
        );
    }
    s.set_observation_policy_json(
        r#"{"started":true,"per_hour_limit":60,"credit_acknowledged":true}"#,
    )?;
    drop(s);
    assert_eq!(Store::open(&db.0)?.observation_policy()?.per_hour_limit, 60);
    Ok(())
}

#[test]
fn frames_are_validated_normalized_and_bounded_with_counted_losses()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    let mut bad = frame("text");
    bad.image.clear();
    assert_eq!(
        s.submit_frame(bad, at(0)).err(),
        Some(StoreError::InvalidInput)
    );
    let mut bad = frame("text");
    bad.display_key.clear();
    assert_eq!(
        s.submit_frame(bad, at(0)).err(),
        Some(StoreError::InvalidInput)
    );
    let mut bad = frame("text");
    bad.lines[0].words.push(ObservedWord {
        text: "text".into(),
        rect: NormalizedRect {
            x: 0.9,
            y: 0.1,
            width: 0.2,
            height: f64::NAN,
        },
    });
    assert_eq!(
        s.submit_frame(bad, at(0)).err(),
        Some(StoreError::InvalidInput)
    );
    // Control characters go, whitespace collapses, and the cut beyond 32 KiB is a counted loss.
    let long = format!("  héllo\u{1}  wörld \t\n\n{}", "é".repeat(20_000));
    let out = s.submit_frame(frame(&long), at(1))?;
    assert_eq!(out.disposition, FrameDisposition::Recorded);
    assert_eq!(out.next, Capture::After { seconds: 5 });
    let ObservationBegin::Ready(a) = s.begin_observation(at(20))? else {
        return Err("not ready".into());
    };
    let d = s.dispatch_check(a, at(20).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    let text = context["frames"][0]["text"].as_str().ok_or("text")?;
    assert!(text.starts_with("héllo wörld\néé"));
    assert_eq!(text.len(), MAX_FRAME_TEXT);
    let full = "héllo wörld\n".len() + 40_000;
    assert_eq!(context["dropped_bytes"], (full - MAX_FRAME_TEXT) as u64);
    Ok(())
}

#[test]
fn request_lists_live_tasks_allows_only_open_ones_and_details_open_plans()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    let done = s.create_task("Finished launch checklist")?;
    let planned = s.create_task("Prepare the quarterly review")?;
    let noted = s.create_task("Reply to the vendor")?;
    let deleted = s.create_task("Old idea")?;
    s.add_note(
        noted.id,
        noted.revision,
        "The vendor asked for a call on Friday",
        1,
    )?;
    s.complete_task(done.id, done.revision, "Launched", &[], &[], &[])?;
    s.delete_task(deleted.id, deleted.revision, &[])?;
    s.add_step(
        planned.id,
        planned.revision,
        &StepInput {
            description: "Collect the numbers".into(),
            prompt: None,
            reference: None,
        },
    )?;
    let context = observe(&mut s, 0, confirmed())?;
    let ids = |key: &str| -> Vec<i64> {
        context[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["id"].as_i64())
            .collect()
    };
    assert_eq!(ids("tasks"), vec![noted.id.0, planned.id.0, done.id.0]);
    assert_eq!(ids("allowed_tasks"), vec![noted.id.0, planned.id.0]);
    assert_eq!(ids("task_details"), vec![planned.id.0, noted.id.0]);
    let revision = s.task(planned.id)?.revision;
    assert_eq!(context["allowed_tasks"][1]["revision"], revision);
    assert_eq!(
        context["task_details"][0]["steps"][0]["description"],
        "Collect the numbers"
    );
    assert_eq!(
        context["task_details"][1]["notes"][0]["text"],
        "The vendor asked for a call on Friday"
    );
    assert_eq!(context["dropped_tasks"], 0);
    Ok(())
}

#[test]
fn delete_log_clears_frames_and_sends_but_keeps_attempt_accounting()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    observe(&mut s, 0, confirmed())?;
    s.submit_frame(frame("unsent alpha bravo"), at(40))?;
    let j = s.observation_journal(at(41).utc)?;
    assert_eq!((j.log_frames, j.unsent_frames, j.sends.len()), (2, 1, 1));
    assert_eq!(j.sends[0].tier, ActualTier::Default as u32);
    assert_eq!(j.sends[0].apps, vec!["Editor".to_owned()]);
    s.delete_observation_log()?;
    let j = s.observation_journal(at(42).utc)?;
    assert_eq!(
        (
            j.log_frames,
            j.unsent_frames,
            j.sends.len(),
            j.hour_attempts
        ),
        (0, 0, 0, 1)
    );
    assert_eq!(s.check_summary()?.attempts, 1);
    Ok(())
}

#[test]
fn frame_metadata_counts_against_the_request_bound_so_begin_never_overflows()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    // Empty screens in windows with changing long titles are all distinct rows.
    for i in 0..300 {
        let mut f = frame("");
        f.lines.clear();
        f.title = format!("Window {i} {}", "x".repeat(990));
        s.submit_frame(f, at(i))?;
    }
    let ObservationBegin::Ready(a) = s.begin_observation(at(320))? else {
        return Err("not ready".into());
    };
    let d = s.dispatch_check(a, at(320).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    let sent = context["frames"].as_array().ok_or("frames")?.len() as u64;
    assert!(serde_json::to_string(&context["frames"])?.len() <= MAX_REQUEST_FRAMES);
    assert_eq!(
        sent + context["dropped_frames"].as_u64().ok_or("dropped")?,
        300
    );
    assert!(sent > 0 && sent < 300);
    Ok(())
}

#[test]
fn deleting_the_log_during_a_send_does_not_bring_old_losses_back()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    s.submit_frame(frame("old alpha bravo charlie"), at(0))?;
    // One day later the unsent row is pruned into a pending loss of the next request.
    s.submit_frame(
        frame(&format!(
            "new {}",
            "delta echo foxtrot golf hotel india juliet kilo lima mike"
        )),
        at(86_401),
    )?;
    let ObservationBegin::Ready(a) = s.begin_observation(at(86_420))? else {
        return Err("not ready".into());
    };
    let flag = a.cancellation();
    let d = s.dispatch_check(a, at(86_420).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    assert_eq!(context["dropped_frames"], 1);
    s.delete_observation_log()?;
    let finish = s.finish_observation(d.id(), confirmed(), None, at(86_421))?;
    s.release_check(&flag);
    assert_eq!(finish.result.outcome, CheckOutcome::Cancelled);
    s.submit_frame(
        frame("fresh november oscar papa quebec romeo sierra tango uniform victor"),
        at(87_000),
    )?;
    let ObservationBegin::Ready(a) = s.begin_observation(at(87_020))? else {
        return Err("not ready".into());
    };
    let d = s.dispatch_check(a, at(87_020).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    assert_eq!(
        (
            &context["dropped_frames"],
            &context["dropped_bytes"],
            &context["dropped_images"]
        ),
        (&Value::from(0), &Value::from(0), &Value::from(0))
    );
    Ok(())
}

#[test]
fn a_retry_reports_ring_images_left_out_of_the_failed_send()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    // Two frames near the text limit: only the newer fits the request, both images are in the ring.
    for (t, word) in [(0, "older"), (1, "newer")] {
        s.submit_frame(frame(&format!("{word} {}", "ab ".repeat(12_000))), at(t))?;
    }
    let ObservationBegin::Ready(a) = s.begin_observation(at(20))? else {
        return Err("not ready".into());
    };
    let flag = a.cancellation();
    let d = s.dispatch_check(a, at(20).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    assert_eq!(
        (&context["dropped_frames"], &context["dropped_images"]),
        (&Value::from(1), &Value::from(1))
    );
    s.finish_observation(
        d.id(),
        ModelCheckResult::unknown(CheckOutcome::Provider),
        None,
        at(21),
    )?;
    s.release_check(&flag);
    s.set_model_session(2, true, true)?;
    let ObservationBegin::Ready(a) = s.begin_observation(at(40))? else {
        return Err("not ready".into());
    };
    let d = s.dispatch_check(a, at(40).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    // Both images are gone now: the cut one and the one spent on the failed send.
    assert_eq!(
        (&context["dropped_frames"], &context["dropped_images"]),
        (&Value::from(1), &Value::from(2))
    );
    Ok(())
}

#[test]
fn the_newest_frame_is_always_sent_even_when_escaping_doubles_it()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    s.submit_frame(frame(&format!("quoted {}", "\"\\".repeat(17_000))), at(0))?;
    let ObservationBegin::Ready(a) = s.begin_observation(at(20))? else {
        return Err("not ready".into());
    };
    let d = s.dispatch_check(a, at(20).utc)?;
    let context: Value = serde_json::from_str(d.observation().ok_or("request")?.context())?;
    assert_eq!(context["frames"].as_array().ok_or("frames")?.len(), 1);
    Ok(())
}

#[test]
fn duplicate_image_replacement_evicts_oldest_and_reports_the_image_loss()
-> Result<(), Box<dyn std::error::Error>> {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    started(&mut s)?;
    let mut newer = frame("newer screen");
    newer.image = vec![2; 768 * 1024];
    // Exactly 1 MiB fits before replacing the older display's duplicate JPEG.
    let mut duplicate = frame("older screen");
    duplicate.display_key = "display-B".into();
    // Keep the older text as the last frame on its own display.
    duplicate.image = vec![1; 256 * 1024];
    s.submit_frame(duplicate.clone(), at(0))?;
    let newest = s.submit_frame(newer, at(1))?.frame.ok_or("frame")?;
    duplicate.image = vec![3; 768 * 1024];
    assert_eq!(
        s.submit_frame(duplicate, at(4))?.disposition,
        FrameDisposition::Duplicate
    );
    let ObservationBegin::Ready(a) = s.begin_observation(at(20))? else {
        return Err("not ready".into());
    };
    let d = s.dispatch_check(a, at(20).utc)?;
    let request = d.observation().ok_or("request")?;
    assert_eq!(request.images().len(), 1);
    assert_eq!(request.images()[0].frame, newest);
    assert_eq!(request.images()[0].jpeg.len(), 768 * 1024);
    let context: Value = serde_json::from_str(request.context())?;
    assert_eq!(context["dropped_images"], 1);
    assert_eq!(context["frames"].as_array().ok_or("frames")?.len(), 2);
    Ok(())
}
