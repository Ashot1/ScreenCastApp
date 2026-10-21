use sha2::{Digest, Sha256};

pub const MAX_TEXT_BYTES: usize = 16_384;
pub const MAX_PAGE_ITEMS: usize = 50;
pub const MAX_PAGE_BYTES: usize = 256 * 1024;
pub const MAX_RETAINED_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_ROWS: i64 = 100_000;
pub const MAX_LIVE_TASKS: i64 = 1_024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskVersion {
    pub task_id: TaskId,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub revision: u64,
    pub text: String,
    pub lifecycle: TaskLifecycle,
    pub initiative_suppressed: bool,
    pub plan_completeness: Completeness,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TaskLifecycle {
    Open = 0,
    Completed = 1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StepId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependencyId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceId(pub i64);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInput {
    pub description: String,
    pub prompt: Option<String>,
    pub reference: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    pub id: StepId,
    pub task_id: TaskId,
    pub description: String,
    pub prompt: Option<String>,
    pub reference: Option<String>,
    /// Explicit user confirmation; readiness is computed on each read, never persisted.
    pub completed_at_utc: Option<i64>,
    pub ready: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CheckResult {
    Unknown = 0,
    Passed = 1,
    Failed = 2,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanCheck {
    pub id: CheckId,
    pub task_id: TaskId,
    pub step_id: StepId,
    pub description: String,
    pub result: CheckResult,
    /// Only explicit user confirmation supplies a timestamp; unknown has none.
    pub confirmed_at_utc: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDependency {
    pub id: DependencyId,
    pub task_id: TaskId,
    pub predecessor: StepId,
    pub dependent: StepId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsefulReference {
    pub id: ReferenceId,
    pub task_id: TaskId,
    pub text: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Completeness {
    Unknown = 0,
    Partial = 1,
    Complete = 2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AssertionKind {
    SourceFact = 0,
    Assumption = 1,
    UserConfirmation = 2,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceKey {
    pub namespace: String,
    pub scope: String,
    pub object_id: String,
}
impl SourceKey {
    pub fn validate(&self) -> Result<(), StoreError> {
        key_field(&self.namespace, 64)?;
        key_field(&self.scope, 256)?;
        key_field(&self.object_id, 2048)
    }
    pub fn digest(&self) -> Result<[u8; 32], StoreError> {
        self.validate()?;
        let mut hash = Sha256::new();
        // Versioned length prefixes preserve exact tuple boundaries; content/version are excluded.
        hash.update([1]);
        for part in [&self.namespace, &self.scope, &self.object_id] {
            hash.update((part.len() as u32).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Ok(hash.finalize().into())
    }
    pub fn bytes(&self) -> usize {
        self.namespace.len() + self.scope.len() + self.object_id.len()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    User,
    External {
        key: SourceKey,
        version: Option<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEvidence {
    pub text: String,
    pub observed_at_utc: Option<i64>,
    pub completeness: Completeness,
    pub kind: AssertionKind,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub id: EvidenceId,
    pub task_id: TaskId,
    pub text: String,
    pub origin: Origin,
    pub observed_at_utc: Option<i64>,
    pub completeness: Completeness,
    pub kind: AssertionKind,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBinding {
    pub id: i64,
    pub task_id: TaskId,
    pub evidence_id: EvidenceId,
    pub key: SourceKey,
    pub version: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLink {
    pub id: i64,
    pub other_task_id: TaskId,
    pub related: bool,
    pub action_at_utc: i64,
    pub other_task_revision: u64,
    pub other_task_text: String,
    pub other_task_lifecycle: TaskLifecycle,
    pub other_task_initiative_suppressed: bool,
    pub other_task_plan_completeness: Completeness,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingDecision {
    pub id: i64,
    pub evidence_id: EvidenceId,
    pub accepted: bool,
    pub action_at_utc: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_after: Option<i64>,
    pub payload_bytes: usize,
    pub task_revision: Option<u64>,
    pub plan_completeness: Option<Completeness>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    Applied(EvidenceId),
    Forgotten,
    UserProtected,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Paused,
    Busy,
    Cancelled,
    ReportRequired,
    CapabilityRequired,
    SessionRequired,
    InvalidInput,
    TextTooLong,
    Storage,
    UnsupportedSchema,
    ResourceLimit,
    NotFound,
    Deleted,
    StaleRevision,
    TaskCompleted,
}
impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Paused => "connection check is paused",
            Self::Busy => "connection check is busy",
            Self::Cancelled => "connection check was cancelled",
            Self::CapabilityRequired => "current High connection check is required",
            Self::SessionRequired => "current model session is required",
            Self::ReportRequired => "credit setting report is required",
            Self::InvalidInput => "invalid memory input",
            Self::TextTooLong => "memory input exceeds byte limit",
            Self::Storage => "memory storage failed",
            Self::UnsupportedSchema => "memory schema is unsupported or damaged",
            Self::ResourceLimit => "memory resource limit reached",
            Self::NotFound => "memory identity not found",
            Self::Deleted => "memory identity was deleted",
            Self::StaleRevision => "memory revision changed",
            Self::TaskCompleted => "memory task is completed",
        })
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::SqliteFailure(code, _)
                if matches!(
                    code.code,
                    rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
                ) =>
            {
                Self::UnsupportedSchema
            }
            _ => Self::Storage,
        }
    }
}
pub(crate) fn text_field(text: &str) -> Result<(), StoreError> {
    if text.is_empty() {
        Err(StoreError::InvalidInput)
    } else if text.len() > MAX_TEXT_BYTES {
        Err(StoreError::TextTooLong)
    } else {
        Ok(())
    }
}
pub(crate) fn key_field(text: &str, limit: usize) -> Result<(), StoreError> {
    if text.is_empty() || text.contains('\0') {
        Err(StoreError::InvalidInput)
    } else if text.len() > limit {
        Err(StoreError::TextTooLong)
    } else {
        Ok(())
    }
}
