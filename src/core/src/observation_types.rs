use crate::{CheckAdmission, ModelCheckResult, ProposalId, StoreError, TaskId, TaskVersion};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Text of one frame after normalization; longer screens are cut and the cut is a loss.
pub const MAX_FRAME_TEXT: usize = 32 * 1024;
/// One downscaled JPEG; the native shell encodes it to fit.
pub const MAX_FRAME_IMAGE: usize = 768 * 1024;
/// Serialized unsent frames of one request, newest first, text and metadata (ADR-025 «Запрос»).
pub const MAX_REQUEST_FRAMES: usize = 64 * 1024;
/// The whole serialized context; frames, the task list and task details are bounded below it.
pub const MAX_OBSERVATION_CONTEXT: usize = 256 * 1024;
pub const DEFAULT_PER_HOUR_LIMIT: u32 = 20;
/// Source namespace of verbatim screen quotes. Only the observation answer writes it; the public
/// source paths reject it.
pub const SCREEN_NAMESPACE: &str = "screen";

/// Both clocks at one instant: wall time is persisted history (frames, attempt rows, the rolling
/// hour), monotonic time drives in-session deadlines (settling, backoff).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservationTime {
    pub utc: i64,
    pub monotonic_ms: u64,
}

/// Boundary JSON of `set_policy`. Starting requires the credit acknowledgement, which is kept
/// until stop (D103).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationPolicy {
    pub started: bool,
    pub per_hour_limit: u32,
    pub credit_acknowledged: bool,
}

/// Vision rectangle normalized to the display, origin bottom-left.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedWord {
    pub text: String,
    pub rect: NormalizedRect,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedLine {
    pub text: String,
    pub words: Vec<ObservedWord>,
}
/// One capture of the display under the front window. The frame time is the submit time.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedFrame {
    pub display_key: String,
    pub bundle_id: String,
    pub app_name: String,
    pub title: String,
    pub idle_seconds: u32,
    pub lines: Vec<ObservedLine>,
    pub image: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameId(pub(crate) i64);
impl FrameId {
    pub fn from_raw(id: u64) -> Result<Self, StoreError> {
        i64::try_from(id)
            .ok()
            .filter(|id| *id > 0)
            .map(Self)
            .ok_or(StoreError::InvalidInput)
    }
    pub fn get(self) -> u64 {
        self.0 as u64
    }
}

/// When the native shell acts next. `Hold` means "do not capture"; it keeps calling `tick`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    After { seconds: u32 },
    Hold,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FrameDisposition {
    Recorded = 1,
    Duplicate = 2,
    NotStarted = 3,
    Paused = 4,
    CloudUnavailable = 5,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameOutcome {
    pub disposition: FrameDisposition,
    /// The recorded row; `None` for duplicates and dropped frames.
    pub frame: Option<FrameId>,
    pub next: Capture,
    pub highlight_version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ObservationBlock {
    NotStarted = 1,
    Paused = 2,
    CloudUnavailable = 3,
    NotDue = 4,
    HourlyLimit = 5,
    Busy = 6,
}
/// `Ready` holds the shared single attempt slot; it is served like any other admission.
pub enum ObservationBegin {
    Ready(CheckAdmission),
    Blocked(ObservationBlock),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationImage {
    pub frame: FrameId,
    pub jpeg: Arc<[u8]>,
}
/// The immutable request built at begin and shared by the attempt slot and its dispatch.
#[derive(Debug, PartialEq, Eq)]
pub struct ObservationRequest {
    pub(crate) context: String,
    pub(crate) images: Vec<ObservationImage>,
    /// Frames of this send with their exact text, newest first, for quote checks.
    pub(crate) frames: Vec<SentFrame>,
    /// Open tasks the answer may write to, each with its revision fence.
    pub(crate) allowed: Vec<TaskVersion>,
    pub(crate) last_frame: FrameId,
    pub(crate) apps: Vec<String>,
    /// What is missing from this send, as reported in its context.
    pub(crate) dropped: Losses,
    /// The part of `dropped` carried over from earlier pruning, cuts and evictions.
    pub(crate) pending: Losses,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SentFrame {
    pub id: FrameId,
    pub at_utc: i64,
    pub text: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Losses {
    pub frames: u64,
    pub bytes: u64,
    pub images: u64,
}
impl ObservationRequest {
    pub fn context(&self) -> &str {
        &self.context
    }
    pub fn images(&self) -> &[ObservationImage] {
        &self.images
    }
}

/// The model's answer. The provider maps required-nullable wire fields into these types; the
/// core validates and applies them (27C).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationAnswer {
    pub items: Vec<ObservationItem>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservationTarget {
    Existing(TaskId),
    New(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationItem {
    pub task: ObservationTarget,
    pub related: Vec<TaskId>,
    pub quotes: Vec<ObservationQuote>,
    pub proposal: Option<ObservationProposal>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationQuote {
    pub frame: FrameId,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationProposal {
    pub message: String,
    pub rationale: String,
    /// Indexes into the item's own `quotes`.
    pub quotes: Vec<u32>,
    pub highlight: Option<ObservationAnchor>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationAnchor {
    pub frame: FrameId,
    pub anchor: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservationFinish {
    pub result: ModelCheckResult,
    /// Whether the answer reached the apply step; false for failures and late answers.
    pub applied: bool,
    pub added_cards: u32,
    pub rejected_items: u32,
    pub highlight_version: u64,
}

/// The place of one card on the screen, in memory only. It is anchored in a frame of the ring and
/// follows newer frames of the same display; another display needs a recheck.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Highlight {
    pub proposal_id: ProposalId,
    pub frame_id: FrameId,
    pub display_key: String,
    pub anchor: String,
    pub rect: NormalizedRect,
    pub needs_recheck: bool,
}
/// Every change, including removal, raises `version`; the shell reads this only when the version
/// in a frame or finish result changed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HighlightState {
    pub version: u64,
    pub highlight: Option<Highlight>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservationSend {
    pub attempt: u64,
    pub at_utc: i64,
    pub first_frame: FrameId,
    pub last_frame: FrameId,
    pub apps: Vec<String>,
    pub images: u32,
    pub dropped_frames: u64,
    pub dropped_bytes: u64,
    pub dropped_images: u64,
    /// `CheckOutcome` as its number; 0 while the attempt is in flight.
    pub outcome: u32,
    /// `ActualTier` as its number, the served tier label.
    pub tier: u32,
    pub added_cards: u32,
    pub rejected_items: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservationJournal {
    pub policy: ObservationPolicy,
    pub log_frames: u64,
    pub unsent_frames: u64,
    /// Observation attempts in the rolling hour, against `policy.per_hour_limit`.
    pub hour_attempts: u64,
    /// Newest first, at most one day.
    pub sends: Vec<ObservationSend>,
}
