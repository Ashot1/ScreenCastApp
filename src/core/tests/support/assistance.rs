use ambient_core::*;
pub fn prepare_new(
    s: &mut Store,
    task: TaskId,
    revision: u64,
    notes: &[EvidenceId],
) -> Result<AssistancePreview, StoreError> {
    let workspace_revision = s.assistant_workspace_revision(task)?;
    s.prepare_assistance(AssistanceRequest {
        task: TaskVersion {
            task_id: task,
            revision,
        },
        workspace_revision,
        mode: AssistanceMode::NewContext,
        selected_evidence: notes.to_vec(),
        reply: None,
    })
}
pub fn prepare_reply(
    s: &mut Store,
    target: ProposalTarget,
    text: &str,
) -> Result<AssistancePreview, StoreError> {
    let p = s.proposal_details(target)?.proposal;
    s.prepare_assistance(AssistanceRequest {
        task: p.task,
        workspace_revision: p.workspace_revision,
        mode: AssistanceMode::Continue,
        selected_evidence: vec![],
        reply: Some(ProposalReply {
            target,
            text: text.into(),
        }),
    })
}
pub fn finish_single(
    s: &mut Store,
    id: AttemptId,
    result: ModelCheckResult,
    answer: Option<AssistanceAnswer>,
) -> Result<Option<ProposalTarget>, StoreError> {
    let publication = s.finish_assistance(id, result, answer)?;
    match publication {
        Some(v) => Ok(s
            .proposal_feed()?
            .proposals
            .into_iter()
            .find(|p| p.task == v.task)
            .map(|p| p.target())),
        None => Ok(None),
    }
}
pub fn suppress(s: &mut Store, target: ProposalTarget) -> Result<(), StoreError> {
    let p = s.proposal_details(target)?.proposal;
    if p.stale {
        return Err(StoreError::StaleRevision);
    }
    s.set_initiative_suppressed(p.task.task_id, p.task.revision, true)
}
/// Makes High assistance current through the Core test path.
pub fn ready(s: &mut Store) -> Result<(), StoreError> {
    s.set_model_session(9, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(d.id(), confirmed())?;
    s.release_check(&flag);
    Ok(())
}
pub fn confirmed() -> ModelCheckResult {
    ModelCheckResult {
        outcome: CheckOutcome::Confirmed,
        actual: ActualConfig {
            model: ActualModel::Luna,
            effort: ActualEffort::High,
            tier: ActualTier::Fast,
        },
        usage: None,
    }
}
/// One clarification grounded only in the task text.
pub fn answer(task: TaskId, id: Option<ProposalId>) -> AssistanceAnswer {
    AssistanceAnswer {
        proposals: vec![ProposalAnswer {
            proposal_id: id,
            kind: AssistanceKind::Clarification,
            message: "Unconfirmed premise?".into(),
            rationale: "Saved intent".into(),
            evidence_ids: vec![format!("task:{}", task.0)],
            step_id: None,
        }],
        rationale: "Full set".into(),
    }
}
