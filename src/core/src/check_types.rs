use crate::StoreError;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttemptId(pub(crate) i64);
impl AttemptId {
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
// Neither admission nor dispatch can be cloned or constructed outside Core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ModelPurpose {
    Connectivity = 1,
    Assistance = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ModelProfile {
    Low = 1,
    High = 2,
}
pub struct CheckAdmission {
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) purpose: ModelPurpose,
    pub(crate) profile: ModelProfile,
    pub(crate) epoch: u64,
    pub(crate) request_id: Option<u64>,
}
impl CheckAdmission {
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    pub fn purpose(&self) -> ModelPurpose {
        self.purpose
    }
}
pub struct CheckDispatch {
    pub(crate) profile: ModelProfile,
    pub(crate) snapshot: Option<Arc<crate::assistance_types::AssistanceSnapshot>>,
    pub(crate) id: AttemptId,
    pub(crate) cancelled: Arc<AtomicBool>,
}
impl CheckDispatch {
    pub fn profile(&self) -> ModelProfile {
        self.profile
    }
    pub fn purpose(&self) -> ModelPurpose {
        if self.snapshot.is_some() {
            ModelPurpose::Assistance
        } else {
            ModelPurpose::Connectivity
        }
    }
    pub fn context(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|s| s.preview.context.as_str())
    }
    pub fn id(&self) -> AttemptId {
        self.id
    }
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum CheckOutcome {
    #[default]
    Pending = 0,
    Confirmed = 1,
    Mismatch = 2,
    Provider = 3,
    Invalid = 4,
    Cancelled = 5,
    Timeout = 6,
    Interrupted = 7,
    Local = 8,
    Quota = 9,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ActualModel {
    #[default]
    Missing = 0,
    Luna = 1,
    Other = 2,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ActualEffort {
    #[default]
    Missing = 0,
    Low = 1,
    Other = 2,
    High = 3,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ActualTier {
    #[default]
    Missing = 0,
    Priority = 1,
    Fast = 2,
    Default = 3,
    Other = 4,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActualConfig {
    pub model: ActualModel,
    pub effort: ActualEffort,
    pub tier: ActualTier,
}
impl ActualConfig {
    /// The tier is shown, never gated (D91, D96): requests still ask for priority, and the served
    /// tier, even a missing one, is only recorded as an honest label.
    pub fn confirmed(self, profile: ModelProfile) -> bool {
        self.model == ActualModel::Luna
            && self.effort
                == match profile {
                    ModelProfile::Low => ActualEffort::Low,
                    ModelProfile::High => ActualEffort::High,
                }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckUsage {
    input: u64,
    output: u64,
    total: u64,
}
impl CheckUsage {
    pub fn new(input: u64, output: u64, total: u64) -> Result<Self, StoreError> {
        if input.checked_add(output) != Some(total) || total > i64::MAX as u64 {
            return Err(StoreError::InvalidInput);
        }
        Ok(Self {
            input,
            output,
            total,
        })
    }
    pub fn input(self) -> u64 {
        self.input
    }
    pub fn output(self) -> u64 {
        self.output
    }
    pub fn total(self) -> u64 {
        self.total
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelCheckResult {
    pub outcome: CheckOutcome,
    pub actual: ActualConfig,
    pub usage: Option<CheckUsage>,
}
impl ModelCheckResult {
    pub fn unknown(outcome: CheckOutcome) -> Self {
        Self {
            outcome,
            ..Self::default()
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckAttempt {
    pub purpose: ModelPurpose,
    pub profile: ModelProfile,
    pub id: AttemptId,
    pub at_utc: i64,
    pub stage: u32,
    pub result: ModelCheckResult,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckSummary {
    pub attempts: u64,
    pub input: u64,
    pub output: u64,
    pub total: u64,
    pub unknown: u64,
    pub latest: Option<CheckAttempt>,
}

/// One function call requested by the model; arguments were parsed from a JSON object string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Map<String, serde_json::Value>,
}
/// Calls win over text: a response with calls never carries an answer. `forwarded` holds the
/// reasoning and call items returned unchanged in the next POST of the same request.
#[derive(Debug)]
pub struct ModelResponse {
    pub check: ModelCheckResult,
    pub answer: Option<crate::AssistanceAnswer>,
    pub calls: Vec<ToolCall>,
    pub forwarded: Vec<serde_json::Value>,
}
impl ModelResponse {
    pub fn unknown(outcome: CheckOutcome) -> Self {
        Self {
            check: ModelCheckResult::unknown(outcome),
            answer: None,
            calls: Vec::new(),
            forwarded: Vec::new(),
        }
    }
}
