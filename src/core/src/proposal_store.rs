use super::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeSet, sync::atomic::Ordering};

pub(super) struct WorkspaceBasis {
    pub task: TaskVersion,
    pub selected: Vec<EvidenceId>,
    pub sources: Vec<SourceSettingsFence>,
}
fn sql_revision(r: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value = r.get::<_, i64>(index)?;
    if value <= 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(value as u64)
}
fn encode<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: DeserializeOwned>(value: &str) -> Result<T, StoreError> {
    serde_json::from_str(value).map_err(|_| StoreError::UnsupportedSchema)
}
pub(super) fn workspace_revision(c: &Connection, task: TaskId) -> Result<u64, StoreError> {
    if task.0 <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let value: Option<i64> = c
        .query_row(
            "SELECT assistant_workspace_revision FROM tasks WHERE id=?1",
            [task.0],
            |r| r.get(0),
        )
        .optional()?;
    value
        .filter(|v| *v > 0)
        .map(|v| v as u64)
        .ok_or(StoreError::NotFound)
}
fn bump_workspace(c: &Connection, task: TaskId) -> Result<u64, StoreError> {
    let next = workspace_revision(c, task)?
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(StoreError::ResourceLimit)?;
    c.execute(
        "UPDATE tasks SET assistant_workspace_revision=?1 WHERE id=?2",
        params![next as i64, task.0],
    )?;
    Ok(next)
}
pub(super) fn workspace(
    c: &Connection,
    task: TaskId,
) -> Result<Option<WorkspaceBasis>, StoreError> {
    let raw: Option<(u64, String, String)> = c
        .query_row(
            "SELECT task_revision,selected,sources FROM assistant_workspaces WHERE task_id=?1",
            [task.0],
            |r| Ok((sql_revision(r, 0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    raw.map(|(revision, selected, sources)| {
        let selected: Vec<EvidenceId> = decode(&selected)?;
        let sources: Vec<SourceSettingsFence> = decode(&sources)?;
        if revision == 0
            || revision > i64::MAX as u64
            || selected.len() > 32
            || sources.len() > 32
            || selected.iter().any(|e| e.0 <= 0)
            || selected.iter().map(|e| e.0).collect::<BTreeSet<_>>().len() != selected.len()
        {
            return Err(StoreError::UnsupportedSchema);
        }
        Ok(WorkspaceBasis {
            task: TaskVersion {
                task_id: task,
                revision,
            },
            selected,
            sources,
        })
    })
    .transpose()
}
pub(super) fn basis_fence(c: &Connection, w: &WorkspaceBasis) -> Result<(), StoreError> {
    plan::open_fence(c, w.task.task_id, w.task.revision)?;
    for f in &w.sources {
        assistance::source_settings_fence(c, f)?;
    }
    for id in &w.selected {
        owned_evidence(c, *id, w.task.task_id)?;
    }
    Ok(())
}
pub(super) fn replies(c: &Connection, task: TaskId) -> Result<Vec<DialoguePair>, StoreError> {
    let mut q=c.prepare("SELECT id,proposal_id,content_revision,model_question,user_reply FROM assistant_replies WHERE task_id=?1 ORDER BY position LIMIT 33")?;
    let pairs = q
        .query_map([task.0], |r| {
            Ok(DialoguePair {
                id: r.get(0)?,
                proposal: ProposalTarget {
                    id: ProposalId(r.get(1)?),
                    content_revision: sql_revision(r, 2)?,
                },
                model_question: r.get(3)?,
                user_reply: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if pairs.len() > MAX_REPLY_PAIRS {
        return Err(StoreError::ResourceLimit);
    }
    Ok(pairs)
}
pub(super) fn task_proposals(c: &Connection, task: TaskId) -> Result<Vec<Proposal>, StoreError> {
    let w = workspace(c, task)?.ok_or(StoreError::StaleRevision)?;
    let stale = match basis_fence(c, &w) {
        Ok(()) => false,
        Err(
            StoreError::StaleRevision
            | StoreError::Deleted
            | StoreError::NotFound
            | StoreError::TaskCompleted,
        ) => true,
        Err(e) => return Err(e),
    };
    let revision = workspace_revision(c, task)?;
    let mut q=c.prepare("SELECT id,content_revision,kind,message,rationale,citations,step_id,visible FROM assistant_proposals WHERE task_id=?1 ORDER BY id LIMIT 17")?;
    let rows = q
        .query_map([task.0], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                sql_revision(r, 1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, bool>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > MAX_PROPOSALS_PER_TASK {
        return Err(StoreError::ResourceLimit);
    }
    rows.into_iter()
        .map(
            |(id, content_revision, kind, message, rationale, citations, step, visible)| {
                let kind = match kind {
                    1 => AssistanceKind::Suggestion,
                    2 => AssistanceKind::Clarification,
                    _ => return Err(StoreError::UnsupportedSchema),
                };
                let p = Proposal {
                    id: ProposalId(id),
                    task: w.task,
                    workspace_revision: revision,
                    content_revision,
                    kind,
                    message,
                    rationale,
                    citations: decode(&citations)?,
                    step_id: step.map(StepId),
                    visible,
                    stale,
                };
                if id <= 0
                    || content_revision == 0
                    || content_revision > i64::MAX as u64
                    || p.message.trim().is_empty()
                    || p.rationale.trim().is_empty()
                    || p.citations.len() > 32
                    || p.citations
                        .iter()
                        .map(ProposalCitationRef::key)
                        .collect::<BTreeSet<_>>()
                        .len()
                        != p.citations.len()
                {
                    return Err(StoreError::UnsupportedSchema);
                }
                Ok(p)
            },
        )
        .collect()
}
fn proposal(c: &Connection, target: ProposalTarget) -> Result<Proposal, StoreError> {
    if target.id.0 <= 0 || target.content_revision == 0 || target.content_revision > i64::MAX as u64
    {
        return Err(StoreError::InvalidInput);
    }
    let task: Option<i64> = c
        .query_row(
            "SELECT task_id FROM assistant_proposals WHERE id=?1",
            [target.id.0],
            |r| r.get(0),
        )
        .optional()?;
    task_proposals(c, TaskId(task.ok_or(StoreError::StaleRevision)?))?
        .into_iter()
        .find(|p| p.target() == target)
        .ok_or(StoreError::StaleRevision)
}
fn feed(c: &Connection) -> Result<ProposalFeed, StoreError> {
    let mut q = c.prepare("SELECT task_id FROM assistant_workspaces ORDER BY task_id LIMIT 129")?;
    let tasks = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if tasks.len() > MAX_PROPOSALS {
        return Err(StoreError::ResourceLimit);
    }
    let mut rows = Vec::new();
    for t in tasks {
        rows.extend(task_proposals(c, TaskId(t))?);
    }
    rows.sort_by_key(|p| p.id);
    if rows.len() > MAX_PROPOSALS {
        return Err(StoreError::ResourceLimit);
    }
    for row in &rows {
        if encode(row)?.len() > MAX_PROPOSAL_ROW {
            return Err(StoreError::ResourceLimit);
        }
    }
    let value = ProposalFeed { proposals: rows };
    if encode(&value)?.len() > MAX_PROPOSAL_FEED {
        return Err(StoreError::ResourceLimit);
    }
    Ok(value)
}
fn details(c: &Connection, target: ProposalTarget) -> Result<ProposalDetails, StoreError> {
    let p = proposal(c, target)?;
    let mut citations = Vec::new();
    if !p.stale {
        let w = workspace(c, p.task.task_id)?.ok_or(StoreError::StaleRevision)?;
        let mut s = assistance::snapshot(c, p.task.task_id, p.task.revision, &w.selected, 0, 0)?;
        if s.sources != w.sources {
            return Err(StoreError::UnsupportedSchema);
        }
        s.replies = replies(c, p.task.task_id)?;
        assistance::add_reply_citations(&mut s);
        for reference in &p.citations {
            let citation = s
                .citations
                .get(&reference.key())
                .filter(|c| &c.reference == reference)
                .ok_or(StoreError::UnsupportedSchema)?;
            citations.push(citation.clone());
        }
    }
    let value = ProposalDetails {
        proposal: p,
        citations,
    };
    if encode(&value)?.len() > MAX_ASSISTANCE_CONTEXT {
        return Err(StoreError::ResourceLimit);
    }
    Ok(value)
}
impl Store {
    pub fn assistant_workspace_revision(&self, task: TaskId) -> Result<u64, StoreError> {
        workspace_revision(&self.connection, task)
    }
    /// Read-only projections do not acquire transport, credentials, or a High permit.
    pub fn proposal_feed(&self) -> Result<ProposalFeed, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        feed(&tx)
    }
    pub fn proposal_feed_json(&self) -> Result<String, StoreError> {
        encode(&self.proposal_feed()?)
    }
    pub fn proposal_details(&self, target: ProposalTarget) -> Result<ProposalDetails, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        details(&tx, target)
    }
    pub fn proposal_details_json(&self, target: ProposalTarget) -> Result<String, StoreError> {
        encode(&self.proposal_details(target)?)
    }
    pub fn validate_proposal_action(
        &self,
        target: ProposalTarget,
    ) -> Result<AssistanceKind, StoreError> {
        // A reply to a card leads to a new assistance request, so it keeps the assistance gate.
        self.require_assistance()?;
        let tx = self.connection.unchecked_transaction()?;
        let p = details(&tx, target)?.proposal;
        if p.stale {
            return Err(StoreError::StaleRevision);
        }
        Ok(p.kind)
    }
    pub fn copy_proposal(&self, target: ProposalTarget) -> Result<String, StoreError> {
        self.require_card_action()?;
        let tx = self.connection.unchecked_transaction()?;
        let p = details(&tx, target)?.proposal;
        if p.stale {
            return Err(StoreError::StaleRevision);
        }
        Ok(p.message)
    }
    pub fn set_proposal_visibility(
        &mut self,
        target: ProposalTarget,
        visible: bool,
    ) -> Result<(), StoreError> {
        if visible {
            self.require_card_action()?;
        }
        self.change(|c| {
            let p = proposal(c, target)?;
            if visible && p.stale {
                return Err(StoreError::StaleRevision);
            }
            c.execute(
                "UPDATE assistant_proposals SET visible=?1 WHERE id=?2",
                params![visible, target.id.0],
            )?;
            Ok(())
        })
    }
    pub fn clear_discussion(&mut self, task: TaskVersion, revision: u64) -> Result<(), StoreError> {
        self.change(|c| {
            fence(c, task.task_id, task.revision)?;
            if workspace_revision(c, task.task_id)? != revision {
                return Err(StoreError::StaleRevision);
            }
            clear(c, task.task_id)?;
            Ok(())
        })
    }
}
fn clear(c: &Connection, task: TaskId) -> Result<(), StoreError> {
    bump_workspace(c, task)?;
    c.execute(
        "DELETE FROM assistant_workspaces WHERE task_id=?1",
        [task.0],
    )?;
    Ok(())
}
/// Erasure joins the original mutation's write transaction, including uncited selected evidence.
pub(super) fn clear_revoked(c: &Connection) -> Result<(), StoreError> {
    let mut q = c.prepare("SELECT task_id FROM assistant_workspaces ORDER BY task_id LIMIT 129")?;
    let tasks = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if tasks.len() > MAX_PROPOSALS {
        return Err(StoreError::ResourceLimit);
    }
    for raw in tasks {
        let id = TaskId(raw);
        let w = workspace(c, id)?.ok_or(StoreError::UnsupportedSchema)?;
        let task = match read_task(c, id) {
            Ok(t) => Some(t),
            Err(StoreError::Deleted | StoreError::NotFound) => None,
            Err(e) => return Err(e),
        };
        let mut removed = task
            .as_ref()
            .is_none_or(|t| t.lifecycle == TaskLifecycle::Completed);
        for e in &w.selected {
            match owned_evidence(c, *e, w.task.task_id) {
                Ok(_) => {}
                Err(StoreError::Deleted | StoreError::NotFound | StoreError::StaleRevision) => {
                    removed = true
                }
                Err(e) => return Err(e),
            }
        }
        for f in &w.sources {
            match assistance::source_authorization_fence(c, f) {
                Ok(()) => {}
                Err(StoreError::Deleted | StoreError::NotFound | StoreError::StaleRevision) => {
                    removed = true
                }
                Err(e) => return Err(e),
            }
        }
        if removed {
            clear(c, id)?;
        } else if task.is_some_and(|t| t.initiative_suppressed) {
            c.execute(
                "UPDATE assistant_proposals SET visible=0 WHERE task_id=?1",
                [raw],
            )?;
        }
    }
    Ok(())
}
/// Saves one cited tool result with `write_source(explicit = None)` and its plugin metadata,
/// threading the growing TaskVersion. Forgotten or user-edited keys reject the whole publication.
fn save_result(
    c: &Connection,
    owner: &mut TaskVersion,
    s: &AssistanceSnapshot,
    r: &ToolResult,
) -> Result<SourceSettingsFence, StoreError> {
    if !r.citable {
        return Err(StoreError::InvalidInput);
    }
    let pinned = s
        .plugins
        .tools
        .get(r.tool)
        .ok_or(StoreError::InvalidInput)?;
    let metadata = plugin_loop::result_metadata(pinned, r);
    let key = plugins::result_key(&metadata);
    let evidence = match write_source(
        c,
        *owner,
        &key,
        Some(&metadata.content_hash),
        &plugin_loop::result_evidence(r),
        None,
    )? {
        IngestOutcome::Applied(e) => e,
        IngestOutcome::Forgotten | IngestOutcome::UserProtected => {
            return Err(StoreError::StaleRevision);
        }
    };
    owner.revision = read_task(c, owner.task_id)?.revision;
    let binding: i64 = c.query_row(
        "SELECT id FROM source_bindings WHERE evidence_id=?1 AND namespace IS NOT NULL",
        [evidence.0],
        |r| r.get(0),
    )?;
    let data = encode(&metadata)?;
    c.execute(
        "DELETE FROM integration_metadata WHERE binding_id=?1",
        [binding],
    )?;
    c.execute(
        "INSERT INTO integration_metadata(binding_id,data) VALUES(?1,?2)",
        params![binding, data],
    )?;
    byte_room(c, 0)?;
    Ok(SourceSettingsFence::Plugin(Box::new(PluginSavedMetadata {
        binding_id: binding,
        evidence_id: evidence,
        metadata,
    })))
}
pub(super) fn publish(
    c: &mut Connection,
    s: &AssistanceSnapshot,
    a: &AssistanceAnswer,
    grounds: &[Vec<ResolvedGround>],
    tools: &[ToolResult],
) -> Result<WorkspaceVersion, StoreError> {
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // The write lock may have waited: recheck every original ground and the durable ABA fence here.
    assistance::snapshot_fence(&tx, s)?;
    if pause(&tx)? {
        return Err(StoreError::Paused);
    }
    let task = s.preview.task.task_id;
    let previous = if s.preview.mode == AssistanceMode::Continue {
        task_proposals(&tx, task)?
    } else {
        Vec::new()
    };
    if s.preview.mode == AssistanceMode::NewContext {
        tx.execute(
            "DELETE FROM assistant_workspaces WHERE task_id=?1",
            [task.0],
        )?;
    }
    // Cited tool results become saved grounds of this workspace before any proposal is written.
    let mut owner = s.preview.task;
    let mut selected = s.selected.clone();
    let mut sources = s.sources.clone();
    let mut saved = std::collections::BTreeMap::new();
    for g in grounds.iter().flatten() {
        if let ResolvedGround::Tool(n) = g
            && !saved.contains_key(n)
        {
            let fence = save_result(
                &tx,
                &mut owner,
                s,
                tools.get(*n).ok_or(StoreError::InvalidInput)?,
            )?;
            let SourceSettingsFence::Plugin(m) = &fence else {
                return Err(StoreError::InvalidInput);
            };
            let evidence = m.evidence_id;
            if let Some(old) = sources
                .iter_mut()
                .find(|f| matches!(f, SourceSettingsFence::Plugin(o) if o.evidence_id == evidence))
            {
                *old = fence;
            } else {
                selected.push(evidence);
                sources.push(fence);
            }
            saved.insert(*n, evidence);
        }
    }
    if selected.len() > 32 {
        return Err(StoreError::ResourceLimit);
    }
    let selected_json = encode(&selected)?;
    let sources_json = encode(&sources)?;
    if selected_json.len() > 1024 || sources_json.len() > MAX_ASSISTANCE_CONTEXT {
        return Err(StoreError::ResourceLimit);
    }
    tx.execute("INSERT INTO assistant_workspaces VALUES(?1,?2,?3,?4) ON CONFLICT(task_id) DO UPDATE SET task_revision=excluded.task_revision,selected=excluded.selected,sources=excluded.sources",params![task.0,owner.revision as i64,selected_json,sources_json])?;
    let sequence: i64 = tx.query_row(
        "SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name='assistant_proposals'),0)",
        [],
        |r| r.get(0),
    )?;
    let additions = a
        .proposals
        .iter()
        .filter(|p| p.proposal_id.is_none())
        .count();
    if sequence < 0 || sequence.checked_add(additions as i64).is_none() {
        return Err(StoreError::ResourceLimit);
    }
    let mut retained = BTreeSet::new();
    for (answer, grounds) in a.proposals.iter().zip(grounds) {
        let refs = grounds
            .iter()
            .map(|g| match g {
                ResolvedGround::Stored(r) => Ok(r.clone()),
                ResolvedGround::Tool(n) => saved
                    .get(n)
                    .map(|e| ProposalCitationRef::Source(*e))
                    .ok_or(StoreError::InvalidInput),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let citations = encode(&refs)?;
        if citations.len() > 8192 {
            return Err(StoreError::ResourceLimit);
        }
        if let Some(id) = answer.proposal_id {
            let old = previous
                .iter()
                .find(|p| p.id == id)
                .ok_or(StoreError::InvalidInput)?;
            let changed = old.kind != answer.kind
                || old.message != answer.message
                || old.rationale != answer.rationale
                || old.citations != refs
                || old.step_id != answer.step_id;
            let revision = old
                .content_revision
                .checked_add(u64::from(changed))
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or(StoreError::ResourceLimit)?;
            // Visibility is read from this transaction, so Close during POST survives the content update.
            tx.execute("UPDATE assistant_proposals SET content_revision=?1,kind=?2,message=?3,rationale=?4,citations=?5,step_id=?6 WHERE id=?7 AND task_id=?8",params![revision as i64,answer.kind as u32,answer.message,answer.rationale,citations,answer.step_id.map(|v|v.0),id.0,task.0])?;
            retained.insert(id);
        } else {
            tx.execute("INSERT INTO assistant_proposals(task_id,content_revision,kind,message,rationale,citations,step_id,visible) VALUES(?1,1,?2,?3,?4,?5,?6,1)",params![task.0,answer.kind as u32,answer.message,answer.rationale,citations,answer.step_id.map(|v|v.0)])?;
            retained.insert(ProposalId(tx.last_insert_rowid()));
        }
    }
    for old in previous {
        if !retained.contains(&old.id) {
            tx.execute("DELETE FROM assistant_proposals WHERE id=?1", [old.id.0])?;
        }
    }
    tx.execute("DELETE FROM assistant_replies WHERE task_id=?1", [task.0])?;
    for (index, pair) in s.replies.iter().enumerate() {
        tx.execute(
            "INSERT INTO assistant_replies VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                task.0,
                index as i64,
                pair.id,
                pair.proposal.id.0,
                pair.proposal.content_revision as i64,
                pair.model_question,
                pair.user_reply
            ],
        )?;
    }
    let revision = bump_workspace(&tx, task)?;
    if a.proposals.is_empty() {
        tx.execute(
            "DELETE FROM assistant_workspaces WHERE task_id=?1",
            [task.0],
        )?;
    }
    byte_room(&tx, 0)?;
    let validated = feed(&tx)?;
    for p in validated
        .proposals
        .iter()
        .filter(|p| p.task.task_id == task)
    {
        details(&tx, p.target())?;
    }
    if s.cancelled.load(Ordering::Acquire) {
        return Err(StoreError::Cancelled);
    }
    tx.commit()?;
    Ok(WorkspaceVersion {
        task: owner,
        workspace_revision: revision,
    })
}
pub(super) fn validate_workspaces(c: &Connection) -> Result<(), StoreError> {
    let all = feed(c)?;
    let max: i64 = c.query_row(
        "SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name='assistant_proposals'),0)",
        [],
        |r| r.get(0),
    )?;
    let mut q = c.prepare("SELECT task_id FROM assistant_workspaces ORDER BY task_id")?;
    let tasks = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for raw in tasks {
        let id = TaskId(raw);
        let w = workspace(c, id)?.ok_or(StoreError::UnsupportedSchema)?;
        let t = read_task(c, id)?;
        let revision = workspace_revision(c, id)?;
        let rows = all
            .proposals
            .iter()
            .filter(|p| p.task.task_id == id)
            .collect::<Vec<_>>();
        if t.lifecycle != TaskLifecycle::Open
            || w.task.revision > t.revision
            || revision < 2
            || rows.is_empty()
        {
            return Err(StoreError::UnsupportedSchema);
        }
        for f in &w.sources {
            assistance::source_authorization_fence(c, f)?;
        }
        let mut expected_sources = Vec::new();
        for e in &w.selected {
            let e = owned_evidence(c, *e, id)?;
            if let Origin::External { key, .. } = &e.origin {
                expected_sources.push(
                    if IntegrationKind::from_namespace(&key.namespace).is_some() {
                        SourceSettingsFence::Integration(Box::new(integrations::stored_metadata(
                            c, e.id,
                        )?))
                    } else if key.namespace == PLUGIN_NAMESPACE {
                        SourceSettingsFence::Plugin(Box::new(plugins::stored_metadata(c, e.id)?))
                    } else if key.namespace == CODEX_NAMESPACE {
                        SourceSettingsFence::Codex(codex::evidence_metadata(c, e.id)?)
                    } else {
                        let root = sources::evidence_root(c, key)?;
                        SourceSettingsFence::Git {
                            path: root.path,
                            revision: root.revision,
                        }
                    },
                );
            }
        }
        if expected_sources.len() != w.sources.len() {
            return Err(StoreError::UnsupportedSchema);
        }
        for (saved, current) in w.sources.iter().zip(&expected_sources) {
            if w.task.revision == t.revision {
                if saved != current {
                    return Err(StoreError::UnsupportedSchema);
                }
                continue;
            }
            match (saved, current) {
                (SourceSettingsFence::Integration(old), SourceSettingsFence::Integration(now)) => {
                    integrations::same_ground(c, old, now)?;
                }
                (SourceSettingsFence::Plugin(old), SourceSettingsFence::Plugin(now)) => {
                    plugins::same_ground(c, old, now)?;
                }
                (SourceSettingsFence::Codex(old), SourceSettingsFence::Codex(now)) => {
                    // A newer ordinal/content is ordinary evidence. Keep the old typed identity, not its text.
                    old.identity.validate()?;
                    let e = owned_evidence(c, old.evidence_id, id)?;
                    let Origin::External { key, .. } = e.origin else {
                        return Err(StoreError::UnsupportedSchema);
                    };
                    if old.binding_id != now.binding_id
                        || old.evidence_id != now.evidence_id
                        || old.selection_id != now.selection_id
                        || old.settings_revision != now.settings_revision
                        || old.accepted_ordinal > now.accepted_ordinal
                        || old.identity.source_key(&key.scope) != key
                    {
                        return Err(StoreError::UnsupportedSchema);
                    }
                }
                _ if saved == current => {}
                _ => return Err(StoreError::UnsupportedSchema),
            }
        }
        let pairs = replies(c, id)?;
        for (index, p) in pairs.iter().enumerate() {
            let parts = p.id.split(':').collect::<Vec<_>>();
            let old = parts
                .get(2)
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|r| *r > 0 && *r < revision)
                .ok_or(StoreError::UnsupportedSchema)?;
            if p.id != format!("reply:{raw}:{old}:{index}")
                || p.proposal.id.0 <= 0
                || p.proposal.id.0 > max
                || p.proposal.content_revision == 0
                || p.proposal.content_revision > i64::MAX as u64
                || p.model_question.trim().is_empty()
                || p.user_reply.trim().is_empty()
            {
                return Err(StoreError::UnsupportedSchema);
            }
        }
        for p in rows {
            if p.id.0 > max {
                return Err(StoreError::UnsupportedSchema);
            }
            for r in &p.citations {
                match r {
                    ProposalCitationRef::Task(v) if *v == id => {}
                    ProposalCitationRef::Note(v) | ProposalCitationRef::Source(v) => {
                        if !w.selected.contains(v) {
                            return Err(StoreError::UnsupportedSchema);
                        }
                        let e = owned_evidence(c, *v, id)?;
                        if matches!(r, ProposalCitationRef::Note(_))
                            != matches!(e.origin, Origin::User)
                        {
                            return Err(StoreError::UnsupportedSchema);
                        }
                    }
                    ProposalCitationRef::Step(v) => {
                        let owned: bool = c.query_row(
                            "SELECT EXISTS(SELECT 1 FROM plan_steps WHERE id=?1 AND task_id=?2)",
                            params![v.0, raw],
                            |r| r.get(0),
                        )?;
                        if !owned {
                            return Err(StoreError::UnsupportedSchema);
                        }
                    }
                    ProposalCitationRef::Check(v) => {
                        let owned: bool = c.query_row(
                            "SELECT EXISTS(SELECT 1 FROM plan_checks WHERE id=?1 AND task_id=?2)",
                            params![v.0, raw],
                            |r| r.get(0),
                        )?;
                        if !owned {
                            return Err(StoreError::UnsupportedSchema);
                        }
                    }
                    ProposalCitationRef::Reply(v) if pairs.iter().any(|p| &p.id == v) => {}
                    _ => return Err(StoreError::UnsupportedSchema),
                }
            }
            details(c, p.target())?;
        }
    }
    Ok(())
}
