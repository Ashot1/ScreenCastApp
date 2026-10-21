use crate::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, atomic::AtomicBool};
pub const LOCAL_BRANCH_NAMESPACE: &str = "local-git-branch-v1";
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_SOURCE_FILES: usize = 256;
pub const MAX_SOURCE_ENTRIES: usize = 4096;
pub const MAX_SOURCE_BRANCHES: usize = 256;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalRootSettings {
    pub path: String,
    pub revision: u64,
    pub exclusions: Vec<String>,
}
impl LocalRootSettings {
    pub fn excluded(&self, path: &str) -> bool {
        self.exclusions
            .iter()
            .any(|p| path == p || path.strip_prefix(p).is_some_and(|s| s.starts_with('/')))
    }
    pub fn skip_packed(&self) -> bool {
        self.excluded(".git/refs/heads")
            || self
                .exclusions
                .iter()
                .any(|p| p.starts_with(".git/refs/heads/"))
    }
    pub fn permits_evidence(&self, full_ref: &str) -> bool {
        ![
            ".git",
            ".git/config",
            ".git/HEAD",
            ".git/packed-refs",
            ".git/commondir",
            ".git/refs",
            ".git/refs/heads",
        ]
        .iter()
        .any(|p| self.excluded(p))
            && !self.excluded(&format!(".git/{full_ref}"))
    }
    pub fn scope(&self) -> String {
        local_root_scope(&self.path)
    }
}
pub fn local_root_scope(path: &str) -> String {
    let mut h = Sha256::new();
    // A separate versioned encoding identifies a displayed location, never an inode or grant.
    h.update(b"ambient-local-root\x01");
    h.update((path.len() as u32).to_be_bytes());
    h.update(path.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn relative_path(path: &str, max: usize) -> Result<(), StoreError> {
    if path.is_empty()
        || path.len() > max
        || path.contains('\0')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn validate_root(path: &str, exclusions: &[String]) -> Result<(), StoreError> {
    if !path.starts_with('/')
        || path.len() > 4096
        || path.contains('\0')
        || (path != "/" && relative_path(&path[1..], 4095).is_err())
    {
        return Err(StoreError::InvalidInput);
    }
    if exclusions.len() > 16
        || path.len() + exclusions.iter().map(String::len).sum::<usize>() + 8 + exclusions.len() * 8
            > 16384
    {
        return Err(StoreError::ResourceLimit);
    }
    let mut seen = std::collections::BTreeSet::new();
    for p in exclusions {
        relative_path(p, 1024)?;
        if !seen.insert(p) {
            return Err(StoreError::InvalidInput);
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum LocalReadReason {
    Excluded = 1,
    Limit = 2,
    Unavailable = 3,
    InvalidName = 4,
    Malformed = 5,
    Unsupported = 6,
    Changed = 7,
    Symbolic = 8,
}
#[derive(Clone, Debug)]
pub struct LocalGitFile {
    pub path: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum BranchHead {
    Unknown = 0,
    Current = 1,
    Other = 2,
    Detached = 3,
    Unborn = 4,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBranch {
    pub full_ref: String,
    pub oid: Option<String>,
    pub head: BranchHead,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalObservation {
    pub started_at_utc: i64,
    pub ended_at_utc: i64,
    pub completeness: Completeness,
    pub reasons: Vec<LocalReadReason>,
    pub branches: Vec<LocalBranch>,
}
impl LocalObservation {
    pub fn evidence(
        &self,
        root: &LocalRootSettings,
        index: usize,
    ) -> Result<(SourceKey, Option<String>, SourceEvidence), StoreError> {
        let b = self.branches.get(index).ok_or(StoreError::InvalidInput)?;
        let text = format!(
            "Dated local Git branch observation (not current repository state).\nRoot: {}\nFull ref: {}\nTip OID: {}\nHEAD relation: {:?}\nObservation interval (local Unix UTC seconds): {}..{}\nCoverage: {:?}; reasons: {:?}\nProvenance: bounded native Git metadata, parsed by Core; no workfiles, index, objects or network. The OID does not prove object availability, commit date, unfinished work, branch age or PR absence.",
            root.path,
            b.full_ref,
            b.oid.as_deref().unwrap_or("unknown"),
            b.head,
            self.started_at_utc,
            self.ended_at_utc,
            self.completeness,
            self.reasons
        );
        if text.len() > MAX_TEXT_BYTES {
            return Err(StoreError::TextTooLong);
        }
        Ok((
            SourceKey {
                namespace: LOCAL_BRANCH_NAMESPACE.into(),
                scope: root.scope(),
                object_id: b.full_ref.clone(),
            },
            b.oid.clone(),
            SourceEvidence {
                text,
                observed_at_utc: Some(self.ended_at_utc),
                completeness: self.completeness,
                kind: AssertionKind::SourceFact,
            },
        ))
    }
}
pub(crate) struct LocalReadOperation {
    pub id: u64,
    pub task: TaskVersion,
    pub root: LocalRootSettings,
    pub epoch: u64,
    pub cancelled: Arc<AtomicBool>,
    pub io_active: bool,
    pub observation: Option<LocalObservation>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalReadStats {
    pub bytes: usize,
    pub files: usize,
    pub directories: usize,
    pub entries: usize,
}
