use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool},
};
pub const MAX_ASSISTANCE_CONTEXT: usize = 256 * 1024;
pub const MAX_ASSISTANCE_BODY: usize = 320 * 1024;
pub const MAX_REPLY_TEXT: usize = 16 * 1024;
pub const MAX_REPLY_PAIRS: usize = 32;
pub const MAX_ASSISTANCE_TEXT: usize = 32 * 1024;
pub const MAX_PROPOSALS_PER_TASK: usize = 16;
pub const MAX_PROPOSALS: usize = 128;
pub const MAX_PROPOSAL_ROW: usize = 32 * 1024;
pub const MAX_PROPOSAL_FEED: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProposalId(pub(crate) i64);
impl ProposalId {
    pub fn from_raw(id: u64) -> Result<Self, StoreError> {
        i64::try_from(id)
            .ok()
            .filter(|v| *v > 0)
            .map(Self)
            .ok_or(StoreError::InvalidInput)
    }
    pub fn get(self) -> u64 {
        self.0 as u64
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalTarget {
    pub id: ProposalId,
    pub content_revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceVersion {
    pub task: TaskVersion,
    pub workspace_revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistanceMode {
    Continue,
    NewContext,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalReply {
    pub target: ProposalTarget,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistanceRequest {
    pub task: TaskVersion,
    pub workspace_revision: u64,
    pub mode: AssistanceMode,
    pub selected_evidence: Vec<EvidenceId>,
    pub reply: Option<ProposalReply>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistancePreview {
    pub request_id: u64,
    pub task: TaskVersion,
    pub workspace_revision: u64,
    pub mode: AssistanceMode,
    pub context: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum AssistanceKind {
    Suggestion = 1,
    Clarification = 2,
}
/// Untrusted full provider set; only Store validates identity, grounds and readiness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistanceAnswer {
    pub proposals: Vec<ProposalAnswer>,
    pub rationale: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalAnswer {
    pub proposal_id: Option<ProposalId>,
    pub kind: AssistanceKind,
    pub message: String,
    pub rationale: String,
    pub evidence_ids: Vec<String>,
    pub step_id: Option<StepId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProposalCitationRef {
    Task(TaskId),
    Note(EvidenceId),
    Source(EvidenceId),
    Step(StepId),
    Check(CheckId),
    Reply(String),
}
impl ProposalCitationRef {
    pub fn key(&self) -> String {
        match self {
            Self::Task(id) => format!("task:{}", id.0),
            Self::Note(id) => format!("note:{}", id.0),
            Self::Source(id) => format!("source:{}", id.0),
            Self::Step(id) => format!("step:{}", id.0),
            Self::Check(id) => format!("check:{}", id.0),
            Self::Reply(id) => id.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssistanceCitation {
    pub reference: ProposalCitationRef,
    pub text: String,
    pub model_question: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<CitationIntegration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<CitationPlugin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_observed_at_utc: Option<i64>,
}
/// Provenance of a saved plugin tool result shown with its citation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CitationPlugin {
    pub plugin: String,
    pub server: String,
    pub tool: String,
    pub called_at_utc: i64,
    pub complete: bool,
}
/// A model citation resolved against the snapshot or this request's tool results; never stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedGround {
    Stored(ProposalCitationRef),
    Tool(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: ProposalId,
    pub task: TaskVersion,
    pub workspace_revision: u64,
    pub content_revision: u64,
    pub kind: AssistanceKind,
    pub message: String,
    pub rationale: String,
    pub citations: Vec<ProposalCitationRef>,
    pub step_id: Option<StepId>,
    pub visible: bool,
    pub stale: bool,
}
impl Proposal {
    pub fn target(&self) -> ProposalTarget {
        ProposalTarget {
            id: self.id,
            content_revision: self.content_revision,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalFeed {
    pub proposals: Vec<Proposal>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalDetails {
    pub proposal: Proposal,
    pub citations: Vec<AssistanceCitation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DialoguePair {
    pub id: String,
    pub proposal: ProposalTarget,
    pub model_question: String,
    pub user_reply: String,
}
#[derive(Clone)]
pub(crate) struct AssistanceSnapshot {
    pub cancelled: Arc<AtomicBool>,
    pub preview: AssistancePreview,
    pub epoch: u64,
    pub task: Task,
    pub steps: Vec<PlanStep>,
    pub citations: BTreeMap<String, AssistanceCitation>,
    pub selected: Vec<EvidenceId>,
    pub sources: Vec<SourceSettingsFence>,
    pub proposals: Vec<Proposal>,
    pub replies: Vec<DialoguePair>,
    /// Fixed by prepare only; empty for details and open-time checks.
    pub plugins: PluginOffer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PluginVersion {
    pub id: u64,
    pub revision: u64,
}
/// An approved tool offered to the model, built from its pin at prepare and not from the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PinnedTool {
    pub plugin: u64,
    pub plugin_name: String,
    pub plugin_version: String,
    pub server: String,
    pub kind: ServerKind,
    pub name: String,
    pub model_name: String,
    pub definition: String,
    pub hash: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PinnedSkill {
    pub plugin: String,
    pub skill: PluginSkill,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PluginOffer {
    pub versions: Vec<PluginVersion>,
    pub tools: Vec<PinnedTool>,
    pub skills: Vec<PinnedSkill>,
    /// Results that may still become cited grounds: 32 minus the selected evidence.
    pub citable: usize,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct ModelSession {
    pub epoch: u64,
    pub valid: bool,
    pub reported: bool,
}
pub(crate) struct ActiveModel {
    pub cancelled: Arc<AtomicBool>,
    pub purpose: ModelPurpose,
    pub profile: ModelProfile,
    pub epoch: u64,
    pub request_id: Option<u64>,
    pub attempt: Option<AttemptId>,
    /// The request an observation admission was built with at begin.
    pub observation: Option<Arc<crate::ObservationRequest>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum SourceSettingsFence {
    Git { path: String, revision: u64 },
    Codex(CodexMetadata),
    Integration(Box<IntegrationSavedMetadata>),
    Plugin(Box<PluginSavedMetadata>),
}
/// Saved plugin result metadata in the shared binding-metadata table (namespace `plugin`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginResultMetadata {
    pub plugin_id: u64,
    pub plugin_version: String,
    pub plugin_name: String,
    pub server: String,
    pub tool: String,
    pub arguments: String,
    pub called_at_utc: i64,
    pub content_hash: String,
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginSavedMetadata {
    pub binding_id: i64,
    pub evidence_id: EvidenceId,
    pub metadata: PluginResultMetadata,
}
