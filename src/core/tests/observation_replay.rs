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
    tasks: HashMap<String, TaskId>,
    admission: Option<CheckAdmission>,
    dispatch: Option<CheckDispatch>,
    highlight_version: u64,
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
/// Word boxes follow the episode geometry: word `i` of `n` on line `row` starts at
/// `0.02 + dx + i * w` with `w = 0.9 / max(n, 10)`, is `0.9 * w` wide, 0.03 high, at `0.9 - 0.04 * row`.
fn word_rect(row: usize, index: usize, count: usize, dx: f64) -> NormalizedRect {
    let width = 0.9 / count.max(10) as f64;
    NormalizedRect {
        x: 0.02 + dx + width * index as f64,
        y: 0.9 - 0.04 * row as f64,
        width: width * 0.9,
        height: 0.03,
    }
}
fn close(a: NormalizedRect, b: NormalizedRect) -> bool {
    [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height]
        .iter()
        .all(|d| d.abs() < 1e-9)
}
fn expected_rect(v: &Value) -> NormalizedRect {
    let n = |k: &str| v[k].as_u64().unwrap_or_default() as usize;
    let dx = v["dx"].as_f64().unwrap_or_default();
    let (first, last) = (
        word_rect(n("line"), n("first"), n("count"), dx),
        word_rect(n("line"), n("last"), n("count"), dx),
    );
    if n("first") == n("last") {
        return first;
    }
    NormalizedRect {
        width: last.x + last.width - first.x,
        ..first
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
    fn task(&self, step: &Value, key: &str) -> Result<Task, Box<dyn std::error::Error>> {
        let label = step[key].as_str().ok_or("task label")?;
        Ok(self
            .store
            .task(*self.tasks.get(label).ok_or("known task")?)?)
    }
    fn frame_id(&self, label: &Value) -> Result<FrameId, StoreError> {
        match label.as_str().and_then(|l| self.labels.get(l)) {
            Some(id) => Ok(*id),
            // A frame that does not exist in this episode.
            None => FrameId::from_raw(1_000_000),
        }
    }
    fn answer(&self, v: &Value) -> Result<ObservationAnswer, Box<dyn std::error::Error>> {
        let mut items = Vec::new();
        for i in v["items"].as_array().into_iter().flatten() {
            let task = if let Some(label) = i["task"]["existing"].as_str() {
                ObservationTarget::Existing(*self.tasks.get(label).ok_or("existing task")?)
            } else {
                ObservationTarget::New(text(&i["task"], "new"))
            };
            let related = i["related"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| {
                    l.as_str()
                        .and_then(|l| self.tasks.get(l).copied())
                        .ok_or("related task")
                })
                .collect::<Result<_, _>>()?;
            let quotes = i["quotes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|q| {
                    Ok(ObservationQuote {
                        frame: self.frame_id(&q["frame"])?,
                        text: text(q, "text"),
                    })
                })
                .collect::<Result<_, StoreError>>()?;
            let p = &i["proposal"];
            let proposal = if p.is_object() {
                let h = &p["highlight"];
                Some(ObservationProposal {
                    message: text(p, "message"),
                    rationale: text(p, "rationale"),
                    quotes: p["quotes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| n.as_u64().map(|n| n as u32))
                        .collect(),
                    highlight: if h.is_object() {
                        Some(ObservationAnchor {
                            frame: self.frame_id(&h["frame"])?,
                            anchor: text(h, "anchor"),
                        })
                    } else {
                        None
                    },
                })
            } else {
                None
            };
            items.push(ObservationItem {
                task,
                related,
                quotes,
                proposal,
            });
        }
        Ok(ObservationAnswer { items })
    }
    fn card(&self, message: &str) -> Result<Proposal, Box<dyn std::error::Error>> {
        Ok(self
            .store
            .proposal_feed()?
            .proposals
            .into_iter()
            .find(|p| p.message == message)
            .ok_or("card")?)
    }
    fn version(&mut self, version: u64, expect: &Value, at: &str) {
        assert_eq!(version, self.store.highlight().version, "{at}");
        if let Some(changed) = expect.get("highlight_changed") {
            assert_eq!(
                &Value::Bool(version != self.highlight_version),
                changed,
                "{at}"
            );
        }
        self.highlight_version = version;
    }
    fn memory(&self, expect: &Value, at: &str) -> Result<(), Box<dyn std::error::Error>> {
        let s = &self.store;
        if let Some(want) = expect.get("tasks") {
            let titles: Vec<Value> = s
                .tasks(0, 50)?
                .items
                .into_iter()
                .map(|t| Value::String(t.text))
                .collect();
            assert_eq!(&Value::Array(titles), want, "{at} tasks");
        }
        for (label, want) in expect["evidence"].as_object().into_iter().flatten() {
            let id = *self.tasks.get(label).ok_or("evidence task")?;
            let mut texts = Vec::new();
            for e in s.evidence(id, 0, 50)?.items {
                // Screen evidence is always an assumption from the reserved namespace.
                if let Origin::External { key, .. } = &e.origin {
                    assert_eq!(key.namespace, SCREEN_NAMESPACE, "{at}");
                    assert_eq!(e.kind, AssertionKind::Assumption, "{at}");
                }
                texts.push(Value::String(e.text));
            }
            assert_eq!(&Value::Array(texts), want, "{at} evidence of {label}");
        }
        if let Some(want) = expect.get("cards") {
            let cards: Vec<Value> = s
                .proposal_feed()?
                .proposals
                .into_iter()
                .map(|p| {
                    assert!(p.citations.is_empty() && p.step_id.is_none(), "{at}");
                    serde_json::json!({"message": p.message, "visible": p.visible, "stale": p.stale})
                })
                .collect();
            assert_eq!(&Value::Array(cards), want, "{at} cards");
        }
        for (label, want) in expect["links"].as_object().into_iter().flatten() {
            let id = *self.tasks.get(label).ok_or("link task")?;
            let links: Vec<Value> = s
                .links(id, 0, 50)?
                .items
                .into_iter()
                .map(|l| {
                    let other = self
                        .tasks
                        .iter()
                        .find(|(_, t)| **t == l.other_task_id)
                        .map_or_else(String::new, |(n, _)| n.clone());
                    serde_json::json!([other, l.related])
                })
                .collect();
            assert_eq!(&Value::Array(links), want, "{at} links of {label}");
        }
        Ok(())
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
                        .enumerate()
                        .map(|(row, l)| {
                            let words: Vec<&str> = if step["words"] == true {
                                l.split_whitespace().collect()
                            } else {
                                Vec::new()
                            };
                            let row = row + step["row0"].as_u64().unwrap_or_default() as usize;
                            let dx = step["dx"].as_f64().unwrap_or_default();
                            ObservedLine {
                                text: l.to_owned(),
                                words: words
                                    .iter()
                                    .enumerate()
                                    .map(|(i, w)| ObservedWord {
                                        text: (*w).to_owned(),
                                        rect: word_rect(row, i, words.len(), dx),
                                    })
                                    .collect(),
                            }
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
                self.version(out.highlight_version, expect, at);
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
                let answer = if step["answer"].is_object() {
                    Some(self.answer(&step["answer"])?)
                } else {
                    None
                };
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
                let count = |k: &str| expect[k].as_u64().unwrap_or_default() as u32;
                assert_eq!(
                    (finish.added_cards, finish.rejected_items),
                    (count("added_cards"), count("rejected_items")),
                    "{at}"
                );
                self.version(finish.highlight_version, expect, at);
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
                    for (key, actual) in [
                        ("added_cards", send.added_cards),
                        ("rejected_items", send.rejected_items),
                    ] {
                        if let Some(v) = last.get(key) {
                            assert_eq!(Some(u64::from(actual)), v.as_u64(), "{at} {key}");
                        }
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
            "task" => {
                let t = self.store.create_task(&text(step, "title"))?;
                self.tasks.insert(text(step, "label"), t.id);
            }
            "name_task" => {
                let title = text(step, "title");
                let t = self
                    .store
                    .tasks(0, 50)?
                    .items
                    .into_iter()
                    .find(|t| t.text == title)
                    .ok_or("titled task")?;
                self.tasks.insert(text(step, "label"), t.id);
            }
            "edit_task" => {
                let t = self.task(step, "task")?;
                self.store
                    .edit_task(t.id, t.revision, &text(step, "title"))?;
            }
            "suppress" => {
                let t = self.task(step, "task")?;
                self.store
                    .set_initiative_suppressed(t.id, t.revision, true)?;
            }
            "delete_task" => {
                let t = self.task(step, "task")?;
                let others: Vec<TaskVersion> = self
                    .store
                    .deletion_impact(t.id, None, 0, 50)?
                    .items
                    .into_iter()
                    .map(|o| TaskVersion {
                        task_id: o.id,
                        revision: o.revision,
                    })
                    .collect();
                self.store.delete_task(t.id, t.revision, &others)?;
            }
            "link" => {
                let (a, b) = (self.task(step, "a")?, self.task(step, "b")?);
                self.store.set_link(
                    a.id,
                    a.revision,
                    b.id,
                    b.revision,
                    step["related"] == true,
                    now.utc,
                )?;
            }
            "hide" => {
                let card = self.card(&text(step, "message"))?;
                self.store.set_proposal_visibility(card.target(), false)?;
                let version = self.store.highlight().version;
                self.version(version, expect, at);
            }
            "copy" => {
                let card = self.card(&text(step, "message"))?;
                assert_eq!(
                    self.store.copy_proposal(card.target())?,
                    card.message,
                    "{at}"
                );
            }
            "memory" => self.memory(expect, at)?,
            "highlight" => {
                let state = self.store.highlight();
                self.version(state.version, expect, at);
                let h = state.highlight;
                assert_eq!(h.is_some(), expect["present"] == true, "{at}: {h:?}");
                if let Some(h) = h {
                    if let Some(label) = expect["frame"].as_str() {
                        assert_eq!(self.label(h.frame_id.get()), label, "{at}");
                    }
                    if let Some(r) = expect.get("rect") {
                        assert!(close(h.rect, expected_rect(r)), "{at}: {:?}", h.rect);
                    }
                    assert_eq!(h.needs_recheck, expect["needs_recheck"] == true, "{at}");
                    if let Some(m) = expect["card"].as_str() {
                        assert_eq!(self.card(m)?.id, h.proposal_id, "{at}");
                    }
                }
            }
            "restart" => {
                let path = self.db.0.clone();
                self.store = Store::open(&path)?;
                self.admission = None;
                self.dispatch = None;
                // The highlight is process memory; a new process starts at version 0.
                self.highlight_version = 0;
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
    assert_eq!(paths.len(), 7, "episodes 1-7 of ADR-025");
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
            tasks: HashMap::new(),
            admission: None,
            dispatch: None,
            highlight_version: 0,
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
