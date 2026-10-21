use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};

pub const MAX_INTEGRATION_BODY: usize = 1024 * 1024;
pub const MAX_INTEGRATION_WINDOW: usize = 512 * 1024;

/// Built-in HTTP integrations owned by the shared engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationKind {
    GitHub,
    Jira,
    Todoist,
}
impl IntegrationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Jira => "jira",
            Self::Todoist => "todoist",
        }
    }
    pub(crate) fn parse(raw: &str) -> Result<Self, StoreError> {
        match raw {
            "github" => Ok(Self::GitHub),
            "jira" => Ok(Self::Jira),
            "todoist" => Ok(Self::Todoist),
            _ => Err(StoreError::UnsupportedSchema),
        }
    }
    /// Saved integration evidence is read back only through its binding namespace.
    pub fn from_namespace(namespace: &str) -> Option<Self> {
        ADAPTERS
            .into_iter()
            .find(|a| a.namespace() == namespace)
            .map(|a| a.kind())
    }
}
/// Built-in integrations that the user switches on and off as a whole (D61).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuiltInIntegration {
    LocalProjects,
    Codex,
    GitHub,
    Jira,
    Todoist,
}
impl BuiltInIntegration {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalProjects => "local-projects",
            Self::Codex => "codex",
            Self::GitHub => "github",
            Self::Jira => "jira",
            Self::Todoist => "todoist",
        }
    }
}
impl From<IntegrationKind> for BuiltInIntegration {
    fn from(kind: IntegrationKind) -> Self {
        match kind {
            IntegrationKind::GitHub => Self::GitHub,
            IntegrationKind::Jira => Self::Jira,
            IntegrationKind::Todoist => Self::Todoist,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegrationProfileVersion {
    pub id: u64,
    pub settings_revision: u64,
    pub credential_generation: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraConfig {}
/// Adapter settings nested as JSON; stored rows are read back only by their kind.
/// GitHub settings are the repository allowlist array itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum IntegrationConfig {
    GitHub(Vec<GitHubRepository>),
    Jira(JiraConfig),
    Todoist(TodoistConfig),
}
impl IntegrationConfig {
    pub fn kind(&self) -> IntegrationKind {
        match self {
            Self::GitHub(_) => IntegrationKind::GitHub,
            Self::Jira(_) => IntegrationKind::Jira,
            Self::Todoist(_) => IntegrationKind::Todoist,
        }
    }
    pub(crate) fn decode(kind: IntegrationKind, raw: &str) -> Result<Self, StoreError> {
        match kind {
            IntegrationKind::GitHub => serde_json::from_str(raw).map(Self::GitHub),
            IntegrationKind::Jira => serde_json::from_str(raw).map(Self::Jira),
            IntegrationKind::Todoist => serde_json::from_str(raw).map(Self::Todoist),
        }
        .map_err(|_| StoreError::UnsupportedSchema)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IntegrationProfile {
    pub id: u64,
    pub kind: IntegrationKind,
    pub settings_revision: u64,
    pub credential_generation: u64,
    pub name: String,
    pub scope: Option<String>,
    pub config: IntegrationConfig,
    pub not_before_utc: Option<i64>,
    pub server_deadline_known: bool,
}
impl IntegrationProfile {
    pub fn version(&self) -> IntegrationProfileVersion {
        IntegrationProfileVersion {
            id: self.id,
            settings_revision: self.settings_revision,
            credential_generation: self.credential_generation,
        }
    }
}
/// A permitted request; providers build the address from the variant's registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrationRequest {
    GitHub(GitHubRequest),
    Jira(JiraRequest),
    Todoist(TodoistRequest),
}
#[derive(Clone, Debug)]
pub struct IntegrationStep {
    pub request: IntegrationRequest,
    pub remaining: Duration,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntegrationHeaders {
    pub link: Option<String>,
    pub retry_after: Option<String>,
    pub remaining: Option<String>,
    pub reset: Option<String>,
    pub link_ambiguous: bool,
    pub rate_ambiguous: bool,
}
impl IntegrationHeaders {
    /// `Retry-After` as delay seconds or an HTTP date; its age is not judged here. None when absent
    /// or unreadable; callers that must tell those apart look at `retry_after` itself.
    pub fn retry_after_deadline(&self, now: i64) -> Option<i64> {
        let raw = self.retry_after.as_deref()?;
        let delta = if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
            raw.parse::<i64>().ok().and_then(|n| now.checked_add(n))
        } else {
            None
        };
        let date = httpdate::parse_http_date(raw)
            .ok()
            .and_then(|d| d.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_secs()).ok());
        delta.or(date)
    }
}
#[derive(Clone, Debug)]
pub struct IntegrationResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: IntegrationHeaders,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum IntegrationCoverage {
    GitHub(Vec<GitHubCoverage>),
    Jira(Vec<JiraCoverage>),
    Todoist(Vec<TodoistCoverage>),
}
/// Untagged so each adapter's stored JSON stays exactly its previous shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum IntegrationMetadata {
    GitHub(GitHubMetadata),
    Jira(JiraMetadata),
    Todoist(TodoistMetadata),
}
impl IntegrationMetadata {
    pub fn kind(&self) -> IntegrationKind {
        match self {
            Self::GitHub(_) => IntegrationKind::GitHub,
            Self::Jira(_) => IntegrationKind::Jira,
            Self::Todoist(_) => IntegrationKind::Todoist,
        }
    }
    pub(crate) fn decode(kind: IntegrationKind, raw: &str) -> Result<Self, StoreError> {
        match kind {
            IntegrationKind::GitHub => serde_json::from_str(raw).map(Self::GitHub),
            IntegrationKind::Jira => serde_json::from_str(raw).map(Self::Jira),
            IntegrationKind::Todoist => serde_json::from_str(raw).map(Self::Todoist),
        }
        .map_err(|_| StoreError::UnsupportedSchema)
    }
    fn from_value(kind: IntegrationKind, raw: serde_json::Value) -> Result<Self, StoreError> {
        match kind {
            IntegrationKind::GitHub => serde_json::from_value(raw).map(Self::GitHub),
            IntegrationKind::Jira => serde_json::from_value(raw).map(Self::Jira),
            IntegrationKind::Todoist => serde_json::from_value(raw).map(Self::Todoist),
        }
        .map_err(|_| StoreError::UnsupportedSchema)
    }
    pub(crate) fn profile(&self) -> IntegrationProfileVersion {
        let (id, settings_revision, credential_generation) = match self {
            Self::GitHub(m) => (m.profile_id, m.settings_revision, m.credential_generation),
            Self::Jira(m) => (m.profile_id, m.settings_revision, m.credential_generation),
            Self::Todoist(m) => (m.profile_id, m.settings_revision, m.credential_generation),
        };
        IntegrationProfileVersion {
            id,
            settings_revision,
            credential_generation,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IntegrationObject {
    pub key: SourceKey,
    pub version: String,
    pub text: String,
    pub observed_at_utc: Option<i64>,
    pub metadata: IntegrationMetadata,
    pub bindable: bool,
}
/// Bytes counted against the 16 KiB per-object limit; untagged metadata keeps each adapter's count.
pub(crate) fn combined_bytes(o: &IntegrationObject) -> Result<usize, StoreError> {
    Ok(o.text.len()
        + o.key.bytes()
        + o.version.len()
        + serde_json::to_vec(&o.metadata)
            .map_err(|_| StoreError::InvalidInput)?
            .len())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IntegrationBindResult {
    pub task: TaskVersion,
    pub evidence_ids: Vec<EvidenceId>,
}
/// A saved binding's metadata. The persisted 08B ground names the kind and is decoded by it;
/// native saved-metadata pages are built from the fields without `kind`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedJson", into = "SavedOut")]
pub struct IntegrationSavedMetadata {
    pub binding_id: i64,
    pub evidence_id: EvidenceId,
    pub metadata: IntegrationMetadata,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedJson {
    kind: IntegrationKind,
    binding_id: i64,
    evidence_id: EvidenceId,
    metadata: serde_json::Value,
}
#[derive(Serialize)]
struct SavedOut {
    kind: IntegrationKind,
    binding_id: i64,
    evidence_id: EvidenceId,
    metadata: IntegrationMetadata,
}
impl TryFrom<SavedJson> for IntegrationSavedMetadata {
    type Error = StoreError;
    fn try_from(v: SavedJson) -> Result<Self, StoreError> {
        Ok(Self {
            binding_id: v.binding_id,
            evidence_id: v.evidence_id,
            metadata: IntegrationMetadata::from_value(v.kind, v.metadata)?,
        })
    }
}
impl From<IntegrationSavedMetadata> for SavedOut {
    fn from(v: IntegrationSavedMetadata) -> Self {
        Self {
            kind: v.metadata.kind(),
            binding_id: v.binding_id,
            evidence_id: v.evidence_id,
            metadata: v.metadata,
        }
    }
}
/// Citation detail for native presentation; native decodes `metadata` by `kind`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CitationIntegration {
    pub kind: IntegrationKind,
    pub metadata: IntegrationMetadata,
}

#[derive(Clone)]
pub(crate) enum IntegrationTarget {
    GitHub(GitHubTarget),
    Jira(JiraTarget),
    Todoist(TodoistTarget),
}
pub(crate) struct Pause {
    pub until: Option<i64>,
    pub known: bool,
    pub failure: u32,
}
pub(crate) struct Started {
    pub reader: Box<dyn Reader>,
    pub coverage: IntegrationCoverage,
}
pub(crate) enum Step {
    Continue,
    Done,
    Fail(u32),
    /// Continue, but first write this scope into the profile once (D65: the Todoist account).
    Pin(String),
}
#[derive(Serialize)]
pub(crate) struct IntegrationWindow {
    pub observation_id: u64,
    pub interval_start: i64,
    pub interval_end: i64,
    pub categories: IntegrationCoverage,
    pub objects: Vec<IntegrationObject>,
}
/// The built-in adapters; namespaces resolve to kinds through this table.
pub(crate) const ADAPTERS: [&dyn Adapter; 3] = [&crate::GitHub, &crate::Jira, &crate::Todoist];
/// Service-specific behavior; the engine owns every shared lifecycle decision. A method given
/// another adapter's variant returns an error instead of panicking.
pub(crate) trait Adapter: Sync {
    fn kind(&self) -> IntegrationKind;
    fn namespace(&self) -> &'static str;
    fn max_requests(&self) -> u32;
    fn parse_target(&self, text: &str) -> Result<IntegrationTarget, StoreError>;
    /// A kind always has a scope (Jira Cloud ID), never has one (GitHub), or has none until the
    /// engine writes it once on the adapter's `Step::Pin` (Todoist account).
    fn validate_scope(&self, scope: Option<&str>) -> Result<(), StoreError>;
    fn validate_config(&self, config: &IntegrationConfig) -> Result<(), StoreError>;
    fn admit(
        &self,
        profile: &IntegrationProfile,
        target: &IntegrationTarget,
    ) -> Result<(), StoreError>;
    fn authorized(
        &self,
        profile: &IntegrationProfile,
        saved: &IntegrationMetadata,
    ) -> Result<(), StoreError>;
    fn same_identity(&self, old: &IntegrationMetadata, new: &IntegrationMetadata) -> bool;
    fn start(
        &self,
        profile: &IntegrationProfile,
        target: &IntegrationTarget,
    ) -> Result<Started, StoreError>;
    fn pause(&self, response: &IntegrationResponse, now: i64) -> Result<Option<Pause>, StoreError>;
    fn validate_metadata(&self, metadata: &IntegrationMetadata) -> Result<(), StoreError>;
    fn validate_object(
        &self,
        object: &IntegrationObject,
        user_edited: bool,
    ) -> Result<(), StoreError>;
}
pub(crate) trait Reader: Send {
    /// `reserved`: category steps must stop; only the final request and body reserve remain.
    fn next(
        &mut self,
        reserved: bool,
        window: &mut IntegrationWindow,
    ) -> Result<IntegrationRequest, StoreError>;
    fn accept(
        &mut self,
        response: &IntegrationResponse,
        now: i64,
        window: &mut IntegrationWindow,
    ) -> Step;
}
pub(crate) struct IntegrationRead {
    pub id: u64,
    pub task: TaskVersion,
    pub profile: IntegrationProfile,
    pub target: IntegrationTarget,
    pub session_expires_at: i64,
    pub epoch: u64,
    pub cancelled: Arc<AtomicBool>,
    pub started: Instant,
    pub io_active: bool,
    pub dispatched: bool,
    pub pending: bool,
    pub done: bool,
    pub requests: u32,
    pub body_bytes: usize,
    pub preview: Option<Vec<usize>>,
    pub failure: u32,
    pub window: IntegrationWindow,
    pub reader: Box<dyn Reader>,
}
