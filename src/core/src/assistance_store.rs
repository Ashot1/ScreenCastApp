use super::*;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
impl Store {
    pub fn set_model_session(
        &mut self,
        epoch: u64,
        valid: bool,
        reported: bool,
    ) -> Result<(), StoreError> {
        if epoch == 0 {
            return Err(StoreError::InvalidInput);
        }
        if self.model_session.epoch != epoch {
            self.reset_observation_backoff();
        }
        if self.model_session.epoch != epoch || !valid || !reported {
            self.cancel_all_reads();
            self.high_capability = None;
            self.cancel_check();
            self.clear_assistance();
        }
        self.model_session = ModelSession {
            epoch,
            valid,
            reported,
        };
        Ok(())
    }
    fn current_high_capability(&self) -> bool {
        self.high_capability
            .as_ref()
            .is_some_and(|(epoch, cancelled)| {
                *epoch == self.model_session.epoch && !cancelled.load(Ordering::Acquire)
            })
    }
    pub fn assistance_available(&self) -> Result<bool, StoreError> {
        Ok(self.model_session.valid
            && self.model_session.reported
            && self.current_high_capability()
            && !self.manual_pause()?)
    }
    pub(super) fn require_assistance(&self) -> Result<(), StoreError> {
        if self.manual_pause()? {
            return Err(StoreError::Paused);
        }
        if !self.model_session.valid {
            return Err(StoreError::SessionRequired);
        }
        if !self.model_session.reported {
            return Err(StoreError::ReportRequired);
        }
        if !self.current_high_capability() {
            return Err(StoreError::CapabilityRequired);
        }
        Ok(())
    }
    /// Card actions are local: copy and show need a valid session and no pause, not the
    /// connection check or the credit acknowledgement (ADR-025 «Карточки»).
    pub(super) fn require_card_action(&self) -> Result<(), StoreError> {
        if self.manual_pause()? {
            return Err(StoreError::Paused);
        }
        if !self.model_session.valid {
            return Err(StoreError::SessionRequired);
        }
        Ok(())
    }
    /// Clears only the process permit/preview. Durable discussion has a separate explicit operation.
    pub fn clear_assistance(&mut self) {
        if let Some(a) = &self.active_check
            && a.purpose == ModelPurpose::Assistance
        {
            a.cancelled.store(true, Ordering::Release);
        }
        if let Some(p) = self.prepared.take() {
            p.cancelled.store(true, Ordering::Release);
        }
    }
    pub fn discard_assistance_preview(&mut self, request: u64) -> Result<(), StoreError> {
        if request == 0 {
            return Err(StoreError::InvalidInput);
        }
        if self
            .prepared
            .as_ref()
            .is_some_and(|s| s.preview.request_id == request)
        {
            if self
                .active_check
                .as_ref()
                .is_some_and(|a| a.request_id == Some(request))
            {
                return Err(StoreError::Busy);
            }
            self.clear_assistance();
        }
        Ok(())
    }
    pub(super) fn invalidate_stale_assistance(&mut self) {
        if self
            .prepared
            .as_ref()
            .is_some_and(|p| snapshot_fence(&self.connection, p).is_err())
        {
            self.clear_assistance();
        }
    }
    pub(super) fn current_snapshot(
        &self,
        request: u64,
    ) -> Result<Arc<AssistanceSnapshot>, StoreError> {
        let s = self
            .prepared
            .as_ref()
            .filter(|p| p.preview.request_id == request)
            .ok_or(StoreError::StaleRevision)?;
        snapshot_fence(&self.connection, s)?;
        if s.epoch != self.model_session.epoch {
            return Err(StoreError::StaleRevision);
        }
        Ok(s.clone())
    }
    pub fn assistance_preview(&self, request: u64) -> Result<AssistancePreview, StoreError> {
        self.require_assistance()?;
        Ok(self.current_snapshot(request)?.preview.clone())
    }
    pub fn prepare_assistance(
        &mut self,
        input: AssistanceRequest,
    ) -> Result<AssistancePreview, StoreError> {
        if self.active_check.is_some() {
            return Err(StoreError::Busy);
        }
        self.require_assistance()?;
        self.clear_assistance();
        if input.selected_evidence.len() > 32 {
            return Err(StoreError::ResourceLimit);
        }
        let request = self.next_request;
        self.next_request = request.checked_add(1).ok_or(StoreError::ResourceLimit)?;
        let tx = self.connection.unchecked_transaction()?;
        if proposals::workspace_revision(&tx, input.task.task_id)? != input.workspace_revision {
            return Err(StoreError::StaleRevision);
        }
        let (notes, stored, replies, previous) = match input.mode {
            AssistanceMode::NewContext => {
                if input.reply.is_some() {
                    return Err(StoreError::InvalidInput);
                }
                (input.selected_evidence, None, Vec::new(), Vec::new())
            }
            AssistanceMode::Continue => {
                if !input.selected_evidence.is_empty() {
                    return Err(StoreError::InvalidInput);
                }
                let w = proposals::workspace(&tx, input.task.task_id)?
                    .ok_or(StoreError::StaleRevision)?;
                if w.task != input.task {
                    return Err(StoreError::StaleRevision);
                }
                proposals::basis_fence(&tx, &w)?;
                let pairs = proposals::replies(&tx, input.task.task_id)?;
                let rows = proposals::task_proposals(&tx, input.task.task_id)?;
                (w.selected.clone(), Some(w), pairs, rows)
            }
        };
        let mut s = snapshot(
            &tx,
            input.task.task_id,
            input.task.revision,
            &notes,
            request,
            self.model_session.epoch,
        )?;
        if let Some(w) = stored
            && w.sources != s.sources
        {
            return Err(StoreError::StaleRevision);
        }
        s.preview.mode = input.mode;
        s.proposals = previous;
        s.replies = replies;
        if let Some(reply) = input.reply {
            text_field(&reply.text)?;
            if reply.text.trim().is_empty() {
                return Err(StoreError::InvalidInput);
            }
            if s.replies.len() >= MAX_REPLY_PAIRS {
                return Err(StoreError::ResourceLimit);
            }
            let p = s
                .proposals
                .iter()
                .find(|p| {
                    p.target() == reply.target
                        && !p.stale
                        && p.kind == AssistanceKind::Clarification
                })
                .ok_or(StoreError::StaleRevision)?;
            s.replies.push(DialoguePair {
                id: format!(
                    "reply:{}:{}:{}",
                    input.task.task_id.0,
                    input.workspace_revision,
                    s.replies.len()
                ),
                proposal: reply.target,
                model_question: p.message.clone(),
                user_reply: reply.text,
            });
        }
        add_reply_citations(&mut s);
        let mut context: serde_json::Value =
            serde_json::from_str(&s.preview.context).map_err(|_| StoreError::InvalidInput)?;
        context["mode"] = json!(input.mode);
        context["workspace_revision"] = json!(input.workspace_revision);
        if input.mode == AssistanceMode::Continue {
            context["proposals"]=json!(s.proposals.iter().map(|p|json!({"proposal_id":p.id,"content_revision":p.content_revision,"kind":p.kind,"message":p.message,"rationale":p.rationale,"evidence_ids":p.citations.iter().map(ProposalCitationRef::key).collect::<Vec<_>>(),"step_id":p.step_id.map(|id|format!("step:{}",id.0)),"visible":p.visible})).collect::<Vec<_>>());
            context["replies"]=json!(s.replies.iter().map(|p|json!({"evidence_id":p.id,"proposal_id":p.proposal.id,"content_revision":p.proposal.content_revision,"model_question":p.model_question,"user_reply":p.user_reply})).collect::<Vec<_>>());
        }
        // Only prepare offers plugin tools; without an enabled plugin the context is unchanged.
        if let Some((offer, report)) =
            plugins::offer(&tx, s.selected.len(), context.to_string().len())?
        {
            context["plugins"] = report;
            s.plugins = offer;
        }
        s.preview.context = context.to_string();
        if s.preview.context.len() > MAX_ASSISTANCE_CONTEXT {
            return Err(StoreError::TextTooLong);
        }
        drop(tx);
        let preview = s.preview.clone();
        self.prepared = Some(Arc::new(s));
        Ok(preview)
    }
    pub fn begin_assistance(&mut self, request: u64) -> Result<CheckAdmission, StoreError> {
        self.require_assistance()?;
        if self.active_check.is_some() {
            return Err(StoreError::Busy);
        }
        self.current_snapshot(request)?;
        if request <= self.consumed_request {
            return Err(StoreError::StaleRevision);
        }
        let admission =
            self.reserve_model(ModelPurpose::Assistance, ModelProfile::High, Some(request))?;
        self.consumed_request = request;
        Ok(admission)
    }
    pub fn finish_assistance(
        &mut self,
        id: AttemptId,
        result: ModelCheckResult,
        answer: Option<AssistanceAnswer>,
    ) -> Result<Option<WorkspaceVersion>, StoreError> {
        self.finish_assistance_with(id, result, answer, &[])
    }
    /// Accounts the final POST, then publishes the full set; cited tool results of this request
    /// are saved inside the same publication transaction.
    pub(super) fn finish_assistance_with(
        &mut self,
        id: AttemptId,
        mut result: ModelCheckResult,
        answer: Option<AssistanceAnswer>,
        tools: &[ToolResult],
    ) -> Result<Option<WorkspaceVersion>, StoreError> {
        let confirmed = result.outcome == CheckOutcome::Confirmed;
        let validated = (|| {
            self.require_assistance()?;
            let active = self
                .active_check
                .as_ref()
                .filter(|a| a.attempt == Some(id) && a.purpose == ModelPurpose::Assistance)
                .ok_or(StoreError::InvalidInput)?;
            if active.cancelled.load(Ordering::Acquire) {
                return Err(StoreError::Cancelled);
            }
            let s = self.current_snapshot(active.request_id.ok_or(StoreError::InvalidInput)?)?;
            let answer = answer.ok_or(StoreError::InvalidInput)?;
            if answer.proposals.len() > MAX_PROPOSALS_PER_TASK
                || answer.rationale.trim().is_empty()
                || answer.rationale.len() > 8192
            {
                return Err(StoreError::InvalidInput);
            }
            let mut seen = BTreeSet::new();
            let mut grounds = Vec::new();
            for p in &answer.proposals {
                if let Some(id) = p.proposal_id
                    && (!seen.insert(id) || !s.proposals.iter().any(|p| p.id == id))
                {
                    return Err(StoreError::InvalidInput);
                }
                grounds.push(validate_proposal(&s, p, tools)?);
            }
            Ok((s, answer, grounds))
        })();
        if confirmed && let Err(e) = &validated {
            result.outcome = if matches!(
                e,
                StoreError::InvalidInput | StoreError::TextTooLong | StoreError::ResourceLimit
            ) {
                CheckOutcome::Invalid
            } else {
                CheckOutcome::Cancelled
            };
        }
        let result = self.finish_check(id, result)?;
        // Usage is durable before publication. A rejected full set never rolls back the attempt.
        if confirmed && result.outcome == CheckOutcome::Invalid {
            return Err(validated.err().unwrap_or(StoreError::InvalidInput));
        }
        if result.outcome != CheckOutcome::Confirmed || !result.actual.confirmed(ModelProfile::High)
        {
            return Ok(None);
        }
        let (s, answer, grounds) = validated?;
        self.require_assistance()?;
        let version = proposals::publish(&mut self.connection, &s, &answer, &grounds, tools)?;
        Ok(Some(version))
    }
}
pub(super) fn snapshot_fence(c: &Connection, s: &AssistanceSnapshot) -> Result<(), StoreError> {
    if s.cancelled.load(Ordering::Acquire) {
        return Err(StoreError::Cancelled);
    }
    plan::open_fence(c, s.preview.task.task_id, s.preview.task.revision)?;
    if proposals::workspace_revision(c, s.preview.task.task_id)? != s.preview.workspace_revision {
        return Err(StoreError::StaleRevision);
    }
    for source in &s.sources {
        source_settings_fence(c, source)?;
    }
    // Disable, key revocation, unapproval and re-pinning all move the plugin revision.
    for v in &s.plugins.versions {
        plugins::at_revision(c, *v)?;
    }
    Ok(())
}
pub(super) fn add_reply_citations(s: &mut AssistanceSnapshot) {
    for p in &s.replies {
        s.citations.insert(
            p.id.clone(),
            AssistanceCitation {
                reference: ProposalCitationRef::Reply(p.id.clone()),
                text: p.user_reply.clone(),
                model_question: Some(p.model_question.clone()),
                integration: None,
                plugin: None,
                source_observed_at_utc: None,
            },
        );
    }
}
fn citation(
    citations: &mut BTreeMap<String, AssistanceCitation>,
    reference: ProposalCitationRef,
    text: &str,
) -> String {
    let key = reference.key();
    citations.insert(
        key.clone(),
        AssistanceCitation {
            reference,
            text: text.to_owned(),
            model_question: None,
            integration: None,
            plugin: None,
            source_observed_at_utc: None,
        },
    );
    key
}
pub(super) fn snapshot(
    c: &Connection,
    id: TaskId,
    expected: u64,
    notes: &[EvidenceId],
    request: u64,
    epoch: u64,
) -> Result<AssistanceSnapshot, StoreError> {
    let tx = c;
    let task = plan::open_fence(tx, id, expected)?;
    let mut citations = BTreeMap::new();
    let task_key = citation(&mut citations, ProposalCitationRef::Task(id), &task.text);
    let mut selected = BTreeSet::new();
    let mut note_json = Vec::new();
    let mut source_json = Vec::new();
    let mut source_fences = Vec::new();
    for note in notes {
        if !selected.insert(note.0) {
            return Err(StoreError::InvalidInput);
        }
        let e = owned_evidence(tx, *note, id)?;
        match &e.origin {
            Origin::User => {
                let key = citation(&mut citations, ProposalCitationRef::Note(e.id), &e.text);
                note_json.push(json!({"evidence_id":key,"id":e.id.0,"text":e.text,"origin":"user","observed_at_utc":e.observed_at_utc,"completeness":e.completeness as u32,"assertion_kind":e.kind as u32}));
            }
            Origin::External { key, version } => {
                let citation = citation(&mut citations, ProposalCitationRef::Source(e.id), &e.text);
                let mut source_details = if let Some(kind) =
                    IntegrationKind::from_namespace(&key.namespace)
                {
                    let m = integrations::stored_metadata(tx, e.id)?;
                    integrations::check_saved(tx, &m)?;
                    // The model sees the adapter detail under its kind name (`github`, `jira`).
                    let detail = json!({kind.as_str():m.metadata,"coverage":"selected_objects","observation":"non_atomic_observation","current_state":"current_state_unverified"});
                    let c = citations
                        .get_mut(&citation)
                        .ok_or(StoreError::InvalidInput)?;
                    c.integration = Some(CitationIntegration {
                        kind,
                        metadata: m.metadata.clone(),
                    });
                    c.source_observed_at_utc = e.observed_at_utc;
                    source_fences.push(SourceSettingsFence::Integration(Box::new(m)));
                    detail
                } else if key.namespace == PLUGIN_NAMESPACE {
                    let m = plugins::stored_metadata(tx, e.id)?;
                    plugins::check_saved(tx, &m)?;
                    let r = &m.metadata;
                    let detail = json!({"plugin":{"plugin":r.plugin_name,"version":r.plugin_version,"server":r.server,"tool":r.tool,"arguments":r.arguments,"called_at_utc":r.called_at_utc,"complete":r.complete},"coverage":"tool_result","observation":"saved_tool_result","current_state":"current_state_unverified"});
                    let c = citations
                        .get_mut(&citation)
                        .ok_or(StoreError::InvalidInput)?;
                    c.plugin = Some(CitationPlugin {
                        plugin: r.plugin_name.clone(),
                        server: r.server.clone(),
                        tool: r.tool.clone(),
                        called_at_utc: r.called_at_utc,
                        complete: r.complete,
                    });
                    c.source_observed_at_utc = e.observed_at_utc;
                    source_fences.push(SourceSettingsFence::Plugin(Box::new(m)));
                    detail
                } else if key.namespace == CODEX_NAMESPACE {
                    let m = codex::evidence_metadata(tx, e.id)?;
                    let provenance = match m.identity.kind() {
                        CodexEventKind::User => "human_unverified",
                        CodexEventKind::DelegatedCreate | CodexEventKind::DelegatedSend => {
                            "delegated"
                        }
                        CodexEventKind::Agent | CodexEventKind::Plan => "agent",
                        _ => "structured_observation",
                    };
                    let detail = json!({"coverage":"local_records_only/local_window","provenance":provenance,"event_kind":m.identity.kind() as u32,"turn_id":m.identity.turn_id(),"item_id":m.identity.item_id(),"selection_id":m.selection_id,"settings_revision":m.settings_revision,"accepted_ordinal":m.accepted_ordinal,"app_projection":m.identity.item_id().is_none(),"task_completion_unverified":true,"plan_unconfirmed":true});
                    source_fences.push(SourceSettingsFence::Codex(m));
                    detail
                } else {
                    let root = sources::evidence_root(tx, key)?;
                    source_fences.push(SourceSettingsFence::Git {
                        path: root.path.clone(),
                        revision: root.revision,
                    });
                    json!({"root_path":root.path})
                };
                let fields = json!({"evidence_id":citation,"id":e.id.0,"text":e.text,"source":{"namespace":key.namespace,"scope":key.scope,"object_id":key.object_id,"version":version},"observed_at_utc":e.observed_at_utc,"completeness":e.completeness as u32,"assertion_kind":e.kind as u32,"historical_observation":true});
                source_details
                    .as_object_mut()
                    .ok_or(StoreError::InvalidInput)?
                    .extend(fields.as_object().ok_or(StoreError::InvalidInput)?.clone());
                source_json.push(source_details);
            }
        }
    }
    // A disabled integration's saved facts cannot enter a new preview.
    for f in &source_fences {
        switch_fence(tx, f)?;
    }
    let mut q=tx.prepare("SELECT id,description,prompt,reference,completed_at_utc FROM plan_steps WHERE task_id=?1 AND description IS NOT NULL ORDER BY id LIMIT 65")?;
    let mut steps = q
        .query_map([id.0], |r| {
            Ok(PlanStep {
                id: StepId(r.get(0)?),
                task_id: id,
                description: r.get(1)?,
                prompt: r.get(2)?,
                reference: r.get(3)?,
                completed_at_utc: r.get(4)?,
                ready: false,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if steps.len() > 64 {
        return Err(StoreError::ResourceLimit);
    }
    let mut step_json = Vec::new();
    let mut checks = Vec::new();
    for step in &mut steps {
        step.ready = task.plan_completeness == Completeness::Complete
            && step.completed_at_utc.is_none()
            && plan::predecessors_done(tx, id, step.id)?;
        let key = citation(
            &mut citations,
            ProposalCitationRef::Step(step.id),
            &step.description,
        );
        step_json.push(json!({"evidence_id":key,"id":step.id.0,"description":step.description,"prompt":step.prompt,"reference":step.reference,"completed_at_utc":step.completed_at_utc,"ready":step.ready}));
        let mut q=tx.prepare("SELECT id,description,result,confirmed_at_utc FROM plan_checks WHERE step_id=?1 AND description IS NOT NULL ORDER BY id LIMIT 9")?;
        let rows = q
            .query_map([step.id.0], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() > 8 {
            return Err(StoreError::ResourceLimit);
        }
        for (id, text, result, confirmed) in rows {
            let key = citation(
                &mut citations,
                ProposalCitationRef::Check(CheckId(id)),
                &text,
            );
            checks.push(json!({"evidence_id":key,"id":id,"step_id":step.id.0,"description":text,"result":result,"confirmed_at_utc":confirmed}));
        }
    }
    let mut q=tx.prepare("SELECT id,predecessor,dependent FROM plan_dependencies WHERE task_id=?1 AND predecessor IS NOT NULL ORDER BY id LIMIT 513")?;
    let dependencies=q.query_map([id.0],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"predecessor":r.get::<_,i64>(1)?,"dependent":r.get::<_,i64>(2)?})))?.collect::<Result<Vec<_>,_>>()?;
    if dependencies.len() > 512 {
        return Err(StoreError::ResourceLimit);
    }
    let mut q=tx.prepare("SELECT id,text FROM useful_references WHERE task_id=?1 AND text IS NOT NULL ORDER BY id LIMIT 33")?;
    let references = q
        .query_map([id.0], |r| {
            Ok(json!({"id":r.get::<_,i64>(0)?,"text":r.get::<_,String>(1)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if references.len() > 32 {
        return Err(StoreError::ResourceLimit);
    }
    let context=json!({"task":{"id":id.0,"revision":expected,"evidence_id":task_key,"text":task.text,"lifecycle":"open","plan_completeness":task.plan_completeness as u32},"notes":note_json,"sources":source_json,"steps":step_json,"checks":checks,"dependencies":dependencies,"useful_references":references}).to_string();
    if context.len() > MAX_ASSISTANCE_CONTEXT {
        return Err(StoreError::TextTooLong);
    }
    Ok(AssistanceSnapshot {
        cancelled: Arc::new(AtomicBool::new(false)),
        preview: AssistancePreview {
            request_id: request,
            workspace_revision: proposals::workspace_revision(c, id)?,
            mode: AssistanceMode::NewContext,
            task: TaskVersion {
                task_id: id,
                revision: expected,
            },
            context,
        },
        epoch,
        task,
        steps,
        citations,
        sources: source_fences,
        selected: notes.to_vec(),
        proposals: Vec::new(),
        replies: Vec::new(),
        plugins: PluginOffer::default(),
    })
}
/// `tool:N` exactly like `step:N`: decimal without leading zeros, numbering this request's calls.
fn tool_ground(id: &str, tools: &[ToolResult]) -> Option<usize> {
    let n = id.strip_prefix("tool:")?;
    if n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = n.parse::<usize>().ok()?.checked_sub(1)?;
    tools.get(n).filter(|t| t.citable).map(|_| n)
}
fn validate_proposal(
    snapshot: &AssistanceSnapshot,
    a: &ProposalAnswer,
    tools: &[ToolResult],
) -> Result<Vec<ResolvedGround>, StoreError> {
    if a.rationale.trim().is_empty() || a.rationale.len() > 8 * 1024 || a.evidence_ids.len() > 32 {
        return Err(StoreError::InvalidInput);
    }
    if a.message.trim().is_empty() || a.message.len() > MAX_REPLY_TEXT {
        return Err(StoreError::InvalidInput);
    }
    if a.kind == AssistanceKind::Suggestion
        && (a.evidence_ids.is_empty()
            || snapshot.task.plan_completeness == Completeness::Partial
            || (!snapshot.steps.is_empty()
                && (snapshot.task.plan_completeness != Completeness::Complete
                    || a.step_id.is_none())))
    {
        return Err(StoreError::InvalidInput);
    }
    if let Some(id) = a.step_id
        && !snapshot.steps.iter().any(|s| s.id == id && s.ready)
    {
        return Err(StoreError::InvalidInput);
    }
    let mut seen = BTreeSet::new();
    a.evidence_ids
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(StoreError::InvalidInput);
            }
            match snapshot.citations.get(id) {
                Some(c) => Ok(ResolvedGround::Stored(c.reference.clone())),
                None => tool_ground(id, tools)
                    .map(ResolvedGround::Tool)
                    .ok_or(StoreError::InvalidInput),
            }
        })
        .collect()
}

/// The switch of the integration that owns a ground; checked first by both fences (D61).
fn switch_fence(c: &Connection, f: &SourceSettingsFence) -> Result<(), StoreError> {
    let kind = match f {
        SourceSettingsFence::Git { .. } => BuiltInIntegration::LocalProjects,
        SourceSettingsFence::Codex(_) => BuiltInIntegration::Codex,
        SourceSettingsFence::Integration(m) => m.metadata.kind().into(),
        // A plugin's own enabled flag plays the switch role for its saved results.
        SourceSettingsFence::Plugin(m) => return plugins::authorized(c, m),
    };
    integrations::enabled(c, kind)
}
pub(super) fn source_settings_fence(
    c: &Connection,
    f: &SourceSettingsFence,
) -> Result<(), StoreError> {
    switch_fence(c, f)?;
    match f {
        SourceSettingsFence::Git { path, revision }
            if sources::root(c, path)?.revision == *revision =>
        {
            Ok(())
        }
        SourceSettingsFence::Codex(m) => codex::check_saved(c, m),
        SourceSettingsFence::Integration(m) => integrations::check_saved(c, m),
        SourceSettingsFence::Plugin(m) => plugins::check_saved(c, m),
        _ => Err(StoreError::StaleRevision),
    }
}

/// Retention depends on authorization. Ordinary source refreshes change TaskVersion and stay stale.
pub(super) fn source_authorization_fence(
    c: &Connection,
    f: &SourceSettingsFence,
) -> Result<(), StoreError> {
    switch_fence(c, f)?;
    match f {
        SourceSettingsFence::Git { .. } => source_settings_fence(c, f),
        SourceSettingsFence::Integration(m) => integrations::authorized(c, m),
        SourceSettingsFence::Plugin(m) => plugins::authorized(c, m),
        SourceSettingsFence::Codex(m)
            if codex::selection(c, m.selection_id)?.revision == m.settings_revision =>
        {
            Ok(())
        }
        _ => Err(StoreError::StaleRevision),
    }
}
