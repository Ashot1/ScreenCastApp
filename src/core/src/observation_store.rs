use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, atomic::Ordering};

const DAY: i64 = 86_400;
const HOUR: i64 = 3_600;
const SETTLE_MS: u64 = 10_000;
const BACKOFF_MS: u64 = 5 * 60 * 1000;
const USAGE_LIMIT_BACKOFF_MS: u64 = 60 * 60 * 1000;
// New distinct words since the last send; digits-only tokens (clocks, counters) never count.
const SIGNIFICANT_CHARS: usize = 80;
const MAX_LOG_BYTES: i64 = 16 * 1024 * 1024;
const MAX_LOG_FRAMES: i64 = 20_000;
// Twenty-four hours at the highest hourly limit.
const MAX_SENDS: i64 = 24 * 60;
const RING_FRAMES: usize = 2;
const RING_BYTES: usize = 1024 * 1024;
const MAX_KEY: usize = 128;
const MAX_BUNDLE: usize = 256;
const MAX_APP: usize = 256;
const MAX_TITLE: usize = 1024;
const MAX_LINES: usize = 4096;
const MAX_WORDS: usize = 16_384;
const MAX_WORD: usize = 256;
const MAX_APPS: usize = 16;
const MAX_TASK_LIST: usize = 32 * 1024;
const MAX_TASK_TITLE: usize = 200;
const MAX_DETAILS: usize = 48 * 1024;
const DETAIL_TASKS: i64 = 8;
// Avalanche guards of one answer (ADR-025 «Проверки пунктов»).
const MAX_NEW_TASKS: usize = 3;
const MAX_EVIDENCE_ROWS: usize = 16;
const MAX_ANCHOR: usize = 1024;
const QUOTE_SCOPE: &str = "quote";
// How far, in display units, a re-found anchor may move between frames of one display.
const NEAR: f64 = 0.1;

/// Process-local observation state. SQLite owns the log and the unsent set; this holds only the
/// in-memory image ring with its word boxes, in-session deadlines, a derived significance cache
/// and the highlight.
#[derive(Default)]
pub(super) struct ObservationState {
    ring: VecDeque<RingFrame>,
    /// Word boxes of the frames in flight: their images left with the send (D34), the boxes stay
    /// until the answer is settled so its anchor can be found.
    sent: Vec<FrameShape>,
    settled_since: Option<u64>,
    backoff_until: Option<u64>,
    novelty: Option<Novelty>,
    highlight: Option<Highlight>,
    highlight_version: u64,
}
#[derive(Clone)]
struct FrameShape {
    frame: FrameId,
    display_key: String,
    lines: Vec<ObservedLine>,
}
#[derive(Clone)]
struct RingFrame {
    shape: FrameShape,
    jpeg: Arc<[u8]>,
}
/// Significance of the unsent frames against the newest send. Rebuilt from SQLite whenever the
/// unsent set or the baseline changes other than by an append.
#[derive(Default)]
struct Novelty {
    seen: BTreeSet<String>,
    new_chars: usize,
    sent_pair: Option<(String, String)>,
    app_changed: bool,
    unsent: bool,
}
impl Novelty {
    fn significant(&self) -> bool {
        self.unsent && (self.app_changed || self.new_chars >= SIGNIFICANT_CHARS)
    }
    fn feed(&mut self, bundle: &str, title: &str, text: &str) {
        self.unsent = true;
        if self
            .sent_pair
            .as_ref()
            .is_none_or(|(b, t)| b != bundle || t != title)
        {
            self.app_changed = true;
        }
        // Once significant the set is no longer needed, which also bounds its size.
        if self.significant() {
            return;
        }
        for word in words(text) {
            if self.seen.insert(word.to_owned()) {
                self.new_chars += word.chars().count();
            }
        }
    }
}
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
}
/// One line of screen text: control characters dropped, whitespace runs collapsed.
fn line(text: &str) -> String {
    text.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
fn bounded(text: String, limit: usize, lost: &mut u64) -> String {
    if text.len() <= limit {
        return text;
    }
    let cut = text.floor_char_boundary(limit);
    *lost += (text.len() - cut) as u64;
    text[..cut].to_owned()
}
fn prefix(text: &str, limit: usize) -> &str {
    &text[..text.floor_char_boundary(limit)]
}
fn text_hash(text: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"ambient-observation-text\x01");
    h.update(text.as_bytes());
    h.finalize().into()
}
fn cadence(idle_seconds: u32, app_switched: bool) -> Capture {
    match (app_switched, idle_seconds) {
        (true, _) => Capture::After { seconds: 0 },
        (false, 0..30) => Capture::After { seconds: 5 },
        (false, 30..600) => Capture::After { seconds: 30 },
        _ => Capture::Hold,
    }
}
fn key(text: &str, limit: usize, empty: bool) -> Result<(), StoreError> {
    if (!empty && text.is_empty()) || text.len() > limit || text.contains('\0') {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_frame(frame: &ObservedFrame) -> Result<(), StoreError> {
    key(&frame.display_key, MAX_KEY, false)?;
    key(&frame.bundle_id, MAX_BUNDLE, true)?;
    if frame.image.is_empty()
        || frame.image.len() > MAX_FRAME_IMAGE
        || frame.lines.len() > MAX_LINES
        || frame.lines.iter().map(|l| l.words.len()).sum::<usize>() > MAX_WORDS
    {
        return Err(StoreError::InvalidInput);
    }
    let unit = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
    for word in frame.lines.iter().flat_map(|l| &l.words) {
        let r = word.rect;
        if word.text.len() > MAX_WORD
            || ![r.x, r.y, r.width, r.height].into_iter().all(unit)
            || r.x + r.width > 1.0 + f64::EPSILON
            || r.y + r.height > 1.0 + f64::EPSILON
        {
            return Err(StoreError::InvalidInput);
        }
    }
    Ok(())
}
fn add_losses(c: &Connection, l: Losses) -> Result<(), StoreError> {
    c.execute(
        "UPDATE settings SET dropped_frames=dropped_frames+?1,dropped_bytes=dropped_bytes+?2,dropped_images=dropped_images+?3",
        params![l.frames as i64, l.bytes as i64, l.images as i64],
    )?;
    Ok(())
}
/// Deletes rows older than a day, then the oldest rows until `rows`/`bytes` more fit. Deleted
/// unsent rows are recorded as losses of the next request.
fn expire_frames(c: &Connection, now_utc: i64, rows: i64, bytes: i64) -> Result<bool, StoreError> {
    let cutoff = now_utc.saturating_sub(DAY);
    let (mut count, mut total): (i64, i64) = c.query_row(
        "SELECT count(*),coalesce(sum(length(CAST(text AS BLOB))),0) FROM observation_frames",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (mut until, mut lost) = (None, Losses::default());
    let mut q = c.prepare(
        "SELECT id,at_utc,length(CAST(text AS BLOB)),sent_in IS NULL FROM observation_frames ORDER BY id",
    )?;
    let mut all = q.query([])?;
    while let Some(r) = all.next()? {
        let (id, at, len, unsent): (i64, i64, i64, bool) =
            (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        if at >= cutoff && count + rows <= MAX_LOG_FRAMES && total + bytes <= MAX_LOG_BYTES {
            break;
        }
        (count, total, until) = (count - 1, total - len, Some(id));
        if unsent {
            lost.frames += 1;
            lost.bytes += len as u64;
        }
    }
    drop(all);
    let Some(until) = until else {
        return Ok(false);
    };
    c.execute("DELETE FROM observation_frames WHERE id<=?1", [until])?;
    add_losses(c, lost)?;
    Ok(true)
}
fn expire_sends(c: &Connection, now_utc: i64, rows: i64) -> Result<(), StoreError> {
    c.execute(
        "DELETE FROM observation_sends WHERE at_utc<?1",
        [now_utc.saturating_sub(DAY)],
    )?;
    let count: i64 = c.query_row("SELECT count(*) FROM observation_sends", [], |r| r.get(0))?;
    if count + rows > MAX_SENDS {
        c.execute(
            "DELETE FROM observation_sends WHERE attempt_id IN (SELECT attempt_id FROM observation_sends ORDER BY attempt_id LIMIT ?1)",
            [count + rows - MAX_SENDS],
        )?;
    }
    Ok(())
}
fn novelty(c: &Connection) -> Result<Novelty, StoreError> {
    let mut n = Novelty::default();
    let last: Option<i64> =
        c.query_row("SELECT max(sent_in) FROM observation_frames", [], |r| {
            r.get(0)
        })?;
    if let Some(last) = last {
        // The baseline is what the newest send carried: its frames from the oldest one included.
        let mut q = c.prepare(
            "SELECT bundle_id,title,text FROM observation_frames WHERE sent_in=?1 AND id>=coalesce((SELECT first_frame FROM observation_sends WHERE attempt_id=?1),0) ORDER BY id DESC",
        )?;
        let mut rows = q.query([last])?;
        while let Some(r) = rows.next()? {
            let text: String = r.get(2)?;
            if n.sent_pair.is_none() {
                n.sent_pair = Some((r.get(0)?, r.get(1)?));
            }
            n.seen.extend(words(&text).map(str::to_owned));
        }
    }
    let mut q = c.prepare(
        "SELECT bundle_id,title,text FROM observation_frames WHERE sent_in IS NULL ORDER BY id",
    )?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        let (bundle, title, text): (String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
        n.feed(&bundle, &title, &text);
        if n.significant() {
            break;
        }
    }
    Ok(n)
}
fn policy(c: &Connection) -> Result<ObservationPolicy, StoreError> {
    Ok(c.query_row(
        "SELECT observation_started,per_hour_limit,credit_acknowledged FROM settings",
        [],
        |r| {
            Ok(ObservationPolicy {
                started: r.get(0)?,
                per_hour_limit: r.get(1)?,
                credit_acknowledged: r.get(2)?,
            })
        },
    )?)
}
fn hour_attempts(c: &Connection, now_utc: i64) -> Result<u64, StoreError> {
    // Every reserved observation row counts, failed and interrupted ones included (ADR-025).
    let n: i64 = c.query_row(
        "SELECT count(*) FROM connection_checks WHERE purpose=?1 AND at_utc>?2",
        params![
            ModelPurpose::Observation as u32,
            now_utc.saturating_sub(HOUR)
        ],
        |r| r.get(0),
    )?;
    Ok(n as u64)
}
/// Commits the send inside the attempt row's transaction: the frames up to the newest one of the
/// request (older dropped ones included) leave the unsent set, and the pending losses it reported
/// are consumed.
pub(super) fn reserve_send(
    c: &Connection,
    attempt: AttemptId,
    r: &ObservationRequest,
    at_utc: i64,
) -> Result<(), StoreError> {
    let first = r.frames.last().map_or(r.last_frame, |f| f.id);
    c.execute(
        "UPDATE observation_frames SET sent_in=?1 WHERE sent_in IS NULL AND id<=?2",
        params![attempt.0, r.last_frame.0],
    )?;
    expire_sends(c, at_utc, 1)?;
    let apps = serde_json::to_string(&r.apps).map_err(|_| StoreError::InvalidInput)?;
    c.execute(
        "INSERT INTO observation_sends VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0,0,0)",
        params![
            attempt.0,
            at_utc,
            first.0,
            r.last_frame.0,
            apps,
            r.images.len() as i64,
            r.dropped.frames as i64,
            r.dropped.bytes as i64,
            r.dropped.images as i64
        ],
    )?;
    c.execute(
        "UPDATE settings SET dropped_frames=max(dropped_frames-?1,0),dropped_bytes=max(dropped_bytes-?2,0),dropped_images=max(dropped_images-?3,0)",
        params![
            r.pending.frames as i64,
            r.pending.bytes as i64,
            r.pending.images as i64
        ],
    )?;
    Ok(())
}
/// Settles the send inside the transaction that completes its attempt row, so the journal and
/// the unsent set never diverge from the attempt accounting.
pub(super) fn settle_send(
    c: &Connection,
    attempt: AttemptId,
    result: &ModelCheckResult,
    r: &ObservationRequest,
) -> Result<(), StoreError> {
    c.execute(
        "UPDATE observation_sends SET outcome=?1,tier=?2 WHERE attempt_id=?3",
        params![result.outcome as u32, result.actual.tier as u32, attempt.0],
    )?;
    if retried(result.outcome) {
        let returned = c.execute(
            "UPDATE observation_frames SET sent_in=NULL WHERE sent_in=?1",
            [attempt.0],
        )?;
        // The returning frames carry the losses this send consumed; their images were spent on it
        // or cut from it. After a deleted log nothing returns, and neither do its old losses.
        if returned > 0 {
            add_losses(
                c,
                Losses {
                    images: r.dropped.images + r.images.len() as u64,
                    ..r.pending
                },
            )?;
        }
    }
    Ok(())
}
/// Outcomes after which the model did not consume the frames, so the next send carries them again.
fn retried(outcome: CheckOutcome) -> bool {
    matches!(
        outcome,
        CheckOutcome::Provider
            | CheckOutcome::Timeout
            | CheckOutcome::Local
            | CheckOutcome::Quota
            | CheckOutcome::Cancelled
    )
}
pub(super) fn validate_observation(c: &Connection) -> Result<(), StoreError> {
    let (frames, bytes): (i64, i64) = c.query_row(
        "SELECT count(*),coalesce(sum(length(CAST(text AS BLOB))),0) FROM observation_frames",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let sends: i64 = c.query_row("SELECT count(*) FROM observation_sends", [], |r| r.get(0))?;
    if frames > MAX_LOG_FRAMES || bytes > MAX_LOG_BYTES || sends > MAX_SENDS {
        return Err(StoreError::UnsupportedSchema);
    }
    let mut q = c.prepare("SELECT apps FROM observation_sends")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        let apps: Vec<String> = serde_json::from_str(&r.get::<_, String>(0)?)
            .map_err(|_| StoreError::UnsupportedSchema)?;
        if apps.len() > MAX_APPS {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    Ok(())
}

impl Store {
    pub fn observation_policy(&self) -> Result<ObservationPolicy, StoreError> {
        policy(&self.connection)
    }
    pub fn set_observation_policy(&mut self, policy: ObservationPolicy) -> Result<(), StoreError> {
        // The acknowledgement is given by the start and kept until stop (D103).
        if !(1..=60).contains(&policy.per_hour_limit)
            || policy.started != policy.credit_acknowledged
        {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c| {
            c.execute(
                "UPDATE settings SET observation_started=?1,per_hour_limit=?2,credit_acknowledged=?3",
                params![policy.started, policy.per_hour_limit, policy.credit_acknowledged],
            )?;
            Ok(())
        })?;
        if !policy.started {
            self.stop_observation();
        }
        Ok(())
    }
    pub fn set_observation_policy_json(&mut self, json: &str) -> Result<(), StoreError> {
        self.set_observation_policy(
            serde_json::from_str(json).map_err(|_| StoreError::InvalidInput)?,
        )
    }
    /// Stop and pause end the attempt in flight and drop the in-memory images, word boxes and the
    /// highlight; the log stays (D101).
    pub(super) fn stop_observation(&mut self) {
        if let Some(a) = &self.active_check
            && a.purpose == ModelPurpose::Observation
        {
            a.cancelled.store(true, Ordering::Release);
        }
        self.observation.ring.clear();
        self.observation.sent.clear();
        self.set_highlight(None);
    }
    pub fn highlight(&self) -> HighlightState {
        HighlightState {
            version: self.observation.highlight_version,
            highlight: self.observation.highlight.clone(),
        }
    }
    fn set_highlight(&mut self, highlight: Option<Highlight>) {
        if self.observation.highlight != highlight {
            self.observation.highlight = highlight;
            self.observation.highlight_version += 1;
        }
    }
    /// The highlight belongs to a shown card: hiding, eviction, a stale-set cleanup, a manual
    /// publication or a deleted task removes it with the card. Runs after committed writes, so it
    /// cannot fail them: if the card cannot be read, the derived in-memory highlight is dropped.
    pub(super) fn invalidate_stale_highlight(&mut self) {
        let Some(h) = &self.observation.highlight else {
            return;
        };
        let shown = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assistant_proposals WHERE id=?1 AND visible=1)",
                [h.proposal_id.0],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !shown {
            self.set_highlight(None);
        }
    }
    fn follow_highlight(&mut self, shape: &FrameShape) {
        let next = self
            .observation
            .highlight
            .as_ref()
            .and_then(|h| follow(h, shape));
        self.set_highlight(next);
    }
    /// A new session ends the backoff (D48).
    pub(super) fn reset_observation_backoff(&mut self) {
        self.observation.backoff_until = None;
    }
    fn observation_block(
        &self,
        now: ObservationTime,
    ) -> Result<Option<ObservationBlock>, StoreError> {
        Ok(if !self.observation_policy()?.started {
            Some(ObservationBlock::NotStarted)
        } else if self.manual_pause()? {
            Some(ObservationBlock::Paused)
        } else if !self.model_session.valid
            || !self.model_session.reported
            || self
                .observation
                .backoff_until
                .is_some_and(|t| now.monotonic_ms < t)
        {
            Some(ObservationBlock::CloudUnavailable)
        } else {
            None
        })
    }
    /// The dispatch fence of an observation admission: still started, same reported session.
    pub(super) fn require_observation(&self, epoch: u64) -> Result<(), StoreError> {
        if !self.observation_policy()?.started
            || !self.model_session.valid
            || !self.model_session.reported
            || epoch != self.model_session.epoch
        {
            return Err(StoreError::Cancelled);
        }
        Ok(())
    }
    /// After the send row commits, its images leave memory (D34) and the baseline moves. The word
    /// boxes of the sent frames wait for the answer.
    pub(super) fn observation_sent(&mut self, r: &ObservationRequest) {
        let (sent, kept) = std::mem::take(&mut self.observation.ring)
            .into_iter()
            .partition::<Vec<_>, _>(|f| f.shape.frame <= r.last_frame);
        self.observation.sent = sent.into_iter().map(|f| f.shape).collect();
        self.observation.ring = kept.into();
        self.observation.novelty = None;
    }
    fn expire_observation(&mut self, now_utc: i64) -> Result<(), StoreError> {
        let stale: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM observation_frames WHERE at_utc<?1) OR EXISTS(SELECT 1 FROM observation_sends WHERE at_utc<?1)",
            [now_utc.saturating_sub(DAY)],
            |r| r.get(0),
        )?;
        if stale {
            self.change(|c| {
                expire_frames(c, now_utc, 0, 0)?;
                expire_sends(c, now_utc, 0)
            })?;
            self.observation.novelty = None;
        }
        Ok(())
    }
    fn significant(&mut self) -> Result<bool, StoreError> {
        if self.observation.novelty.is_none() {
            self.observation.novelty = Some(novelty(&self.connection)?);
        }
        Ok(self
            .observation
            .novelty
            .as_ref()
            .is_some_and(Novelty::significant))
    }
    pub fn submit_frame(
        &mut self,
        frame: ObservedFrame,
        now: ObservationTime,
    ) -> Result<FrameOutcome, StoreError> {
        validate_frame(&frame)?;
        if let Some(block) = self.observation_block(now)? {
            return Ok(FrameOutcome {
                disposition: match block {
                    ObservationBlock::NotStarted => FrameDisposition::NotStarted,
                    ObservationBlock::Paused => FrameDisposition::Paused,
                    _ => FrameDisposition::CloudUnavailable,
                },
                frame: None,
                next: Capture::Hold,
                highlight_version: self.observation.highlight_version,
            });
        }
        let mut lost = 0;
        let text = bounded(
            frame
                .lines
                .iter()
                .map(|l| line(&l.text))
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            MAX_FRAME_TEXT,
            &mut lost,
        );
        let app = bounded(line(&frame.app_name), MAX_APP, &mut lost);
        let title = bounded(line(&frame.title), MAX_TITLE, &mut lost);
        let hash = text_hash(&text);
        let jpeg: Arc<[u8]> = frame.image.into();
        let next = cadence(frame.idle_seconds, false);
        let previous: Option<(i64, Vec<u8>, String, String)> = self
            .connection
            .query_row(
                "SELECT id,text_hash,bundle_id,title FROM observation_frames WHERE display_key=?1 ORDER BY id DESC LIMIT 1",
                [&frame.display_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        // Dedupe only against the previous frame of the same display, so A->B->A keeps B.
        if let Some((id, h, bundle, t)) = previous
            && h == hash
            && bundle == frame.bundle_id
            && t == title
        {
            self.change(|c| {
                c.execute(
                    "UPDATE observation_frames SET last_seen_utc=max(last_seen_utc,?1) WHERE id=?2",
                    params![now.utc, id],
                )?;
                Ok(())
            })?;
            // The same text may have moved: the newest image and word boxes replace the kept copy.
            let shape = FrameShape {
                frame: FrameId(id),
                display_key: frame.display_key,
                lines: frame.lines,
            };
            self.follow_highlight(&shape);
            if let Some(f) = self
                .observation
                .ring
                .iter_mut()
                .find(|f| f.shape.frame == shape.frame)
            {
                f.jpeg = jpeg;
                f.shape = shape;
            } else if let Some(f) = self
                .observation
                .sent
                .iter_mut()
                .find(|f| f.frame == shape.frame)
            {
                *f = shape;
            }
            return Ok(FrameOutcome {
                disposition: FrameDisposition::Duplicate,
                frame: None,
                next,
                highlight_version: self.observation.highlight_version,
            });
        }
        let mut ring = self.observation.ring.clone();
        let mut evicted = 0;
        let (id, pruned) = self.change(|c| {
            let pruned = expire_frames(c, now.utc, 1, text.len() as i64)?;
            c.execute(
                "INSERT INTO observation_frames(at_utc,last_seen_utc,display_key,bundle_id,app_name,title,text,text_hash,sent_in) VALUES(?1,?1,?2,?3,?4,?5,?6,?7,NULL)",
                params![now.utc, frame.display_key, frame.bundle_id, app, title, text, hash.as_slice()],
            )?;
            let id = FrameId(c.last_insert_rowid());
            ring.push_back(RingFrame {
                shape: FrameShape {
                    frame: id,
                    display_key: frame.display_key,
                    lines: frame.lines,
                },
                jpeg,
            });
            while ring.len() > RING_FRAMES || ring.iter().map(|f| f.jpeg.len()).sum::<usize>() > RING_BYTES {
                ring.pop_front();
                evicted += 1;
            }
            add_losses(
                c,
                Losses {
                    frames: 0,
                    bytes: lost,
                    images: evicted,
                },
            )?;
            Ok((id, pruned))
        })?;
        if let Some(f) = ring.back() {
            self.follow_highlight(&f.shape);
        }
        self.observation.ring = ring;
        self.observation.settled_since = Some(now.monotonic_ms);
        if pruned {
            self.observation.novelty = None;
        } else if let Some(n) = &mut self.observation.novelty {
            n.feed(&frame.bundle_id, &title, &text);
        }
        Ok(FrameOutcome {
            disposition: FrameDisposition::Recorded,
            frame: Some(id),
            next,
            highlight_version: self.observation.highlight_version,
        })
    }
    /// The snapshot-free cadence call while input is idle; it also expires the day-old log.
    pub fn observation_tick(
        &mut self,
        idle_seconds: u32,
        app_switched: bool,
        now: ObservationTime,
    ) -> Result<Capture, StoreError> {
        self.expire_observation(now.utc)?;
        Ok(if self.observation_block(now)?.is_some() {
            Capture::Hold
        } else {
            cadence(idle_seconds, app_switched)
        })
    }
    pub fn begin_observation(
        &mut self,
        now: ObservationTime,
    ) -> Result<ObservationBegin, StoreError> {
        if let Some(block) = self.observation_block(now)? {
            return Ok(ObservationBegin::Blocked(block));
        }
        self.expire_observation(now.utc)?;
        // Identical frames do not restart settling; only a different one does.
        let settling = self
            .observation
            .settled_since
            .is_some_and(|t| now.monotonic_ms.saturating_sub(t) < SETTLE_MS);
        if settling || !self.significant()? {
            return Ok(ObservationBegin::Blocked(ObservationBlock::NotDue));
        }
        if hour_attempts(&self.connection, now.utc)?
            >= u64::from(self.observation_policy()?.per_hour_limit)
        {
            return Ok(ObservationBegin::Blocked(ObservationBlock::HourlyLimit));
        }
        if self.active_check.is_some() {
            return Ok(ObservationBegin::Blocked(ObservationBlock::Busy));
        }
        let request = Arc::new(self.observation_request()?);
        let admission = self.reserve_model(ModelPurpose::Observation, ModelProfile::High, None)?;
        if let Some(active) = &mut self.active_check {
            active.observation = Some(request);
        }
        Ok(ObservationBegin::Ready(admission))
    }
    fn observation_request(&self) -> Result<ObservationRequest, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let pending = tx.query_row(
            "SELECT dropped_frames,dropped_bytes,dropped_images FROM settings",
            [],
            |r| {
                Ok(Losses {
                    frames: r.get::<_, i64>(0)? as u64,
                    bytes: r.get::<_, i64>(1)? as u64,
                    images: r.get::<_, i64>(2)? as u64,
                })
            },
        )?;
        let mut frames = Vec::new();
        let mut frames_json = Vec::new();
        let mut apps: Vec<String> = Vec::new();
        // The serialized array: brackets, entries and their separating commas.
        let mut frames_bytes = 1;
        let mut oldest = i64::MAX;
        {
            let mut q = tx.prepare(
                "SELECT id,at_utc,bundle_id,app_name,title,text FROM observation_frames WHERE sent_in IS NULL ORDER BY id DESC",
            )?;
            let mut rows = q.query([])?;
            while let Some(r) = rows.next()? {
                let text: String = r.get(5)?;
                let id = FrameId(r.get(0)?);
                let app: String = r.get(3)?;
                let image = self.observation.ring.iter().any(|f| f.shape.frame == id);
                let entry = json!({"frame_id": id, "at_utc": r.get::<_, i64>(1)?, "bundle_id": r.get::<_, String>(2)?, "app": app, "title": r.get::<_, String>(4)?, "text": text, "image": image});
                // Metadata and escaping count too, so the whole context stays within its bound.
                let size = entry.to_string().len() + 1;
                // The newest frame always goes: at most 32 KiB of text, even doubled by escaping.
                if !frames_json.is_empty() && frames_bytes + size > MAX_REQUEST_FRAMES {
                    break;
                }
                frames_bytes += size;
                frames_json.push(entry);
                if apps.len() < MAX_APPS && !apps.contains(&app) {
                    apps.push(app);
                }
                frames.push(SentFrame {
                    id,
                    at_utc: r.get(1)?,
                    text,
                });
                oldest = id.0;
            }
        }
        let last_frame = frames.first().map(|f| f.id).ok_or(StoreError::NotFound)?;
        // Older unsent frames beyond the text bound are dropped from this send and counted.
        let (older_frames, older_bytes): (i64, i64) = tx.query_row(
            "SELECT count(*),coalesce(sum(length(CAST(text AS BLOB))),0) FROM observation_frames WHERE sent_in IS NULL AND id<?1",
            [oldest],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let images: Vec<ObservationImage> = self
            .observation
            .ring
            .iter()
            .filter(|f| frames.iter().any(|s| s.id == f.shape.frame))
            .map(|f| ObservationImage {
                frame: f.shape.frame,
                jpeg: f.jpeg.clone(),
            })
            .collect();
        let unsent_images = self
            .observation
            .ring
            .iter()
            .filter(|f| f.shape.frame <= last_frame)
            .count();
        let dropped = Losses {
            frames: pending.frames + older_frames as u64,
            bytes: pending.bytes + older_bytes as u64,
            images: pending.images + unsent_images.saturating_sub(images.len()) as u64,
        };
        let (tasks, allowed, dropped_tasks) = task_list(&tx)?;
        let (details, dropped_details) = task_details(&tx, &allowed)?;
        let context = json!({
            "frames": frames_json,
            "dropped_frames": dropped.frames,
            "dropped_bytes": dropped.bytes,
            "dropped_images": dropped.images,
            "tasks": tasks,
            "dropped_tasks": dropped_tasks,
            "task_details": details,
            "dropped_task_details": dropped_details,
            "allowed_tasks": allowed.iter().map(|t| json!({"id": t.task_id, "revision": t.revision})).collect::<Vec<_>>(),
        })
        .to_string();
        if context.len() > MAX_OBSERVATION_CONTEXT {
            return Err(StoreError::ResourceLimit);
        }
        Ok(ObservationRequest {
            context,
            images,
            frames,
            allowed,
            last_frame,
            apps,
            dropped,
            pending,
        })
    }
    /// Accounts the attempt, then discards a late answer under the store lock or applies a current
    /// one in the same transaction, and anchors its highlight after the commit.
    pub fn finish_observation(
        &mut self,
        id: AttemptId,
        mut result: ModelCheckResult,
        answer: Option<ObservationAnswer>,
        now: ObservationTime,
    ) -> Result<ObservationFinish, StoreError> {
        let active = self
            .active_check
            .as_ref()
            .filter(|a| a.attempt == Some(id) && a.purpose == ModelPurpose::Observation)
            .ok_or(StoreError::InvalidInput)?;
        let epoch = active.epoch;
        let request = active.observation.clone().ok_or(StoreError::InvalidInput)?;
        // Pause, stop, cancel or a session change after dispatch make the answer late.
        let current = !active.cancelled.load(Ordering::Acquire)
            && self.require_observation(epoch).is_ok()
            && !self.manual_pause()?;
        if result.outcome == CheckOutcome::Confirmed {
            if !current {
                result.outcome = CheckOutcome::Cancelled;
            } else if answer.is_none() {
                result.outcome = CheckOutcome::Invalid;
            }
        }
        let same_session = epoch == self.model_session.epoch;
        let finished = self.finish_attempt(id, result, |c, result| {
            // The commit may have waited for the write lock: only a still confirmed result applies.
            match &answer {
                Some(answer) if result.outcome == CheckOutcome::Confirmed => {
                    apply_answer(c, id, &request, answer, now.utc).map(Some)
                }
                _ => Ok(None),
            }
        });
        let sent = std::mem::take(&mut self.observation.sent);
        let (result, applied) = match finished {
            Ok(finished) => finished,
            Err(error) => {
                if same_session {
                    self.observation.backoff_until =
                        Some(now.monotonic_ms.saturating_add(BACKOFF_MS));
                }
                return Err(error);
            }
        };
        if let Some(applied) = &applied {
            self.anchor_highlight(&applied.anchors, &sent);
        }
        if retried(result.outcome) {
            self.observation.novelty = None;
        }
        if same_session
            && !matches!(
                result.outcome,
                CheckOutcome::Confirmed | CheckOutcome::Cancelled
            )
        {
            let wait = if result.outcome == CheckOutcome::Quota {
                USAGE_LIMIT_BACKOFF_MS
            } else {
                BACKOFF_MS
            };
            self.observation.backoff_until = Some(now.monotonic_ms.saturating_add(wait));
        }
        Ok(ObservationFinish {
            result,
            applied: applied.is_some(),
            added_cards: applied.as_ref().map_or(0, |a| a.added_cards),
            rejected_items: applied.as_ref().map_or(0, |a| a.rejected_items),
            highlight_version: self.observation.highlight_version,
        })
    }
    /// The newest card whose anchor occurs exactly once in its frame gets the highlight. The anchor
    /// is bound to the cited frame, so a late answer cannot point to a new place; a frame recorded
    /// during the request (the ring holds only those) then moves or removes it like any newer frame.
    fn anchor_highlight(
        &mut self,
        anchors: &[(ProposalId, ObservationAnchor)],
        sent: &[FrameShape],
    ) {
        for (proposal, a) in anchors.iter().rev() {
            let Some(shape) = sent.iter().find(|f| f.frame == a.frame) else {
                continue;
            };
            let [rect] = occurrences(&shape.lines, &a.anchor)[..] else {
                continue;
            };
            let h = Highlight {
                proposal_id: *proposal,
                frame_id: shape.frame,
                display_key: shape.display_key.clone(),
                anchor: line(&a.anchor),
                rect,
                needs_recheck: false,
            };
            let h = match self.observation.ring.back() {
                Some(newest) => follow(&h, &newest.shape),
                None => Some(h),
            };
            self.set_highlight(h);
            return;
        }
    }
    pub fn observation_journal(&mut self, now_utc: i64) -> Result<ObservationJournal, StoreError> {
        self.expire_observation(now_utc)?;
        let c = &self.connection;
        let (log_frames, unsent_frames): (i64, i64) = c.query_row(
            "SELECT count(*),count(*) FILTER (WHERE sent_in IS NULL) FROM observation_frames",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut q = c.prepare("SELECT attempt_id,at_utc,first_frame,last_frame,apps,images,dropped_frames,dropped_bytes,dropped_images,outcome,tier,added_cards,rejected_items FROM observation_sends ORDER BY attempt_id DESC")?;
        let sends = q
            .query_map([], |r| {
                Ok((
                    ObservationSend {
                        attempt: r.get::<_, i64>(0)? as u64,
                        at_utc: r.get(1)?,
                        first_frame: FrameId(r.get(2)?),
                        last_frame: FrameId(r.get(3)?),
                        apps: Vec::new(),
                        images: r.get(5)?,
                        dropped_frames: r.get::<_, i64>(6)? as u64,
                        dropped_bytes: r.get::<_, i64>(7)? as u64,
                        dropped_images: r.get::<_, i64>(8)? as u64,
                        outcome: r.get(9)?,
                        tier: r.get(10)?,
                        added_cards: r.get(11)?,
                        rejected_items: r.get(12)?,
                    },
                    r.get::<_, String>(4)?,
                ))
            })?
            .map(|row| {
                let (mut send, apps) = row?;
                send.apps =
                    serde_json::from_str(&apps).map_err(|_| StoreError::UnsupportedSchema)?;
                Ok(send)
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok(ObservationJournal {
            policy: policy(c)?,
            log_frames: log_frames as u64,
            unsent_frames: unsent_frames as u64,
            hour_attempts: hour_attempts(c, now_utc)?,
            sends,
        })
    }
    /// The explicit delete of the text log and the send journal; attempt accounting stays.
    pub fn delete_observation_log(&mut self) -> Result<(), StoreError> {
        self.change(|c| {
            c.execute("DELETE FROM observation_frames", [])?;
            c.execute("DELETE FROM observation_sends", [])?;
            c.execute(
                "UPDATE settings SET dropped_frames=0,dropped_bytes=0,dropped_images=0",
                [],
            )?;
            Ok(())
        })?;
        self.stop_observation();
        self.observation.novelty = None;
        self.observation.settled_since = None;
        Ok(())
    }
}
/// All live tasks, newest first, within a byte bound; the open ones form the allow-list.
fn task_list(c: &Connection) -> Result<(Vec<Value>, Vec<TaskVersion>, u64), StoreError> {
    let mut q = c.prepare(
        "SELECT id,revision,text,lifecycle FROM tasks WHERE text IS NOT NULL ORDER BY id DESC",
    )?;
    let mut rows = q.query([])?;
    let (mut list, mut allowed, mut bytes, mut dropped) = (Vec::new(), Vec::new(), 0, 0);
    while let Some(r) = rows.next()? {
        let (id, revision, text, lifecycle): (i64, i64, String, i64) =
            (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        let entry = json!({"id": id, "revision": revision, "title": prefix(&text, MAX_TASK_TITLE)});
        let size = entry.to_string().len() + 1;
        if bytes + size > MAX_TASK_LIST {
            dropped += 1;
            continue;
        }
        bytes += size;
        list.push(entry);
        if lifecycle == TaskLifecycle::Open as i64 {
            allowed.push(TaskVersion {
                task_id: TaskId(id),
                revision: revision as u64,
            });
        }
    }
    Ok((list, allowed, dropped))
}
/// Details of open listed tasks with open plans first, then the most recently noted ones.
fn task_details(c: &Connection, allowed: &[TaskVersion]) -> Result<(Vec<Value>, u64), StoreError> {
    let mut q = c.prepare("SELECT t.id,t.revision,t.text,t.plan_completeness,t.initiative_suppressed FROM tasks t WHERE t.text IS NOT NULL AND t.lifecycle=0 ORDER BY EXISTS(SELECT 1 FROM plan_steps s WHERE s.task_id=t.id AND s.description IS NOT NULL AND s.completed_at_utc IS NULL) DESC,(SELECT coalesce(max(e.id),0) FROM evidence e WHERE e.task_id=t.id AND e.text IS NOT NULL) DESC,t.id DESC")?;
    let mut rows = q.query([])?;
    let (mut details, mut bytes, mut dropped) = (Vec::new(), 0, 0);
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        if details.len() as i64 >= DETAIL_TASKS {
            break;
        }
        if !allowed.iter().any(|t| t.task_id.0 == id) {
            continue;
        }
        let steps: Vec<Value> = c
            .prepare("SELECT id,description,completed_at_utc IS NOT NULL FROM plan_steps WHERE task_id=?1 AND description IS NOT NULL ORDER BY id LIMIT 16")?
            .query_map([id], |s| {
                let description: String = s.get(1)?;
                Ok(json!({"id": s.get::<_, i64>(0)?, "description": prefix(&description, 512), "completed": s.get::<_, bool>(2)?}))
            })?
            .collect::<Result<_, _>>()?;
        let notes: Vec<Value> = c
            .prepare("SELECT id,text,kind FROM evidence WHERE task_id=?1 AND text IS NOT NULL ORDER BY id DESC LIMIT 8")?
            .query_map([id], |e| {
                let text: String = e.get(1)?;
                let kind = match e.get::<_, i64>(2)? {
                    0 => "source_fact",
                    1 => "assumption",
                    2 => "user_confirmation",
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(json!({"id": e.get::<_, i64>(0)?, "text": prefix(&text, 1024), "kind": kind}))
            })?
            .collect::<Result<_, _>>()?;
        let cards: Vec<Value> = c
            .prepare(
                "SELECT message FROM assistant_proposals WHERE task_id=?1 ORDER BY id DESC LIMIT 4",
            )?
            .query_map([id], |p| {
                let message: String = p.get(0)?;
                Ok(json!({"message": prefix(&message, 512)}))
            })?
            .collect::<Result<_, _>>()?;
        let text: String = r.get(2)?;
        let completeness = match plan::decode_completeness(r.get(3)?)? {
            Completeness::Unknown => "unknown",
            Completeness::Partial => "partial",
            Completeness::Complete => "complete",
        };
        let detail = json!({"id": id, "revision": r.get::<_, i64>(1)?, "title": prefix(&text, 1024), "plan_completeness": completeness, "initiative_suppressed": r.get::<_, bool>(4)?, "steps": steps, "notes": notes, "recent_cards": cards});
        let size = detail.to_string().len() + 1;
        if bytes + size > MAX_DETAILS {
            dropped += 1;
            continue;
        }
        bytes += size;
        details.push(detail);
    }
    Ok((details, dropped))
}

/// The box of every occurrence of `anchor` within one line, overlapping ones included: the union of
/// the boxes of the words the occurrence touches. Lines are matched as their words joined by one
/// space, the same text the shell recognized.
fn occurrences(lines: &[ObservedLine], anchor: &str) -> Vec<NormalizedRect> {
    let anchor = line(anchor);
    let mut found = Vec::new();
    if anchor.is_empty() {
        return found;
    }
    for l in lines {
        let mut text = String::new();
        let mut spans = Vec::with_capacity(l.words.len());
        for w in &l.words {
            if !text.is_empty() {
                text.push(' ');
            }
            spans.push((text.len(), text.len() + w.text.len(), w.rect));
            text.push_str(&w.text);
        }
        let mut from = 0;
        while let Some(i) = text[from..].find(&anchor) {
            let (start, end) = (from + i, from + i + anchor.len());
            found.extend(
                spans
                    .iter()
                    .filter(|(a, b, _)| *a < end && start < *b)
                    .map(|(_, _, r)| *r)
                    .reduce(union),
            );
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    found
}
fn union(a: NormalizedRect, b: NormalizedRect) -> NormalizedRect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    NormalizedRect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}
fn center(r: NormalizedRect) -> (f64, f64) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}
/// The highlight after a newer frame: on its display the single occurrence near the old place, else
/// none; another display leaves the place unverified until its own display is seen again.
fn follow(h: &Highlight, shape: &FrameShape) -> Option<Highlight> {
    if h.display_key != shape.display_key {
        return Some(Highlight {
            needs_recheck: true,
            ..h.clone()
        });
    }
    let (x, y) = center(h.rect);
    let near: Vec<NormalizedRect> = occurrences(&shape.lines, &h.anchor)
        .into_iter()
        .filter(|r| {
            let (cx, cy) = center(*r);
            (cx - x).hypot(cy - y) <= NEAR
        })
        .collect();
    let [rect] = near[..] else {
        return None;
    };
    Some(Highlight {
        frame_id: shape.frame,
        rect,
        needs_recheck: false,
        ..h.clone()
    })
}

/// What one applied answer wrote, carried out of the commit transaction.
pub(super) struct Applied {
    added_cards: u32,
    rejected_items: u32,
    anchors: Vec<(ProposalId, ObservationAnchor)>,
}
/// Runs `f` inside a savepoint of the open transaction: an item-level refusal rolls back only that
/// item's writes and is reported as `None`; storage errors still abort the whole transaction.
fn item_savepoint<T>(
    c: &Connection,
    f: impl FnOnce() -> Result<T, StoreError>,
) -> Result<Option<T>, StoreError> {
    c.execute_batch("SAVEPOINT observed_item")?;
    match f() {
        Ok(value) => {
            c.execute_batch("RELEASE observed_item")?;
            Ok(Some(value))
        }
        Err(error) => {
            c.execute_batch("ROLLBACK TO observed_item; RELEASE observed_item")?;
            match error {
                StoreError::InvalidInput
                | StoreError::TextTooLong
                | StoreError::ResourceLimit
                | StoreError::NotFound
                | StoreError::Deleted
                | StoreError::StaleRevision
                | StoreError::TaskCompleted => Ok(None),
                other => Err(other),
            }
        }
    }
}
fn screen_key(quote: &str) -> SourceKey {
    SourceKey {
        namespace: SCREEN_NAMESPACE.into(),
        scope: QUOTE_SCOPE.into(),
        object_id: codex_hex(&crate::plugin_package::sha256(line(quote).as_bytes())),
    }
}
/// The checks that need no memory: quotes are exact substrings of their frame of this send, and
/// the card and its anchor are well formed.
fn check_item(r: &ObservationRequest, item: &ObservationItem) -> Result<(), StoreError> {
    if item.quotes.is_empty() {
        return Err(StoreError::InvalidInput);
    }
    let in_send = |frame: FrameId| r.frames.iter().find(|f| f.id == frame);
    for q in &item.quotes {
        text_field(&q.text)?;
        let frame = in_send(q.frame).ok_or(StoreError::InvalidInput)?;
        if q.text.trim().is_empty() || !frame.text.contains(&q.text) {
            return Err(StoreError::InvalidInput);
        }
    }
    if let ObservationTarget::New(title) = &item.task {
        text_field(title)?;
        if title.trim().is_empty() {
            return Err(StoreError::InvalidInput);
        }
    }
    if let Some(p) = &item.proposal {
        text_field(&p.message)?;
        if p.message.trim().is_empty()
            || p.rationale.trim().is_empty()
            || p.rationale.len() > 8192
            || p.quotes.iter().any(|i| *i as usize >= item.quotes.len())
        {
            return Err(StoreError::InvalidInput);
        }
        if let Some(a) = &p.highlight
            && (in_send(a.frame).is_none()
                || line(&a.anchor).is_empty()
                || a.anchor.len() > MAX_ANCHOR)
        {
            return Err(StoreError::InvalidInput);
        }
    }
    Ok(())
}
/// The per-send counts of new tasks and written evidence rows.
#[derive(Clone, Copy, Default)]
struct Spent {
    tasks: usize,
    rows: usize,
}
/// One revision fence per task, checked once before any write: the listed tasks the answer names
/// that are still at their listed revision, open and not «Не актуально». The answer's own writes
/// follow in the same write transaction, so nothing else changes these tasks meanwhile.
fn fenced(
    c: &Connection,
    r: &ObservationRequest,
    answer: &ObservationAnswer,
) -> Result<BTreeSet<i64>, StoreError> {
    let mut fresh = BTreeSet::new();
    for item in &answer.items {
        let own = match item.task {
            ObservationTarget::Existing(t) => Some(t),
            ObservationTarget::New(_) => None,
        };
        for task in own.into_iter().chain(item.related.iter().copied()) {
            let Some(listed) = r.allowed.iter().find(|t| t.task_id == task) else {
                continue;
            };
            match plan::open_fence(c, task, listed.revision) {
                Ok(t) if !t.initiative_suppressed => {
                    fresh.insert(task.0);
                }
                Ok(_)
                | Err(
                    StoreError::StaleRevision
                    | StoreError::Deleted
                    | StoreError::NotFound
                    | StoreError::TaskCompleted,
                ) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(fresh)
}
/// Writes one item's task, evidence and links and returns the counts spent with them; any refusal
/// rejects the whole item.
fn apply_item(
    c: &Connection,
    r: &ObservationRequest,
    item: &ObservationItem,
    fresh: &BTreeSet<i64>,
    mut spent: Spent,
    at_utc: i64,
) -> Result<(TaskId, Spent), StoreError> {
    check_item(r, item)?;
    let allowed = |task: TaskId| {
        if fresh.contains(&task.0) {
            Ok(())
        } else {
            Err(StoreError::InvalidInput)
        }
    };
    let mut related = Vec::new();
    for task in &item.related {
        if item.task == ObservationTarget::Existing(*task) || related.contains(task) {
            return Err(StoreError::InvalidInput);
        }
        allowed(*task)?;
        related.push(*task);
    }
    let mut owner = match &item.task {
        ObservationTarget::Existing(task) => {
            allowed(*task)?;
            TaskVersion {
                task_id: *task,
                revision: read_task(c, *task)?.revision,
            }
        }
        ObservationTarget::New(title) => {
            if spent.tasks >= MAX_NEW_TASKS {
                return Err(StoreError::ResourceLimit);
            }
            spent.tasks += 1;
            let t = insert_task(c, title)?;
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            }
        }
    };
    let mut grounded = false;
    for q in &item.quotes {
        let key = screen_key(&q.text);
        // A quote already stored verbatim is not rewritten, so seeing it again keeps the task's
        // revision and its cards fresh.
        let stored: Option<String> = c
            .query_row(
                "SELECT e.text FROM source_bindings b JOIN evidence e ON e.id=b.evidence_id WHERE b.task_id=?1 AND b.digest=?2 AND b.namespace IS NOT NULL",
                params![owner.task_id.0, key.digest()?.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        if stored.as_deref() == Some(q.text.as_str()) {
            grounded = true;
            continue;
        }
        if spent.rows >= MAX_EVIDENCE_ROWS {
            return Err(StoreError::ResourceLimit);
        }
        let at = r.frames.iter().find(|f| f.id == q.frame).map(|f| f.at_utc);
        let evidence = SourceEvidence {
            text: q.text.clone(),
            observed_at_utc: at,
            completeness: Completeness::Partial,
            kind: AssertionKind::Assumption,
        };
        match write_source(c, owner, &key, None, &evidence, None)? {
            IngestOutcome::Applied(_) => {
                spent.rows += 1;
                owner.revision = read_task(c, owner.task_id)?.revision;
                grounded = true;
            }
            // The user's own version stays and still grounds the item.
            IngestOutcome::UserProtected => grounded = true,
            // Forgotten after a deletion (D41): this quote cannot bring the content back.
            IngestOutcome::Forgotten => {}
        }
    }
    if !grounded {
        return Err(StoreError::Deleted);
    }
    for other in related {
        add_link(c, owner.task_id, other, at_utc)?;
    }
    Ok((owner.task_id, spent))
}
/// Inserts an accepted link when the pair has no decision yet; a user's rejection (D42) or any
/// earlier decision stays as it is.
fn add_link(c: &Connection, a: TaskId, b: TaskId, at_utc: i64) -> Result<bool, StoreError> {
    let (left, right) = if a.0 < b.0 { (a, b) } else { (b, a) };
    let decided: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM decisions WHERE task_id=?1 AND other_task_id=?2)",
        params![left.0, right.0],
        |r| r.get(0),
    )?;
    if decided {
        return Ok(false);
    }
    reserve(c, "decisions", 1)?;
    c.execute(
        "INSERT INTO decisions(task_id,other_task_id,accepted,action_at_utc) VALUES(?1,?2,1,?3)",
        params![left.0, right.0, at_utc],
    )?;
    bump(c, left)?;
    bump(c, right)?;
    Ok(true)
}
/// Applies a current answer inside the transaction that settles its send. The set of writes is
/// closed: tasks, assumption evidence, links and cards in Ambient's own memory. Items are applied
/// first; cards follow once every evidence write of the send is done, so their workspace basis is
/// the tasks' final revision.
fn apply_answer(
    c: &Connection,
    attempt: AttemptId,
    r: &ObservationRequest,
    answer: &ObservationAnswer,
    at_utc: i64,
) -> Result<Applied, StoreError> {
    let fresh = fenced(c, r, answer)?;
    let mut spent = Spent::default();
    let mut applied = Applied {
        added_cards: 0,
        rejected_items: 0,
        anchors: Vec::new(),
    };
    let mut cards = Vec::new();
    for item in &answer.items {
        match item_savepoint(c, || apply_item(c, r, item, &fresh, spent, at_utc))? {
            Some((task, left)) => {
                spent = left;
                cards.extend(item.proposal.as_ref().map(|p| (task, p)));
            }
            None => applied.rejected_items += 1,
        }
    }
    for (task, p) in cards {
        match item_savepoint(c, || {
            proposals::insert_observed(c, task, &p.message, &p.rationale)
        })? {
            Some(id) => {
                applied.added_cards += 1;
                applied.anchors.extend(p.highlight.clone().map(|a| (id, a)));
            }
            None => applied.rejected_items += 1,
        }
    }
    c.execute(
        "UPDATE observation_sends SET added_cards=?1,rejected_items=?2 WHERE attempt_id=?3",
        params![applied.added_cards, applied.rejected_items, attempt.0],
    )?;
    Ok(applied)
}
