//! Replay runner for the observation core (ADR-025 «Testing Decisions»): JSON episodes from
//! `tests/replay/observation` run against a real temporary SQLite with an injected clock.
use ambient_core::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const BASE_UTC: i64 = 1_798_761_600;
const BASE_MONOTONIC_MS: u64 = 7_000_000;
const IMAGE: &[u8] = b"\xff\xd8replay\xff\xd9";

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-observation-{}-{}.sqlite3",
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

struct Run {
    db: Database,
    store: Store,
    labels: HashMap<String, FrameId>,
    admission: Option<CheckAdmission>,
    dispatch: Option<CheckDispatch>,
}

fn time(t: i64) -> ObservationTime {
    ObservationTime {
        utc: BASE_UTC + t,
        monotonic_ms: BASE_MONOTONIC_MS + 1000 * t as u64,
    }
}
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}
fn expected_next(v: &Value) -> Capture {
    match v.as_u64() {
        Some(seconds) => Capture::After {
            seconds: seconds as u32,
        },
        None => {
            assert_eq!(v, "hold");
            Capture::Hold
        }
    }
}
fn outcome(name: &str) -> CheckOutcome {
    match name {
        "confirmed" => CheckOutcome::Confirmed,
        "provider" => CheckOutcome::Provider,
        "quota" => CheckOutcome::Quota,
        "timeout" => CheckOutcome::Timeout,
        "invalid" => CheckOutcome::Invalid,
        "cancelled" => CheckOutcome::Cancelled,
        other => panic!("unknown outcome {other}"),
    }
}
fn block(name: &str) -> ObservationBlock {
    match name {
        "not_started" => ObservationBlock::NotStarted,
        "paused" => ObservationBlock::Paused,
        "cloud_unavailable" => ObservationBlock::CloudUnavailable,
        "not_due" => ObservationBlock::NotDue,
        "hourly_limit" => ObservationBlock::HourlyLimit,
        "busy" => ObservationBlock::Busy,
        other => panic!("unknown block {other}"),
    }
}
fn disposition(name: &str) -> FrameDisposition {
    match name {
        "recorded" => FrameDisposition::Recorded,
        "duplicate" => FrameDisposition::Duplicate,
        "not_started" => FrameDisposition::NotStarted,
        "paused" => FrameDisposition::Paused,
        "cloud_unavailable" => FrameDisposition::CloudUnavailable,
        other => panic!("unknown disposition {other}"),
    }
}
fn frame_text(step: &Value) -> String {
    if let Some(repeat) = step.get("repeat") {
        let line = text(repeat, "line");
        (1..=repeat["count"].as_u64().unwrap_or_default())
            .map(|i| line.replace("{i}", &i.to_string()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text(step, "text")
    }
}

impl Run {
    fn label(&self, id: u64) -> String {
        self.labels
            .iter()
            .find(|(_, f)| f.get() == id)
            .map_or_else(|| format!("#{id}"), |(l, _)| l.clone())
    }
    fn step(&mut self, step: &Value, at: &str) -> Result<(), Box<dyn std::error::Error>> {
        let now = time(step["t"].as_i64().unwrap_or_default());
        let expect = &step["expect"];
        match step["op"].as_str().unwrap_or_default() {
            "policy" => {
                let policy = ObservationPolicy {
                    started: step["started"] == true,
                    per_hour_limit: step["per_hour_limit"].as_u64().unwrap_or_default() as u32,
                    credit_acknowledged: step["credit_acknowledged"] == true,
                };
                let result = self.store.set_observation_policy(policy);
                assert_eq!(result.is_err(), step["expect_error"] == true, "{at}");
            }
            "session" => self.store.set_model_session(
                step["epoch"].as_u64().unwrap_or_default(),
                step["valid"] == true,
                step["reported"] == true,
            )?,
            "pause" => self.store.set_manual_pause(step["paused"] == true)?,
            "connectivity_check" => {
                let admission = self.store.prepare_check(true, ModelProfile::High)?;
                let flag = admission.cancellation();
                let dispatch = self.store.dispatch_check(admission, now.utc)?;
                self.store.finish_check(dispatch.id(), confirmed())?;
                self.store.release_check(&flag);
            }
            "assistance_available" => {
                assert_eq!(self.store.assistance_available()?, expect == true, "{at}")
            }
            "frame" => {
                let body = frame_text(step);
                let frame = ObservedFrame {
                    display_key: text(step, "display"),
                    bundle_id: text(step, "bundle"),
                    app_name: text(step, "app"),
                    title: text(step, "title"),
                    idle_seconds: step["idle"].as_u64().unwrap_or_default() as u32,
                    lines: body
                        .lines()
                        .map(|l| ObservedLine {
                            text: l.to_owned(),
                            words: Vec::new(),
                        })
                        .collect(),
                    image: if step["image"] == true {
                        IMAGE.to_vec()
                    } else {
                        Vec::new()
                    },
                };
                let out = self.store.submit_frame(frame, now)?;
                assert_eq!(
                    out.disposition,
                    disposition(expect["disposition"].as_str().unwrap_or_default()),
                    "{at}"
                );
                assert_eq!(out.next, expected_next(&expect["next"]), "{at}");
                assert_eq!(out.highlight_version, 0, "{at}");
                if let (Some(label), Some(id)) = (step["label"].as_str(), out.frame) {
                    self.labels.insert(label.to_owned(), id);
                }
            }
            "tick" => {
                let next = self.store.observation_tick(
                    step["idle"].as_u64().unwrap_or_default() as u32,
                    step["app_switched"] == true,
                    now,
                )?;
                assert_eq!(next, expected_next(&expect["next"]), "{at}");
            }
            "begin" => match self.store.begin_observation(now)? {
                ObservationBegin::Ready(admission) => {
                    assert_eq!(expect, "ready", "{at}");
                    self.admission = Some(admission);
                }
                ObservationBegin::Blocked(b) => {
                    assert_ne!(expect, "ready", "{at}: blocked {b:?}");
                    assert_eq!(b, block(expect.as_str().unwrap_or_default()), "{at}")
                }
            },
            "dispatch" => {
                let admission = self.admission.take().ok_or("begin before dispatch")?;
                let dispatch = self.store.dispatch_check(admission, now.utc)?;
                assert_eq!(dispatch.purpose(), ModelPurpose::Observation, "{at}");
                assert_eq!(dispatch.profile(), ModelProfile::High, "{at}");
                assert_eq!(dispatch.context(), None, "{at}");
                let request = dispatch.observation().ok_or("observation request")?;
                let context: Value = serde_json::from_str(request.context())?;
                let frames: Vec<String> = context["frames"]
                    .as_array()
                    .ok_or("frames")?
                    .iter()
                    .map(|f| self.label(f["frame_id"].as_u64().unwrap_or_default()))
                    .collect();
                let want: Vec<String> = expect["frames"]
                    .as_array()
                    .ok_or("expected frames")?
                    .iter()
                    .map(|l| l.as_str().unwrap_or_default().to_owned())
                    .collect();
                assert_eq!(frames, want, "{at}");
                for key in ["dropped_frames", "dropped_bytes", "dropped_images"] {
                    if let Some(v) = expect.get(key) {
                        assert_eq!(&context[key], v, "{at} {key}");
                    }
                }
                if let Some(v) = expect.get("images") {
                    assert_eq!(
                        request.images().len() as u64,
                        v.as_u64().unwrap_or(9),
                        "{at}"
                    );
                    let marked = context["frames"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|f| f["image"] == true)
                        .count();
                    assert_eq!(marked, request.images().len(), "{at}");
                }
                self.dispatch = Some(dispatch);
            }
            "finish" => {
                let dispatch = self.dispatch.take().ok_or("dispatch before finish")?;
                let flag = dispatch.cancellation();
                let mut result = if step["outcome"] == "confirmed" {
                    confirmed()
                } else {
                    ModelCheckResult::unknown(outcome(step["outcome"].as_str().unwrap_or_default()))
                };
                if step["outcome"] != "confirmed" {
                    result.usage = None;
                }
                let answer = step["answer"]
                    .is_object()
                    .then(|| ObservationAnswer { items: Vec::new() });
                let finish = self
                    .store
                    .finish_observation(dispatch.id(), result, answer, now)?;
                self.store.release_check(&flag);
                assert_eq!(
                    finish.result.outcome,
                    outcome(expect["outcome"].as_str().unwrap_or_default()),
                    "{at}"
                );
                assert_eq!(finish.applied, expect["applied"] == true, "{at}");
                assert_eq!((finish.added_cards, finish.rejected_items), (0, 0), "{at}");
            }
            "journal" => {
                let j = self.store.observation_journal(now.utc)?;
                let checks: [(&str, u64); 4] = [
                    ("sends", j.sends.len() as u64),
                    ("log_frames", j.log_frames),
                    ("unsent_frames", j.unsent_frames),
                    ("hour_attempts", j.hour_attempts),
                ];
                for (key, actual) in checks {
                    if let Some(v) = expect.get(key) {
                        assert_eq!(Some(actual), v.as_u64(), "{at} {key}");
                    }
                }
                if let Some(v) = expect.get("started") {
                    assert_eq!(&Value::Bool(j.policy.started), v, "{at}");
                }
                if let Some(v) = expect.get("credit_acknowledged") {
                    assert_eq!(&Value::Bool(j.policy.credit_acknowledged), v, "{at}");
                }
                if let Some(last) = expect.get("last") {
                    let send = j.sends.first().ok_or("a send")?;
                    if let Some(o) = last["outcome"].as_str() {
                        assert_eq!(send.outcome, outcome(o) as u32, "{at}");
                    }
                    if let Some(range) = last["frames"].as_array() {
                        let got = [
                            self.label(send.first_frame.get()),
                            self.label(send.last_frame.get()),
                        ];
                        assert_eq!(
                            got.iter().map(String::as_str).collect::<Vec<_>>(),
                            range.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
                            "{at}"
                        );
                    }
                }
            }
            "delete_log" => self.store.delete_observation_log()?,
            "restart" => {
                let path = self.db.0.clone();
                self.store = Store::open(&path)?;
                self.admission = None;
                self.dispatch = None;
            }
            other => panic!("{at}: unknown op {other}"),
        }
        Ok(())
    }
}

fn confirmed() -> ModelCheckResult {
    ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Default,
        },
        usage: Some(CheckUsage::new(40, 10, 50).unwrap_or_else(|e| panic!("usage {e:?}"))),
    }
}

fn episodes() -> std::io::Result<Vec<PathBuf>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/replay/observation");
    let mut paths = std::fs::read_dir(&dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|p| p.extension().is_some_and(|x| x == "json"));
    paths.sort();
    Ok(paths)
}

#[test]
fn observation_episodes_replay_with_recorded_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let paths = episodes()?;
    assert_eq!(paths.len(), 3, "episodes 2, 3 and 6 of ADR-025");
    for path in paths {
        let episode: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        // .claude/rules/replay.md: every episode records its provenance and bases.
        for key in ["source", "consent", "time_basis", "geometry", "summary"] {
            assert!(!episode[key].is_null(), "{name}: missing {key}");
        }
        assert_eq!(episode["schema_version"], 17, "{name}");
        let db = Database::new();
        let store = Store::open(&db.0)?;
        let mut run = Run {
            db,
            store,
            labels: HashMap::new(),
            admission: None,
            dispatch: None,
        };
        let mut last = i64::MIN;
        for (i, step) in episode["steps"]
            .as_array()
            .ok_or("steps")?
            .iter()
            .enumerate()
        {
            let t = step["t"].as_i64().ok_or("t")?;
            assert!(t >= last, "{name} step {i}: time goes backwards");
            last = t;
            let at = format!("{name} step {i} ({} at t={t})", step["op"]);
            run.step(step, &at)
                .unwrap_or_else(|e| panic!("{at}: unexpected {e:?}"));
        }
    }
    Ok(())
}
