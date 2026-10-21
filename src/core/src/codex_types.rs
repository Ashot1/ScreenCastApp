use crate::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, atomic::AtomicBool};

pub const CODEX_NAMESPACE: &str = "codex-local-jsonl-v1";
pub const CODEX_VERSION: &str = "0.163.0-alpha.10.2";
pub const MAX_CODEX_LINE: usize = 256 * 1024;
pub const MAX_CODEX_WINDOW: usize = 1024 * 1024;
pub const MAX_CODEX_ITEMS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexSelection {
    pub id: u64,
    pub revision: u64,
    pub path: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexFileIdentity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHeader {
    pub thread_id: String,
    pub subagent_start: Option<u64>,
    pub fingerprint: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum CodexEventKind {
    User = 1,
    Agent = 2,
    Plan = 3,
    DelegatedCreate = 4,
    DelegatedSend = 5,
    Started = 6,
    Completed = 7,
    Failed = 8,
    Aborted = 9,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CodexEventIdentity {
    Item {
        turn_id: String,
        item_id: String,
        kind: CodexEventKind,
    },
    Turn {
        turn_id: String,
        kind: CodexEventKind,
    },
}
impl CodexEventIdentity {
    pub(crate) fn source_key(&self, thread_id: &str) -> SourceKey {
        let prefix = match self {
            Self::Item { .. } => "item:",
            Self::Turn {
                kind: CodexEventKind::Started,
                ..
            } => "start:",
            _ => "terminal:",
        };
        let hash = match self.item_id() {
            Some(item) => codex_hash(&[self.turn_id().as_bytes(), item.as_bytes()]),
            None => codex_hash(&[self.turn_id().as_bytes()]),
        };
        SourceKey {
            namespace: CODEX_NAMESPACE.into(),
            scope: thread_id.into(),
            object_id: format!("{prefix}{}", codex_hex(&hash)),
        }
    }
    pub fn kind(&self) -> CodexEventKind {
        match self {
            Self::Item { kind, .. } | Self::Turn { kind, .. } => *kind,
        }
    }
    pub fn turn_id(&self) -> &str {
        match self {
            Self::Item { turn_id, .. } | Self::Turn { turn_id, .. } => turn_id,
        }
    }
    pub fn item_id(&self) -> Option<&str> {
        match self {
            Self::Item { item_id, .. } => Some(item_id),
            Self::Turn { .. } => None,
        }
    }
    pub fn variable_bytes(&self) -> usize {
        self.turn_id().len() + self.item_id().map_or(0, str::len)
    }
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        crate::memory_types::key_field(self.turn_id(), 2048)?;
        match self {
            Self::Item { item_id, kind, .. } if (*kind as u32) <= 5 => {
                crate::memory_types::key_field(item_id, 2048)
            }
            Self::Turn { kind, .. } if (*kind as u32) >= 6 => Ok(()),
            _ => Err(StoreError::InvalidInput),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexEvent {
    pub ordinal: u64,
    pub identity: CodexEventIdentity,
    pub key: SourceKey,
    pub version: String,
    pub evidence: SourceEvidence,
}
impl CodexEvent {
    pub fn variable_bytes(&self) -> usize {
        self.identity.variable_bytes()
            + self.key.bytes()
            + self.version.len()
            + self.evidence.text.len()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CodexReason {
    Partial = 1,
    Limit = 2,
    Malformed = 4,
    Unsupported = 8,
    Ordinal = 16,
    Compacted = 32,
    Filtered = 64,
    Turn = 128,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexWindow {
    pub events: Vec<CodexEvent>,
    pub reasons: u32,
    pub next_before: u64,
    pub physical_lines: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexObservation {
    pub thread_id: String,
    pub observed_at_utc: i64,
    pub cursor: u64,
    pub window: CodexWindow,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodexMetadata {
    pub binding_id: i64,
    pub evidence_id: EvidenceId,
    pub selection_id: u64,
    pub settings_revision: u64,
    pub accepted_ordinal: u64,
    pub identity: CodexEventIdentity,
}
#[derive(Clone)]
pub(crate) struct CodexCursor {
    pub token: u64,
    pub selection: CodexSelection,
    pub file: CodexFileIdentity,
    pub header: CodexHeader,
    pub before: u64,
}
#[derive(Clone, Debug)]
pub struct CodexReadPlan {
    pub start: u64,
    pub end: u64,
}
pub(crate) struct CodexReadOperation {
    pub id: u64,
    pub task: TaskVersion,
    pub selection: CodexSelection,
    pub epoch: u64,
    pub cancelled: Arc<AtomicBool>,
    pub io_active: bool,
    pub prior: Option<CodexCursor>,
    pub cursor: Option<CodexCursor>,
    pub plan: Option<CodexReadPlan>,
    pub window: Option<CodexWindow>,
    pub observed_at_utc: i64,
}
pub(crate) fn codex_hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"ambient-codex\x01");
    for p in parts {
        h.update((p.len() as u32).to_be_bytes());
        h.update(p);
    }
    h.finalize().into()
}
pub(crate) fn codex_hex(hash: &[u8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn codex_event_version(
    thread: &str,
    identity: &CodexEventIdentity,
    evidence: &SourceEvidence,
) -> String {
    let kind = (identity.kind() as u32).to_be_bytes();
    let at = evidence.observed_at_utc.map(|t| t.to_be_bytes());
    codex_hex(&codex_hash(&[
        thread.as_bytes(),
        identity.turn_id().as_bytes(),
        identity.item_id().unwrap_or("").as_bytes(),
        &kind,
        at.as_ref().map_or(&[][..], |t| &t[..]),
        evidence.text.as_bytes(),
        &[u8::from(evidence.completeness == Completeness::Partial)],
    ]))
}
