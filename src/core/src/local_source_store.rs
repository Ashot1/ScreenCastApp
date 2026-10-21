use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
impl Store {
    pub fn local_roots(&self) -> Result<Vec<LocalRootSettings>, StoreError> {
        roots(&self.connection)
    }
    pub fn local_root(&self, path: &str) -> Result<LocalRootSettings, StoreError> {
        root(&self.connection, path)
    }
    pub fn save_local_root(
        &mut self,
        path: &str,
        expected: u64,
        exclusions: &[String],
    ) -> Result<LocalRootSettings, StoreError> {
        validate_root(path, exclusions)?;
        let result = self.change(|c| {
            let old = match root(c,path) {Ok(r)=>Some(r),Err(StoreError::NotFound)=>None,Err(e)=>return Err(e)};
            if old.as_ref().map_or(0,|r|r.revision) != expected {return Err(StoreError::StaleRevision);}
            let all = roots(c)?;
            if old.is_none() && all.len() >= 64 {return Err(StoreError::ResourceLimit);}
            let bytes = |p: &str, ex: &[String]| p.len()+8+ex.iter().map(|s|s.len()+8).sum::<usize>();
            let total: usize = all.iter().filter(|r|r.path != path).map(|r|bytes(&r.path,&r.exclusions)).sum();
            if total + bytes(path,exclusions) > 1024*1024 {return Err(StoreError::ResourceLimit);}
            let clock: i64 = c.query_row("SELECT generation FROM local_source_clock WHERE singleton=1",[],|r|r.get(0))?;
            let revision = clock.checked_add(1).ok_or(StoreError::ResourceLimit)?;
            c.execute("UPDATE local_source_clock SET generation=?1 WHERE singleton=1",[revision])?;
            c.execute("INSERT INTO local_roots(path,revision) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET revision=excluded.revision",params![path,revision])?;
            c.execute("DELETE FROM local_exclusions WHERE root_path=?1",[path])?;
            for (i,p) in exclusions.iter().enumerate() {c.execute("INSERT INTO local_exclusions VALUES(?1,?2,?3)",params![path,i as i64,p])?;}
            root(c,path)
        })?;
        self.cancel_local_read(0);
        self.cancel_codex_read(0);
        Ok(result)
    }
    pub fn delete_local_root(&mut self, path: &str, expected: u64) -> Result<(), StoreError> {
        self.change(|c| {
            let old = root(c, path)?;
            if old.revision != expected {
                return Err(StoreError::StaleRevision);
            }
            c.execute("DELETE FROM local_exclusions WHERE root_path=?1", [path])?;
            c.execute("DELETE FROM local_roots WHERE path=?1", [path])?;
            Ok(())
        })?;
        self.cancel_local_read(0);
        self.cancel_codex_read(0);
        Ok(())
    }
    pub fn begin_local_read(
        &mut self,
        task: TaskVersion,
        path: &str,
        revision: u64,
    ) -> Result<u64, StoreError> {
        if self.local_read.as_ref().is_some_and(|r| r.io_active) {
            return Err(StoreError::Busy);
        }
        self.require_assistance()?;
        plan::open_fence(&self.connection, task.task_id, task.revision)?;
        integrations::enabled(&self.connection, BuiltInIntegration::LocalProjects)?;
        let root = root(&self.connection, path)?;
        if root.revision != revision {
            return Err(StoreError::StaleRevision);
        }
        let id = self.next_source;
        self.next_source = id.checked_add(1).ok_or(StoreError::ResourceLimit)?;
        self.local_read = Some(LocalReadOperation {
            id,
            task,
            root,
            epoch: self.model_session.epoch,
            cancelled: Arc::new(AtomicBool::new(false)),
            io_active: true,
            observation: None,
        });
        Ok(id)
    }
    fn source_fence(&self, id: u64) -> Result<&LocalReadOperation, StoreError> {
        self.require_assistance()?;
        let r = self
            .local_read
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)?;
        if r.cancelled.load(Ordering::Acquire) {
            return Err(StoreError::Cancelled);
        }
        if r.epoch != self.model_session.epoch {
            return Err(StoreError::StaleRevision);
        }
        plan::open_fence(&self.connection, r.task.task_id, r.task.revision)?;
        integrations::enabled(&self.connection, BuiltInIntegration::LocalProjects)?;
        if root(&self.connection, &r.root.path)?.revision != r.root.revision {
            return Err(StoreError::StaleRevision);
        }
        Ok(r)
    }
    pub fn permit_local_read(
        &self,
        id: u64,
        path: &str,
        directory: bool,
    ) -> Result<bool, StoreError> {
        let r = self.source_fence(id)?;
        if !r.io_active {
            return Err(StoreError::InvalidInput);
        }
        if !path.is_empty() {
            relative_path(path, 2048)?;
        }
        if path.starts_with(".git/refs/heads/") {
            crate::local_git::local_ref(&path[5..])?;
        }
        let allowed = if directory {
            matches!(path, "" | ".git" | ".git/refs" | ".git/refs/heads")
                || path.starts_with(".git/refs/heads/")
        } else {
            matches!(
                path,
                ".git/config" | ".git/HEAD" | ".git/packed-refs" | ".git/commondir"
            ) || path.starts_with(".git/refs/heads/")
        };
        if !allowed {
            return Err(StoreError::InvalidInput);
        }
        Ok(!r.root.excluded(path) && (path != ".git/packed-refs" || !r.root.skip_packed()))
    }
    pub fn local_read_cancellation(&self, id: u64) -> Result<Arc<AtomicBool>, StoreError> {
        self.local_read
            .as_ref()
            .filter(|r| r.id == id)
            .map(|r| r.cancelled.clone())
            .ok_or(StoreError::StaleRevision)
    }
    pub fn cancel_local_read(&self, id: u64) {
        if let Some(r) = self.local_read.as_ref().filter(|r| id == 0 || r.id == id) {
            r.cancelled.store(true, Ordering::Release);
        }
    }
    pub(super) fn invalidate_stale_source(&self) {
        if let Some(r) = &self.local_read
            && self.source_fence(r.id).is_err()
        {
            r.cancelled.store(true, Ordering::Release);
        }
    }
    /// Native calls only after every accepted syscall has returned and its operation FDs have closed.
    pub fn finish_local_read(
        &mut self,
        id: u64,
        files: &[LocalGitFile],
        stats: LocalReadStats,
        start: i64,
        end: i64,
        reasons: &[LocalReadReason],
    ) -> Result<LocalObservation, StoreError> {
        let r = self
            .local_read
            .as_mut()
            .filter(|r| r.id == id && r.io_active)
            .ok_or(StoreError::StaleRevision)?;
        r.io_active = false;
        let root = self.source_fence(id)?.root.clone();
        if stats.bytes > MAX_SOURCE_BYTES
            || stats.files > 256
            || stats.directories > 256
            || stats.entries > MAX_SOURCE_ENTRIES
            || stats.files < files.len()
            || files.iter().map(|f| f.bytes.len()).sum::<usize>() > stats.bytes
        {
            return Err(StoreError::ResourceLimit);
        }
        let o =
            crate::local_git::parse_with_entries(&root, files, start, end, reasons, stats.entries)?;
        self.local_read
            .as_mut()
            .ok_or(StoreError::StaleRevision)?
            .observation = Some(o.clone());
        Ok(o)
    }
    pub fn abort_local_read(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self
            .local_read
            .as_mut()
            .filter(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)?;
        r.cancelled.store(true, Ordering::Release);
        r.io_active = false;
        r.observation = None;
        Ok(())
    }
    pub fn release_local_read(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self
            .local_read
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)?;
        if r.io_active {
            return Err(StoreError::Busy);
        }
        self.local_read = None;
        Ok(())
    }
    pub fn local_observation(&self, id: u64) -> Result<LocalObservation, StoreError> {
        self.source_fence(id)?
            .observation
            .clone()
            .ok_or(StoreError::Busy)
    }
    pub fn local_branch_evidence(
        &self,
        id: u64,
        index: usize,
    ) -> Result<(SourceKey, Option<String>, SourceEvidence), StoreError> {
        let r = self.source_fence(id)?;
        r.observation
            .as_ref()
            .ok_or(StoreError::Busy)?
            .evidence(&r.root, index)
    }
    pub fn bind_local_branch(
        &mut self,
        id: u64,
        index: usize,
        at: i64,
    ) -> Result<EvidenceId, StoreError> {
        let task = self.source_fence(id)?.task;
        let (key, version, evidence) = self.local_branch_evidence(id, index)?;
        self.bind_source(
            task.task_id,
            task.revision,
            &key,
            version.as_deref(),
            &evidence,
            at,
        )
    }
}
pub(super) fn root(c: &Connection, path: &str) -> Result<LocalRootSettings, StoreError> {
    let revision = c
        .query_row(
            "SELECT revision FROM local_roots WHERE path=?1",
            [path],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    let mut q =
        c.prepare("SELECT prefix FROM local_exclusions WHERE root_path=?1 ORDER BY position")?;
    let exclusions = q
        .query_map([path], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LocalRootSettings {
        path: path.into(),
        revision: revision as u64,
        exclusions,
    })
}
fn roots(c: &Connection) -> Result<Vec<LocalRootSettings>, StoreError> {
    let mut q = c.prepare("SELECT path FROM local_roots ORDER BY path LIMIT 65")?;
    let paths = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if paths.len() > 64 {
        return Err(StoreError::ResourceLimit);
    }
    paths.iter().map(|p| root(c, p)).collect()
}
pub(super) fn evidence_root(
    c: &Connection,
    key: &SourceKey,
) -> Result<LocalRootSettings, StoreError> {
    if key.namespace != LOCAL_BRANCH_NAMESPACE {
        return Err(StoreError::InvalidInput);
    }
    crate::local_git::local_ref(&key.object_id)?;
    let r = roots(c)?
        .into_iter()
        .find(|r| r.scope() == key.scope)
        .ok_or(StoreError::StaleRevision)?;
    if !r.permits_evidence(&key.object_id) {
        return Err(StoreError::InvalidInput);
    }
    Ok(r)
}
pub(super) fn validate_roots(c: &Connection) -> Result<(), StoreError> {
    let clock: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let all = roots(c)?;
    let mut total = 0;
    for r in all {
        validate_root(&r.path, &r.exclusions)?;
        if r.revision == 0 || r.revision > clock as u64 {
            return Err(StoreError::UnsupportedSchema);
        }
        total += r.path.len() + 8 + r.exclusions.iter().map(|s| s.len() + 8).sum::<usize>();
    }
    if total > 1024 * 1024 {
        return Err(StoreError::UnsupportedSchema);
    }
    Ok(())
}
