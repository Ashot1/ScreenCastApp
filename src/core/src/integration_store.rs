use super::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const MAX_PROFILES: usize = 8;
const MAX_EXCLUSIONS: usize = 128;
// Inherited per-kind settings limit, counted over the native profile-list envelope.
const MAX_SETTINGS: usize = 1024 * 1024;

fn adapter(kind: IntegrationKind) -> &'static dyn Adapter {
    match kind {
        IntegrationKind::GitHub => &GitHub,
        IntegrationKind::Jira => &Jira,
    }
}

fn switch(c: &Connection, kind: BuiltInIntegration) -> Result<bool, StoreError> {
    let enabled: i64 = c
        .query_row(
            "SELECT enabled FROM integration_switches WHERE kind=?1",
            [kind.as_str()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::UnsupportedSchema)?;
    Ok(enabled == 1)
}
/// A disabled integration closes reads and every use of its saved facts (D61).
pub(super) fn enabled(c: &Connection, kind: BuiltInIntegration) -> Result<(), StoreError> {
    if switch(c, kind)? {
        Ok(())
    } else {
        Err(StoreError::StaleRevision)
    }
}

pub(super) fn clock(c: &Connection) -> Result<u64, StoreError> {
    let old: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let next = old.checked_add(1).ok_or(StoreError::ResourceLimit)?;
    c.execute(
        "UPDATE local_source_clock SET generation=?1 WHERE singleton=1",
        [next],
    )?;
    Ok(next as u64)
}
pub(super) fn sql_id(id: u64) -> Result<i64, StoreError> {
    i64::try_from(id)
        .ok()
        .filter(|v| *v > 0)
        .ok_or(StoreError::InvalidInput)
}
fn profile(c: &Connection, id: u64) -> Result<IntegrationProfile, StoreError> {
    let row = c
        .query_row(
            "SELECT kind,settings_revision,credential_generation,name,scope,config,not_before_utc,server_deadline_known FROM integration_profiles WHERE id=?1",
            [sql_id(id)?],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    let kind = IntegrationKind::parse(&row.0)?;
    Ok(IntegrationProfile {
        id,
        kind,
        settings_revision: row.1 as u64,
        credential_generation: row.2 as u64,
        name: row.3,
        scope: row.4,
        config: IntegrationConfig::decode(kind, &row.5)?,
        not_before_utc: row.6,
        server_deadline_known: row.7,
    })
}
fn profiles(c: &Connection, kind: IntegrationKind) -> Result<Vec<IntegrationProfile>, StoreError> {
    let mut q =
        c.prepare("SELECT id FROM integration_profiles WHERE kind=?1 ORDER BY id LIMIT 9")?;
    let ids = q
        .query_map([kind.as_str()], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter().map(|id| profile(c, id as u64)).collect()
}
fn exclusions(c: &Connection, kind: IntegrationKind) -> Result<Vec<String>, StoreError> {
    let mut q = c.prepare(
        "SELECT scope FROM integration_exclusions WHERE kind=?1 ORDER BY scope LIMIT 129",
    )?;
    Ok(q.query_map([kind.as_str()], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}
fn excluded(c: &Connection, kind: IntegrationKind, scope: &str) -> Result<bool, StoreError> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM integration_exclusions WHERE kind=?1 AND scope=?2)",
        params![kind.as_str(), scope],
        |r| r.get(0),
    )?)
}
/// An excluded scope closes every profile that carries it, independent of profile lifetime.
fn allowed(c: &Connection, p: &IntegrationProfile) -> Result<(), StoreError> {
    match &p.scope {
        Some(scope) if excluded(c, p.kind, scope)? => Err(StoreError::StaleRevision),
        _ => Ok(()),
    }
}
pub(super) fn instance(c: &Connection) -> Result<String, StoreError> {
    c.query_row(
        "SELECT lower(hex(identity)) FROM store_identity WHERE id=1",
        [],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(StoreError::UnsupportedSchema)
}
fn settings_json(c: &Connection, kind: IntegrationKind) -> Result<String, StoreError> {
    envelope(c, kind, &profiles(c, kind)?, &exclusions(c, kind)?)
}
fn envelope(
    c: &Connection,
    kind: IntegrationKind,
    profiles: &[IntegrationProfile],
    exclusions: &[String],
) -> Result<String, StoreError> {
    let s = serde_json::to_string(
        &json!({"kind":kind,"instance":instance(c)?,"profiles":profiles,"exclusions":exclusions}),
    )
    .map_err(|_| StoreError::InvalidInput)?;
    if s.len() > MAX_SETTINGS {
        return Err(StoreError::ResourceLimit);
    }
    Ok(s)
}
fn validate_profile(p: &IntegrationProfile) -> Result<(), StoreError> {
    for id in [p.id, p.settings_revision, p.credential_generation] {
        sql_id(id)?;
    }
    if p.name.is_empty() || p.name.len() > 256 {
        return Err(StoreError::InvalidInput);
    }
    let a = adapter(p.kind);
    a.validate_scope(p.scope.as_deref())?;
    a.validate_config(&p.config)
}
fn validate_settings(c: &Connection, kind: IntegrationKind) -> Result<(), StoreError> {
    let all = profiles(c, kind)?;
    let excluded = exclusions(c, kind)?;
    if all.len() > MAX_PROFILES || excluded.len() > MAX_EXCLUSIONS {
        return Err(StoreError::ResourceLimit);
    }
    for scope in &excluded {
        adapter(kind).validate_scope(Some(scope))?;
    }
    envelope(c, kind, &all, &excluded)?;
    let clock: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    for p in all {
        validate_profile(&p)?;
        if [p.id, p.settings_revision, p.credential_generation]
            .iter()
            .any(|id| *id > clock as u64)
        {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    Ok(())
}
/// Revocation and staleness both read the authorizing profile named inside the saved metadata.
pub(super) fn authorized(c: &Connection, m: &IntegrationSavedMetadata) -> Result<(), StoreError> {
    let version = m.metadata.profile();
    let p = profile(c, version.id)?;
    if p.kind != m.metadata.kind() {
        return Err(StoreError::StaleRevision);
    }
    allowed(c, &p)?;
    if p.version() != version {
        return Err(StoreError::StaleRevision);
    }
    adapter(p.kind).authorized(&p, &m.metadata)
}
pub(super) fn check_saved(c: &Connection, m: &IntegrationSavedMetadata) -> Result<(), StoreError> {
    authorized(c, m)?;
    if metadata(c, m.binding_id)? != *m {
        return Err(StoreError::StaleRevision);
    }
    Ok(())
}
/// The open-time 08B comparison after a task revision change keeps the old typed identity.
pub(super) fn same_ground(
    c: &Connection,
    old: &IntegrationSavedMetadata,
    now: &IntegrationSavedMetadata,
) -> Result<(), StoreError> {
    authorized(c, old)?;
    let a = adapter(old.metadata.kind());
    a.validate_metadata(&old.metadata)?;
    if old.binding_id != now.binding_id
        || old.evidence_id != now.evidence_id
        || old.metadata.kind() != now.metadata.kind()
        || !a.same_identity(&old.metadata, &now.metadata)
    {
        return Err(StoreError::UnsupportedSchema);
    }
    Ok(())
}
pub(super) fn metadata(
    c: &Connection,
    binding: i64,
) -> Result<IntegrationSavedMetadata, StoreError> {
    let (evidence, namespace, data): (i64, String, String) = c.query_row(
        "SELECT b.evidence_id,b.namespace,m.data FROM source_bindings b JOIN integration_metadata m ON m.binding_id=b.id WHERE b.id=?1 AND b.namespace IS NOT NULL",
        [binding],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let kind = IntegrationKind::from_namespace(&namespace).ok_or(StoreError::UnsupportedSchema)?;
    Ok(IntegrationSavedMetadata {
        binding_id: binding,
        evidence_id: EvidenceId(evidence),
        metadata: IntegrationMetadata::decode(kind, &data)?,
    })
}
pub(super) fn stored_metadata(
    c: &Connection,
    e: EvidenceId,
) -> Result<IntegrationSavedMetadata, StoreError> {
    let b = c.query_row(
        "SELECT id FROM source_bindings WHERE evidence_id=?1 AND namespace IS NOT NULL",
        [e.0],
        |r| r.get(0),
    )?;
    metadata(c, b)
}
/// Replacement, revocation and 401 move revision and generation to one new clock value.
fn bump_credential(c: &Connection, id: u64) -> Result<(), StoreError> {
    let next = clock(c)?;
    c.execute(
        "UPDATE integration_profiles SET settings_revision=?1,credential_generation=?1 WHERE id=?2",
        params![sql_id(next)?, sql_id(id)?],
    )?;
    Ok(())
}
/// The profile only while it still has the generation a response was sent with. A deleted
/// profile is no error; other storage errors stay visible.
fn sent_generation(
    c: &Connection,
    id: u64,
    generation: u64,
) -> Result<Option<IntegrationProfile>, StoreError> {
    match profile(c, id) {
        Ok(p) if p.credential_generation == generation => Ok(Some(p)),
        Ok(_) | Err(StoreError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}
fn merge(stored: (Option<i64>, bool), pause: &Pause) -> (Option<i64>, bool) {
    match (stored.0, pause.until) {
        (_, None) => stored,
        (Some(a), Some(b)) if a > b => stored,
        (Some(a), Some(b)) if a == b => (Some(a), stored.1 || pause.known),
        (_, Some(b)) => (Some(b), pause.known),
    }
}
pub(super) fn validate_integrations(c: &Connection) -> Result<(), StoreError> {
    // The schema allows only id 1, so a readable identity means exactly one row.
    instance(c)?;
    // The kind key and CHECK allow only the four built-in kinds, so four rows means all of them.
    let switches: i64 = c.query_row("SELECT count(*) FROM integration_switches", [], |r| {
        r.get(0)
    })?;
    if switches != 4 {
        return Err(StoreError::UnsupportedSchema);
    }
    let mut q = c.prepare(
        "SELECT kind FROM integration_profiles UNION SELECT kind FROM integration_exclusions",
    )?;
    let kinds = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for kind in kinds {
        validate_settings(c, IntegrationKind::parse(&kind)?)?;
    }
    for a in crate::integration_types::ADAPTERS {
        let missing: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_bindings b WHERE b.namespace=?1 AND NOT EXISTS(SELECT 1 FROM integration_metadata m WHERE m.binding_id=b.id))",
            [a.namespace()],
            |r| r.get(0),
        )?;
        if missing {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    let clock: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    // Plugin results share the metadata table and are validated by the plugin module.
    let mut q = c.prepare("SELECT m.binding_id FROM integration_metadata m LEFT JOIN source_bindings b ON b.id=m.binding_id WHERE b.namespace IS NOT ?1 ORDER BY m.binding_id")?;
    let ids = q
        .query_map([PLUGIN_NAMESPACE], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for b in ids {
        // A metadata row whose binding is forgotten or foreign has no live namespace to decode by.
        let m = metadata(c, b)?;
        let e = read_evidence(c, m.evidence_id)?;
        let Origin::External { key, version } = e.origin else {
            return Err(StoreError::UnsupportedSchema);
        };
        let o = IntegrationObject {
            key,
            version: version.ok_or(StoreError::UnsupportedSchema)?,
            text: e.text,
            observed_at_utc: e.observed_at_utc,
            metadata: m.metadata,
            bindable: true,
        };
        adapter(o.metadata.kind())
            .validate_object(&o, e.kind == AssertionKind::UserConfirmation)?;
        let v = o.metadata.profile();
        if [v.id, v.settings_revision, v.credential_generation]
            .iter()
            .any(|id| *id > clock as u64)
        {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    Ok(())
}
fn fail(r: &mut IntegrationRead, code: u32) -> Result<(), StoreError> {
    r.failure = code;
    r.done = false;
    r.pending = false;
    r.preview = None;
    r.window.objects.clear();
    Err(StoreError::InvalidInput)
}

impl Store {
    pub fn integration_profiles(
        &self,
        kind: IntegrationKind,
    ) -> Result<Vec<IntegrationProfile>, StoreError> {
        profiles(&self.connection, kind)
    }
    pub fn integration_profile(&self, id: u64) -> Result<IntegrationProfile, StoreError> {
        profile(&self.connection, id)
    }
    pub fn integration_exclusions(&self, kind: IntegrationKind) -> Result<Vec<String>, StoreError> {
        exclusions(&self.connection, kind)
    }
    pub fn integration_settings_json(&self, kind: IntegrationKind) -> Result<String, StoreError> {
        settings_json(&self.connection, kind)
    }
    pub fn save_integration_profile(
        &mut self,
        id: u64,
        expected: u64,
        name: &str,
        scope: Option<&str>,
        config: &IntegrationConfig,
    ) -> Result<IntegrationProfile, StoreError> {
        let kind = config.kind();
        self.change(|c| {
            let old = if id == 0 {
                if expected != 0 {
                    return Err(StoreError::InvalidInput);
                }
                None
            } else {
                // IDs come from one clock across kinds, so another kind's profile is never this target.
                let stored: String = c
                    .query_row(
                        "SELECT kind FROM integration_profiles WHERE id=?1",
                        [sql_id(id)?],
                        |r| r.get(0),
                    )
                    .optional()?
                    .ok_or(StoreError::NotFound)?;
                if stored != kind.as_str() {
                    return Err(StoreError::InvalidInput);
                }
                let p = profile(c, id)?;
                if p.settings_revision != expected {
                    return Err(StoreError::StaleRevision);
                }
                if p.scope.as_deref() != scope {
                    return Err(StoreError::InvalidInput);
                }
                Some(p)
            };
            let all = profiles(c, kind)?;
            if scope.is_some() && all.iter().any(|p| p.id != id && p.scope.as_deref() == scope) {
                return Err(StoreError::InvalidInput);
            }
            if old.is_none() && all.len() >= MAX_PROFILES {
                return Err(StoreError::ResourceLimit);
            }
            let revision = clock(c)?;
            let p = IntegrationProfile {
                id: old.as_ref().map_or(revision, |p| p.id),
                kind,
                settings_revision: revision,
                credential_generation: old.as_ref().map_or(revision, |p| p.credential_generation),
                name: name.into(),
                scope: scope.map(str::to_owned),
                config: config.clone(),
                not_before_utc: old.as_ref().and_then(|p| p.not_before_utc),
                server_deadline_known: old.as_ref().is_some_and(|p| p.server_deadline_known),
            };
            validate_profile(&p)?;
            let config = serde_json::to_string(&p.config).map_err(|_| StoreError::InvalidInput)?;
            c.execute("INSERT INTO integration_profiles(id,kind,settings_revision,credential_generation,name,scope,config,not_before_utc,server_deadline_known) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET settings_revision=excluded.settings_revision,name=excluded.name,config=excluded.config",params![sql_id(p.id)?,kind.as_str(),sql_id(p.settings_revision)?,sql_id(p.credential_generation)?,p.name,p.scope,config,p.not_before_utc,p.server_deadline_known])?;
            validate_settings(c, kind)?;
            Ok(p)
        })
    }
    pub fn integration_enabled(&self, kind: BuiltInIntegration) -> Result<bool, StoreError> {
        switch(&self.connection, kind)
    }
    /// Disabling clears dependent 08B areas in the same transaction and cancels this kind's
    /// operations through the fences; saved facts, profiles, revisions and keys stay unchanged.
    pub fn set_integration_enabled(
        &mut self,
        kind: BuiltInIntegration,
        enabled: bool,
    ) -> Result<(), StoreError> {
        // The same value must not reach `change`, which always runs `clear_revoked`.
        if switch(&self.connection, kind)? == enabled {
            return Ok(());
        }
        self.change(|c| {
            c.execute(
                "UPDATE integration_switches SET enabled=?1 WHERE kind=?2",
                params![enabled, kind.as_str()],
            )?;
            Ok(())
        })
    }
    pub fn set_integration_scope_excluded(
        &mut self,
        kind: IntegrationKind,
        scope: &str,
        excluded: bool,
    ) -> Result<(), StoreError> {
        adapter(kind).validate_scope(Some(scope))?;
        self.change(|c| {
            if self::excluded(c, kind, scope)? == excluded {
                return Ok(());
            }
            if excluded {
                c.execute(
                    "INSERT INTO integration_exclusions(kind,scope) VALUES(?1,?2)",
                    params![kind.as_str(), scope],
                )?;
            } else {
                c.execute(
                    "DELETE FROM integration_exclusions WHERE kind=?1 AND scope=?2",
                    params![kind.as_str(), scope],
                )?;
            }
            let revision = clock(c)?;
            // The settings fence changes on both transitions; exclude→allow cannot revive an earlier response.
            c.execute(
                "UPDATE integration_profiles SET settings_revision=?1 WHERE kind=?2 AND scope=?3",
                params![sql_id(revision)?, kind.as_str(), scope],
            )?;
            validate_settings(c, kind)
        })
    }
    /// Starts both replacement and revocation: settings revision and generation take one new clock value.
    pub fn replace_integration_credential(
        &mut self,
        id: u64,
        expected: u64,
    ) -> Result<IntegrationProfile, StoreError> {
        self.change(|c| {
            let p = profile(c, id)?;
            if p.settings_revision != expected {
                return Err(StoreError::StaleRevision);
            }
            bump_credential(c, id)?;
            profile(c, id)
        })
    }
    /// Native calls this only after confirmed secret deletion, with the exact revoked version.
    pub fn finish_integration_profile_deletion(
        &mut self,
        id: u64,
        revision: u64,
        generation: u64,
    ) -> Result<(), StoreError> {
        self.change(|c| {
            let p = profile(c, id)?;
            if p.settings_revision != revision || p.credential_generation != generation {
                return Err(StoreError::StaleRevision);
            }
            c.execute(
                "DELETE FROM integration_profiles WHERE id=?1",
                [sql_id(id)?],
            )?;
            Ok(())
        })
    }

    pub fn begin_integration_read(
        &mut self,
        task: TaskVersion,
        version: IntegrationProfileVersion,
        target: &str,
        now: i64,
        expires: i64,
    ) -> Result<u64, StoreError> {
        self.require_assistance()?;
        plan::open_fence(&self.connection, task.task_id, task.revision)?;
        if expires <= now {
            return Err(StoreError::SessionRequired);
        }
        // The profile reference resolves the kind and therefore the one read slot it uses.
        let p = profile(&self.connection, version.id)?;
        if self
            .integration_reads
            .get(&p.kind)
            .is_some_and(|r| r.io_active)
        {
            return Err(StoreError::Busy);
        }
        let a = adapter(p.kind);
        let target = a.parse_target(target)?;
        enabled(&self.connection, p.kind.into())?;
        allowed(&self.connection, &p)?;
        a.admit(&p, &target)?;
        if p.version() != version {
            return Err(StoreError::StaleRevision);
        }
        if p.not_before_utc.is_some_and(|t| now < t) {
            return Err(StoreError::Busy);
        }
        let started = a.start(&p, &target)?;
        let id = self.next_source;
        self.next_source = id.checked_add(1).ok_or(StoreError::ResourceLimit)?;
        self.integration_reads.insert(
            p.kind,
            IntegrationRead {
                id,
                task,
                profile: p,
                target,
                session_expires_at: expires,
                epoch: self.model_session.epoch,
                cancelled: Arc::new(AtomicBool::new(false)),
                started: Instant::now(),
                io_active: true,
                dispatched: false,
                pending: false,
                done: false,
                requests: 0,
                body_bytes: 0,
                preview: None,
                failure: 0,
                window: IntegrationWindow {
                    observation_id: id,
                    interval_start: now,
                    interval_end: now,
                    categories: started.coverage,
                    objects: Vec::new(),
                },
                reader: started.reader,
            },
        );
        Ok(id)
    }
    fn integration_read(&self, id: u64) -> Result<&IntegrationRead, StoreError> {
        self.integration_reads
            .values()
            .find(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)
    }
    fn integration_read_mut(&mut self, id: u64) -> Result<&mut IntegrationRead, StoreError> {
        self.integration_reads
            .values_mut()
            .find(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)
    }
    fn integration_fence(&self, id: u64) -> Result<&IntegrationRead, StoreError> {
        self.require_assistance()?;
        let r = self.integration_read(id)?;
        if r.cancelled.load(Ordering::Acquire) {
            return Err(StoreError::Cancelled);
        }
        let p = profile(&self.connection, r.profile.id)?;
        enabled(&self.connection, p.kind.into())?;
        allowed(&self.connection, &p)?;
        adapter(p.kind).admit(&p, &r.target)?;
        if p.version() != r.profile.version() || r.epoch != self.model_session.epoch {
            return Err(StoreError::StaleRevision);
        }
        plan::open_fence(&self.connection, r.task.task_id, r.task.revision)?;
        Ok(r)
    }
    pub fn integration_read_kind(&self, id: u64) -> Result<IntegrationKind, StoreError> {
        Ok(self.integration_read(id)?.profile.kind)
    }
    pub fn validate_integration_session_time(&self, id: u64, now: i64) -> Result<(), StoreError> {
        let r = self.integration_fence(id)?;
        if now >= r.session_expires_at {
            r.cancelled.store(true, Ordering::Release);
            return Err(StoreError::SessionRequired);
        }
        Ok(())
    }
    pub fn integration_read_cancellation(&self, id: u64) -> Result<Arc<AtomicBool>, StoreError> {
        Ok(self.integration_read(id)?.cancelled.clone())
    }
    pub fn cancel_integration_read(&self, id: u64) {
        if let Ok(r) = self.integration_read(id) {
            r.cancelled.store(true, Ordering::Release);
        }
    }
    pub(super) fn cancel_integration_reads(&self) {
        for r in self.integration_reads.values() {
            r.cancelled.store(true, Ordering::Release);
        }
    }
    pub(super) fn invalidate_stale_integrations(&self) {
        for r in self.integration_reads.values() {
            if self.integration_fence(r.id).is_err() {
                r.cancelled.store(true, Ordering::Release);
            }
        }
    }
    pub fn start_integration_transport(&mut self, id: u64) -> Result<(), StoreError> {
        self.integration_fence(id)?;
        let r = self.integration_read_mut(id)?;
        if r.dispatched || !r.io_active {
            return Err(StoreError::Busy);
        }
        r.dispatched = true;
        Ok(())
    }
    pub fn abort_unused_integration_read(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self.integration_read_mut(id)?;
        if r.dispatched {
            return Err(StoreError::Busy);
        }
        r.cancelled.store(true, Ordering::Release);
        r.io_active = false;
        Ok(())
    }
    pub fn permit_integration_step(
        &mut self,
        id: u64,
        now: i64,
    ) -> Result<IntegrationStep, StoreError> {
        self.validate_integration_session_time(id, now)?;
        let r = self.integration_read_mut(id)?;
        if !r.io_active || !r.dispatched || r.pending || r.done || r.failure != 0 {
            return Err(StoreError::InvalidInput);
        }
        let remaining = Duration::from_secs(30)
            .checked_sub(r.started.elapsed())
            .filter(|d| !d.is_zero())
            .ok_or(StoreError::Cancelled)?;
        let max = adapter(r.profile.kind).max_requests();
        // Core refuses beyond the adapter maximum; the dispatch loop has no step count of its own.
        if r.requests >= max {
            return Err(StoreError::ResourceLimit);
        }
        // Category steps leave one request and one full body for the final check; neither bypasses
        // the deadline or the server pause.
        let reserved = r.requests + 1 >= max || r.body_bytes > 6 * MAX_INTEGRATION_BODY;
        let request = r.reader.next(reserved, &mut r.window)?;
        r.requests += 1;
        r.pending = true;
        Ok(IntegrationStep { request, remaining })
    }
    pub fn accept_integration_response(
        &mut self,
        id: u64,
        response: IntegrationResponse,
        now: i64,
    ) -> Result<(), StoreError> {
        let r = self
            .integration_read(id)
            .ok()
            .filter(|r| r.pending)
            .ok_or(StoreError::StaleRevision)?;
        let profile_id = r.profile.id;
        let generation = r.profile.credential_generation;
        let kind = r.profile.kind;
        // A credential rejection is meaningful even after a task change, but only for the exact
        // generation sent.
        if response.status == 401 {
            self.change(|c| {
                if sent_generation(c, profile_id, generation)?.is_some() {
                    bump_credential(c, profile_id)?;
                }
                Ok(())
            })?;
            return self.fail_integration_read(id, 401);
        }
        if let Some(pause) = adapter(kind).pause(response.status, &response.headers, now)? {
            self.change(|c| {
                if let Some(p) = sent_generation(c, profile_id, generation)? {
                    let (until, known) = merge((p.not_before_utc, p.server_deadline_known), &pause);
                    c.execute("UPDATE integration_profiles SET not_before_utc=?1,server_deadline_known=?2 WHERE id=?3",params![until,known,sql_id(profile_id)?])?;
                }
                Ok(())
            })?;
            return self.fail_integration_read(id, pause.failure);
        }
        self.validate_integration_session_time(id, now)?;
        let r = self.integration_read_mut(id)?;
        r.pending = false;
        if r.started.elapsed() >= Duration::from_secs(30)
            || now < r.window.interval_end
            || response.body.len() > MAX_INTEGRATION_BODY
            || r.body_bytes
                .checked_add(response.body.len())
                .is_none_or(|b| b > 8 * MAX_INTEGRATION_BODY)
        {
            return fail(r, 408);
        }
        r.body_bytes += response.body.len();
        r.window.interval_end = now;
        match r.reader.accept(&response, now, &mut r.window) {
            Step::Continue => Ok(()),
            Step::Done => {
                r.done = true;
                Ok(())
            }
            Step::Fail(code) => fail(r, code),
        }
    }
    pub fn fail_integration_read(&mut self, id: u64, code: u32) -> Result<(), StoreError> {
        fail(self.integration_read_mut(id)?, code)
    }
    /// Only the transport owner calls this after its client, runtime and I/O have actually completed.
    pub fn finish_integration_transport(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self
            .integration_read_mut(id)
            .ok()
            .filter(|r| r.io_active)
            .ok_or(StoreError::StaleRevision)?;
        r.io_active = false;
        if r.started.elapsed() >= Duration::from_secs(30) {
            r.done = false;
            r.failure = 408;
        }
        if !r.done || r.pending {
            r.window.objects.clear();
        }
        Ok(())
    }
    pub fn integration_read_status(&self, id: u64) -> Result<u32, StoreError> {
        let r = self.integration_read(id)?;
        Ok(if r.failure != 0 {
            r.failure
        } else if r.cancelled.load(Ordering::Acquire) {
            499
        } else if r.done {
            200
        } else {
            500
        })
    }
    pub fn release_integration_read(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self.integration_read(id)?;
        if r.io_active {
            return Err(StoreError::Busy);
        }
        let kind = r.profile.kind;
        self.integration_reads.remove(&kind);
        Ok(())
    }
    fn integration_window(&self, id: u64) -> Result<&IntegrationWindow, StoreError> {
        let r = self.integration_fence(id)?;
        if r.io_active || !r.done {
            return Err(StoreError::Busy);
        }
        Ok(&r.window)
    }
    pub fn integration_window_page(&self, id: u64, after: usize) -> Result<String, StoreError> {
        let w = self.integration_window(id)?;
        if after > w.objects.len() {
            return Err(StoreError::InvalidInput);
        }
        let mut end = after;
        let mut result = String::new();
        loop {
            let next = (end < w.objects.len()).then_some(end);
            let s=serde_json::to_string(&json!({"observation_id":id,"interval_start":w.interval_start,"interval_end":w.interval_end,"categories":w.categories,"objects":&w.objects[after..end],"next_after":next})).map_err(|_|StoreError::InvalidInput)?;
            if s.len() > MAX_PAGE_BYTES {
                break;
            }
            result = s;
            if end == w.objects.len() || end - after == 50 {
                break;
            }
            end += 1;
        }
        if result.is_empty() {
            return Err(StoreError::ResourceLimit);
        }
        Ok(result)
    }
    fn selected_integration(
        &self,
        id: u64,
        indices: &[usize],
    ) -> Result<Vec<IntegrationObject>, StoreError> {
        let w = self.integration_window(id)?;
        if indices.is_empty()
            || indices.len() > 32
            || indices
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != indices.len()
        {
            return Err(StoreError::InvalidInput);
        }
        indices
            .iter()
            .map(|i| {
                w.objects
                    .get(*i)
                    .filter(|o| o.bindable)
                    .cloned()
                    .ok_or(StoreError::InvalidInput)
            })
            .collect()
    }
    pub fn integration_preview(
        &mut self,
        id: u64,
        indices: &[usize],
    ) -> Result<String, StoreError> {
        let objects = self.selected_integration(id, indices)?;
        let s = serde_json::to_string(&json!({"observation_id":id,"objects":objects}))
            .map_err(|_| StoreError::InvalidInput)?;
        if s.len() > MAX_INTEGRATION_WINDOW {
            return Err(StoreError::ResourceLimit);
        }
        self.integration_read_mut(id)?.preview = Some(indices.to_vec());
        Ok(s)
    }
    /// Saves exactly the previewed set in one transaction; any rejection rolls back every member.
    pub fn bind_integration_objects(
        &mut self,
        id: u64,
        indices: &[usize],
        at: i64,
    ) -> Result<IntegrationBindResult, StoreError> {
        let r = self.integration_fence(id)?;
        if r.preview.as_deref() != Some(indices) {
            return Err(StoreError::InvalidInput);
        }
        let task = r.task;
        let p = r.profile.clone();
        let target = r.target.clone();
        let objects = self.selected_integration(id, indices)?;
        let a = adapter(p.kind);
        self.change(|c| {
            let current = profile(c, p.id)?;
            enabled(c, current.kind.into())?;
            allowed(c, &current)?;
            a.admit(&current, &target)?;
            if current.version() != p.version() {
                return Err(StoreError::StaleRevision);
            }
            let mut replaced = Vec::new();
            for o in &objects {
                a.validate_object(o, false)?;
                let digest = o.key.digest()?;
                let prior: Option<(i64, Option<String>)> = c
                    .query_row(
                        "SELECT id,namespace FROM source_bindings WHERE task_id=?1 AND digest=?2",
                        params![task.task_id.0, digest.as_slice()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((binding, namespace)) = prior {
                    if namespace.is_none() {
                        return Err(StoreError::Deleted);
                    }
                    let old = metadata(c, binding)?;
                    if old.metadata.profile().id != p.id
                        || owned_evidence(c, old.evidence_id, task.task_id)?.kind
                            == AssertionKind::UserConfirmation
                    {
                        return Err(StoreError::InvalidInput);
                    }
                    replaced.push(binding);
                }
            }
            // Remove replaced metadata inside this transaction so byte checks count its final replacement once.
            for binding in replaced {
                c.execute(
                    "DELETE FROM integration_metadata WHERE binding_id=?1",
                    [binding],
                )?;
            }
            let mut owner = task;
            let mut ids = Vec::new();
            for o in &objects {
                let input = SourceEvidence {
                    text: o.text.clone(),
                    observed_at_utc: o.observed_at_utc,
                    completeness: Completeness::Partial,
                    kind: AssertionKind::SourceFact,
                };
                let evidence =
                    match write_source(c, owner, &o.key, Some(&o.version), &input, Some(at))? {
                        IngestOutcome::Applied(e) => e,
                        IngestOutcome::Forgotten => return Err(StoreError::Deleted),
                        IngestOutcome::UserProtected => return Err(StoreError::InvalidInput),
                    };
                let binding: i64 = c.query_row(
                    "SELECT id FROM source_bindings WHERE evidence_id=?1 AND namespace IS NOT NULL",
                    [evidence.0],
                    |r| r.get(0),
                )?;
                let data =
                    serde_json::to_string(&o.metadata).map_err(|_| StoreError::InvalidInput)?;
                c.execute(
                    "INSERT INTO integration_metadata(binding_id,data) VALUES(?1,?2)",
                    params![binding, data],
                )?;
                byte_room(c, 0)?;
                owner.revision = read_task(c, task.task_id)?.revision;
                ids.push(evidence);
            }
            Ok(IntegrationBindResult {
                task: owner,
                evidence_ids: ids,
            })
        })
    }
    pub fn integration_saved_metadata(
        &self,
        kind: IntegrationKind,
        task: TaskId,
        after: i64,
    ) -> Result<String, StoreError> {
        if after < 0 {
            return Err(StoreError::InvalidInput);
        }
        let tx = self.connection.unchecked_transaction()?;
        let task = read_task(&tx, task)?;
        let mut q=tx.prepare("SELECT b.id FROM source_bindings b JOIN integration_metadata m ON m.binding_id=b.id WHERE b.task_id=?1 AND b.id>?2 AND b.namespace=?3 ORDER BY b.id LIMIT 51")?;
        let ids = q
            .query_map(params![task.id.0, after, adapter(kind).namespace()], |r| {
                r.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut rows = Vec::new();
        let mut cursor = None;
        for b in ids {
            let m = metadata(&tx, b)?;
            // Native pages keep the adapter's previous row shape; the kind is the request's own.
            rows.push(json!({"binding_id":m.binding_id,"evidence_id":m.evidence_id,"metadata":m.metadata}));
            let s = serde_json::to_string(
                &json!({"task_revision":task.revision,"rows":rows,"next_after":b}),
            )
            .map_err(|_| StoreError::InvalidInput)?;
            if rows.len() > 50 || s.len() > MAX_PAGE_BYTES {
                rows.pop();
                cursor = rows.last().and_then(|m| m["binding_id"].as_i64());
                break;
            }
        }
        let s = serde_json::to_string(
            &json!({"task_revision":task.revision,"rows":rows,"next_after":cursor}),
        )
        .map_err(|_| StoreError::InvalidInput)?;
        if s.len() > MAX_PAGE_BYTES {
            return Err(StoreError::ResourceLimit);
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that never finishes: only the engine's request limit can end the loop.
    struct Endless;
    impl Reader for Endless {
        fn next(
            &mut self,
            _: bool,
            _: &mut IntegrationWindow,
        ) -> Result<IntegrationRequest, StoreError> {
            Ok(IntegrationRequest::Jira(JiraRequest {
                cloud_id: "Site".into(),
                issue: "OLD-1".into(),
                category: 0,
                start_at: 0,
                max_results: 50,
            }))
        }
        fn accept(&mut self, _: &IntegrationResponse, _: i64, _: &mut IntegrationWindow) -> Step {
            Step::Continue
        }
    }
    #[test]
    fn core_refuses_the_step_after_the_adapter_maximum() -> Result<(), StoreError> {
        let path = std::env::temp_dir().join(format!(
            "ambient-integration-limit-{}.sqlite3",
            std::process::id()
        ));
        let mut s = Store::open(&path)?;
        s.set_model_session(9, true, true)?;
        let a = s.prepare_check(true, ModelProfile::High)?;
        let flag = a.cancellation();
        let d = s.dispatch_check(a, 1)?;
        s.finish_check(
            d.id(),
            ModelCheckResult {
                outcome: CheckOutcome::Confirmed,
                actual: ActualConfig {
                    model: ActualModel::Luna,
                    effort: ActualEffort::High,
                    tier: ActualTier::Fast,
                },
                usage: None,
            },
        )?;
        s.release_check(&flag);
        let p = s.save_integration_profile(
            0,
            0,
            "Work",
            Some("Site"),
            &IntegrationConfig::Jira(JiraConfig::default()),
        )?;
        let t = s.create_task("Discuss")?;
        let op = s.begin_integration_read(
            TaskVersion {
                task_id: t.id,
                revision: t.revision,
            },
            p.version(),
            "OLD-1",
            100,
            1000,
        )?;
        s.start_integration_transport(op)?;
        s.integration_read_mut(op)?.reader = Box::new(Endless);
        let mut permitted = 0;
        let refusal = loop {
            match s.permit_integration_step(op, 100) {
                Ok(_) => permitted += 1,
                Err(e) => break e,
            }
            let response = IntegrationResponse {
                status: 200,
                body: Vec::new(),
                headers: IntegrationHeaders::default(),
            };
            s.accept_integration_response(op, response, 100)?;
        };
        assert_eq!(permitted, Jira.max_requests());
        assert_eq!(refusal, StoreError::ResourceLimit);
        s.finish_integration_transport(op)?;
        drop(s);
        let _ = std::fs::remove_file(path);
        Ok(())
    }
}
