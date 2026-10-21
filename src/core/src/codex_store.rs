use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

impl Store {
    pub fn codex_selections(&self) -> Result<Vec<CodexSelection>, StoreError> {
        selections(&self.connection)
    }
    pub fn codex_selection(&self, id: u64) -> Result<CodexSelection, StoreError> {
        selection(&self.connection, id)
    }
    pub fn save_codex_selection(
        &mut self,
        id: u64,
        expected: u64,
        path: &str,
    ) -> Result<CodexSelection, StoreError> {
        validate_root(path, &[])?;
        let result = self.change(|c|{
            if id==0 && expected!=0 {return Err(StoreError::InvalidInput);}
            if id!=0 && selection(c,id)?.revision!=expected {return Err(StoreError::StaleRevision);}
            let all=selections(c)?;
            if id==0 && all.len()>=64 {return Err(StoreError::ResourceLimit);}
            if all.iter().filter(|s|s.id!=id).map(|s|s.path.len()+16).sum::<usize>()+path.len()+16>1024*1024 {return Err(StoreError::ResourceLimit);}
            let clock:i64=c.query_row("SELECT generation FROM local_source_clock WHERE singleton=1",[],|r|r.get(0))?;
            let revision=clock.checked_add(1).ok_or(StoreError::ResourceLimit)?;
            c.execute("UPDATE local_source_clock SET generation=?1 WHERE singleton=1",[revision])?;
            let id=if id==0 {revision as u64}else{id};
            c.execute("INSERT INTO codex_selections(id,revision,path) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,path=excluded.path",params![sql_id(id)?,revision,path])?;
            selection(c,id)
        })?;
        self.cancel_local_read(0);
        self.cancel_codex_read(0);
        Ok(result)
    }
    pub fn delete_codex_selection(&mut self, id: u64, revision: u64) -> Result<(), StoreError> {
        self.change(|c| {
            if selection(c, id)?.revision != revision {
                return Err(StoreError::StaleRevision);
            }
            c.execute("DELETE FROM codex_selections WHERE id=?1", [sql_id(id)?])?;
            Ok(())
        })?;
        self.cancel_local_read(0);
        self.cancel_codex_read(0);
        Ok(())
    }
    pub fn begin_codex_read(
        &mut self,
        task: TaskVersion,
        id: u64,
        revision: u64,
        cursor: u64,
    ) -> Result<u64, StoreError> {
        if self.codex_read.as_ref().is_some_and(|r| r.io_active) {
            return Err(StoreError::Busy);
        }
        self.require_assistance()?;
        plan::open_fence(&self.connection, task.task_id, task.revision)?;
        integrations::enabled(&self.connection, BuiltInIntegration::Codex)?;
        let selection = selection(&self.connection, id)?;
        if selection.revision != revision {
            return Err(StoreError::StaleRevision);
        }
        let prior = if cursor == 0 {
            None
        } else {
            let r = self.codex_read.as_ref().ok_or(StoreError::StaleRevision)?;
            self.codex_fence(r.id)?;
            Some(
                r.cursor
                    .as_ref()
                    .filter(|c| c.token == cursor && c.before > 0 && c.selection == selection)
                    .ok_or(StoreError::StaleRevision)?
                    .clone(),
            )
        };
        let id = self.next_source;
        self.next_source = id.checked_add(1).ok_or(StoreError::ResourceLimit)?;
        self.codex_read = Some(CodexReadOperation {
            id,
            task,
            selection,
            epoch: self.model_session.epoch,
            cancelled: Arc::new(AtomicBool::new(false)),
            io_active: true,
            prior,
            cursor: None,
            plan: None,
            window: None,
            observed_at_utc: 0,
        });
        Ok(id)
    }
    fn codex_fence(&self, id: u64) -> Result<&CodexReadOperation, StoreError> {
        self.require_assistance()?;
        let r = self
            .codex_read
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)?;
        if r.cancelled.load(Ordering::Acquire) {
            return Err(StoreError::Cancelled);
        }
        if r.epoch != self.model_session.epoch
            || selection(&self.connection, r.selection.id)?.revision != r.selection.revision
        {
            return Err(StoreError::StaleRevision);
        }
        plan::open_fence(&self.connection, r.task.task_id, r.task.revision)?;
        integrations::enabled(&self.connection, BuiltInIntegration::Codex)?;
        Ok(r)
    }
    pub fn permit_codex_read(&self, id: u64) -> Result<CodexSelection, StoreError> {
        let r = self.codex_fence(id)?;
        if !r.io_active {
            return Err(StoreError::InvalidInput);
        }
        Ok(r.selection.clone())
    }
    pub fn codex_read_cancellation(&self, id: u64) -> Result<Arc<AtomicBool>, StoreError> {
        self.codex_read
            .as_ref()
            .filter(|r| r.id == id)
            .map(|r| r.cancelled.clone())
            .ok_or(StoreError::StaleRevision)
    }
    pub fn cancel_codex_read(&self, id: u64) {
        if let Some(r) = self.codex_read.as_ref().filter(|r| id == 0 || r.id == id) {
            r.cancelled.store(true, Ordering::Release);
        }
    }
    pub(super) fn invalidate_stale_codex(&self) {
        if let Some(r) = &self.codex_read
            && self.codex_fence(r.id).is_err()
        {
            r.cancelled.store(true, Ordering::Release);
        }
    }
    pub fn plan_codex_read(
        &mut self,
        id: u64,
        file: CodexFileIdentity,
        header: &[u8],
    ) -> Result<CodexReadPlan, StoreError> {
        let r = self.codex_fence(id)?;
        if !r.io_active
            || r.plan.is_some()
            || file.size > i64::MAX as u64
            || file.size < header.len() as u64
        {
            return Err(StoreError::InvalidInput);
        }
        let header = parse_codex_header(header)?;
        let end = if let Some(prior) = &r.prior {
            if prior.file.device != file.device
                || prior.file.inode != file.inode
                || file.size < prior.file.size
                || prior.header != header
            {
                return Err(StoreError::StaleRevision);
            }
            prior.before
        } else {
            file.size
        };
        let start = end.saturating_sub(MAX_CODEX_WINDOW as u64);
        let plan = CodexReadPlan { start, end };
        let selection = r.selection.clone();
        let r = self.codex_read.as_mut().ok_or(StoreError::StaleRevision)?;
        r.cursor = Some(CodexCursor {
            token: id,
            selection,
            file,
            header,
            before: 0,
        });
        r.plan = Some(plan.clone());
        Ok(plan)
    }
    /// Native confirms actual syscall return and closure of every operation FD before this call.
    pub fn finish_codex_read(
        &mut self,
        id: u64,
        bytes: &[u8],
        observed_at_utc: i64,
    ) -> Result<CodexObservation, StoreError> {
        let r = self
            .codex_read
            .as_mut()
            .filter(|r| r.id == id && r.io_active)
            .ok_or(StoreError::StaleRevision)?;
        r.io_active = false;
        let r = self.codex_fence(id)?;
        let plan = r.plan.as_ref().ok_or(StoreError::InvalidInput)?;
        let cursor = r.cursor.as_ref().ok_or(StoreError::InvalidInput)?;
        let probe = usize::from(plan.start > 0);
        if bytes.len() > MAX_CODEX_WINDOW + 1
            || bytes.len() as u64 != plan.end - plan.start + probe as u64
        {
            return Err(StoreError::InvalidInput);
        }
        let starts_at_line = probe == 0 || bytes.first() == Some(&b'\n');
        let window =
            parse_codex_window(&cursor.header, &bytes[probe..], plan.start, starts_at_line)?;
        if plan.end > 0 && window.next_before >= plan.end {
            return Err(StoreError::InvalidInput);
        }
        let r = self.codex_read.as_mut().ok_or(StoreError::StaleRevision)?;
        r.cursor.as_mut().ok_or(StoreError::InvalidInput)?.before = window.next_before;
        r.window = Some(window);
        r.observed_at_utc = observed_at_utc;
        self.codex_observation(id)
    }
    pub fn codex_observation(&self, id: u64) -> Result<CodexObservation, StoreError> {
        let r = self.codex_fence(id)?;
        let cursor = r.cursor.as_ref().ok_or(StoreError::Busy)?;
        Ok(CodexObservation {
            thread_id: cursor.header.thread_id.clone(),
            observed_at_utc: r.observed_at_utc,
            cursor: if cursor.before > 0 { cursor.token } else { 0 },
            window: r.window.clone().ok_or(StoreError::Busy)?,
        })
    }
    pub fn abort_codex_read(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self
            .codex_read
            .as_mut()
            .filter(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)?;
        r.cancelled.store(true, Ordering::Release);
        r.io_active = false;
        r.window = None;
        r.cursor = None;
        Ok(())
    }
    pub fn release_codex_read(&mut self, id: u64) -> Result<(), StoreError> {
        let r = self
            .codex_read
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or(StoreError::StaleRevision)?;
        if r.io_active {
            return Err(StoreError::Busy);
        }
        self.codex_read = None;
        Ok(())
    }
    pub fn codex_event(&self, id: u64, index: usize) -> Result<CodexEvent, StoreError> {
        self.codex_fence(id)?
            .window
            .as_ref()
            .ok_or(StoreError::Busy)?
            .events
            .get(index)
            .cloned()
            .ok_or(StoreError::InvalidInput)
    }
    pub fn bind_codex_event(
        &mut self,
        id: u64,
        index: usize,
        at: i64,
    ) -> Result<EvidenceId, StoreError> {
        let r = self.codex_fence(id)?;
        let task = r.task;
        let selection = r.selection.clone();
        let e = self.codex_event(id, index)?;
        if e.variable_bytes() > MAX_TEXT_BYTES {
            return Err(StoreError::TextTooLong);
        }
        e.identity.validate()?;
        let digest = e.key.digest()?;
        self.change(|c|{
            let prior:Option<(i64,Option<String>)>=c.query_row("SELECT id,namespace FROM source_bindings WHERE task_id=?1 AND digest=?2",params![task.task_id.0,digest.as_slice()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let old=if let Some((binding,namespace))=prior {
                if namespace.is_none(){return Err(StoreError::Deleted);}
                let m=metadata(c,binding)?;
                let evidence=owned_evidence(c,m.evidence_id,task.task_id)?;
                if m.selection_id!=selection.id || m.accepted_ordinal>e.ordinal || evidence.kind==AssertionKind::UserConfirmation {return Err(StoreError::InvalidInput);}
                let version=match &evidence.origin {Origin::External{version,..}=>version.as_deref(),_=>None};
                if m.accepted_ordinal==e.ordinal && version!=Some(&e.version){return Err(StoreError::InvalidInput);}
                Some(m)
            }else{None};
            let delta=e.identity.variable_bytes() as i64-old.as_ref().map_or(0,|m|m.identity.variable_bytes()) as i64;
            byte_room(c,delta)?;
            let outcome=write_source(c,task,&e.key,Some(&e.version),&e.evidence,Some(at))?;
            let id=match outcome{IngestOutcome::Applied(id)=>id,IngestOutcome::Forgotten=>return Err(StoreError::Deleted),IngestOutcome::UserProtected=>return Err(StoreError::InvalidInput)};
            let binding:i64=c.query_row("SELECT id FROM source_bindings WHERE evidence_id=?1 AND namespace IS NOT NULL",[id.0],|r|r.get(0))?;
            c.execute("INSERT INTO codex_metadata(binding_id,selection_id,settings_revision,accepted_ordinal,turn_id,item_id,kind) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(binding_id) DO UPDATE SET selection_id=excluded.selection_id,settings_revision=excluded.settings_revision,accepted_ordinal=excluded.accepted_ordinal,turn_id=excluded.turn_id,item_id=excluded.item_id,kind=excluded.kind",params![binding,sql_id(selection.id)?,sql_id(selection.revision)?,e.ordinal.to_be_bytes().as_slice(),e.identity.turn_id(),e.identity.item_id(),e.identity.kind() as u32])?;
            byte_room(c,0)?;Ok(id)
        })
    }
    pub fn codex_metadata(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<CodexMetadata>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let owner = read_task(&c, task)?;
        let mut q=c.prepare("SELECT b.id FROM source_bindings b JOIN codex_metadata m ON m.binding_id=b.id WHERE b.task_id=?1 AND b.id>?2 ORDER BY b.id LIMIT ?3")?;
        let ids = q
            .query_map(params![task.0, after, (limit + 1) as i64], |r| {
                r.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let rows = ids
            .into_iter()
            .map(|id| metadata(&c, id).map_err(|_| rusqlite::Error::InvalidQuery));
        Ok(plan::plan_page(
            collect_page(
                rows,
                limit,
                |m| m.binding_id,
                |m| 128 + m.identity.variable_bytes(),
            )?,
            &owner,
        ))
    }
}
fn selections(c: &Connection) -> Result<Vec<CodexSelection>, StoreError> {
    let mut q = c.prepare("SELECT id,revision,path FROM codex_selections ORDER BY id LIMIT 65")?;
    let rows = q
        .query_map([], |r| {
            Ok(CodexSelection {
                id: row_id(r, 0)?,
                revision: row_id(r, 1)?,
                path: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > 64 {
        return Err(StoreError::ResourceLimit);
    }
    Ok(rows)
}
pub(super) fn selection(c: &Connection, id: u64) -> Result<CodexSelection, StoreError> {
    if id == 0 || id > i64::MAX as u64 {
        return Err(StoreError::InvalidInput);
    }
    c.query_row(
        "SELECT id,revision,path FROM codex_selections WHERE id=?1",
        [sql_id(id)?],
        |r| {
            Ok(CodexSelection {
                id: row_id(r, 0)?,
                revision: row_id(r, 1)?,
                path: r.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}
fn metadata(c: &Connection, binding: i64) -> Result<CodexMetadata, StoreError> {
    let (evidence,selection,revision,ordinal,turn,item,kind):(i64,u64,u64,Vec<u8>,String,Option<String>,u32)=c.query_row("SELECT b.evidence_id,m.selection_id,m.settings_revision,m.accepted_ordinal,m.turn_id,m.item_id,m.kind FROM codex_metadata m JOIN source_bindings b ON b.id=m.binding_id WHERE m.binding_id=?1 AND b.namespace IS NOT NULL",[binding],|r|Ok((r.get(0)?,row_id(r,1)?,row_id(r,2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
    let kind = match kind {
        1 => CodexEventKind::User,
        2 => CodexEventKind::Agent,
        3 => CodexEventKind::Plan,
        4 => CodexEventKind::DelegatedCreate,
        5 => CodexEventKind::DelegatedSend,
        6 => CodexEventKind::Started,
        7 => CodexEventKind::Completed,
        8 => CodexEventKind::Failed,
        9 => CodexEventKind::Aborted,
        _ => return Err(StoreError::UnsupportedSchema),
    };
    let identity = match item {
        Some(item_id) => CodexEventIdentity::Item {
            turn_id: turn,
            item_id,
            kind,
        },
        None => CodexEventIdentity::Turn {
            turn_id: turn,
            kind,
        },
    };
    identity.validate()?;
    Ok(CodexMetadata {
        binding_id: binding,
        evidence_id: EvidenceId(evidence),
        selection_id: selection,
        settings_revision: revision,
        accepted_ordinal: u64::from_be_bytes(
            ordinal
                .try_into()
                .map_err(|_| StoreError::UnsupportedSchema)?,
        ),
        identity,
    })
}
pub(super) fn stored_metadata(c: &Connection, id: EvidenceId) -> Result<CodexMetadata, StoreError> {
    let binding = c.query_row(
        "SELECT id FROM source_bindings WHERE evidence_id=?1 AND namespace=?2",
        params![id.0, CODEX_NAMESPACE],
        |r| r.get(0),
    )?;
    metadata(c, binding)
}
pub(super) fn evidence_metadata(
    c: &Connection,
    id: EvidenceId,
) -> Result<CodexMetadata, StoreError> {
    let m = stored_metadata(c, id)?;
    check_saved(c, &m)?;
    Ok(m)
}
pub(super) fn check_saved(c: &Connection, m: &CodexMetadata) -> Result<(), StoreError> {
    if selection(c, m.selection_id)?.revision != m.settings_revision
        || metadata(c, m.binding_id)? != *m
    {
        return Err(StoreError::StaleRevision);
    }
    Ok(())
}
pub(super) fn validate_codex(c: &Connection) -> Result<(), StoreError> {
    let clock: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let clock = u64::try_from(clock).map_err(|_| StoreError::UnsupportedSchema)?;
    let settings = selections(c)?;
    let mut total = 0;
    for s in settings {
        validate_root(&s.path, &[])?;
        if s.revision > clock || s.id > clock {
            return Err(StoreError::UnsupportedSchema);
        }
        total += s.path.len() + 16;
    }
    if total > 1024 * 1024 {
        return Err(StoreError::UnsupportedSchema);
    }
    let bad:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM codex_metadata m JOIN source_bindings b ON b.id=m.binding_id WHERE b.namespace IS NULL OR b.namespace!=?1) OR EXISTS(SELECT 1 FROM source_bindings b WHERE b.namespace=?1 AND NOT EXISTS(SELECT 1 FROM codex_metadata m WHERE m.binding_id=b.id))",[CODEX_NAMESPACE],|r|r.get(0))?;
    if bad {
        return Err(StoreError::UnsupportedSchema);
    }
    let mut q = c.prepare("SELECT binding_id FROM codex_metadata")?;
    let ids = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        let m = metadata(c, id)?;
        if m.settings_revision > clock || m.selection_id > clock {
            return Err(StoreError::UnsupportedSchema);
        }
        let e = read_evidence(c, m.evidence_id)?;
        let extra = match &e.origin {
            Origin::External { key, version } => {
                if m.identity.source_key(&key.scope) != *key
                    || e.completeness == Completeness::Unknown
                    || e.kind == AssertionKind::Assumption
                {
                    return Err(StoreError::UnsupportedSchema);
                }
                let version = version.as_deref().ok_or(StoreError::UnsupportedSchema)?;
                if version.len() != 64
                    || !version
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(StoreError::UnsupportedSchema);
                }
                if e.kind == AssertionKind::SourceFact
                    && crate::codex_types::codex_event_version(
                        &key.scope,
                        &m.identity,
                        &SourceEvidence {
                            text: e.text.clone(),
                            observed_at_utc: e.observed_at_utc,
                            completeness: e.completeness,
                            kind: e.kind,
                        },
                    ) != version
                {
                    return Err(StoreError::UnsupportedSchema);
                }
                key.bytes() + version.len()
            }
            _ => return Err(StoreError::UnsupportedSchema),
        };
        if e.text.len() + extra + m.identity.variable_bytes() > MAX_TEXT_BYTES {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    Ok(())
}

fn sql_id(id: u64) -> Result<i64, StoreError> {
    i64::try_from(id)
        .ok()
        .filter(|v| *v > 0)
        .ok_or(StoreError::InvalidInput)
}
fn row_id(r: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = r.get(index)?;
    u64::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or(rusqlite::Error::InvalidQuery)
}
