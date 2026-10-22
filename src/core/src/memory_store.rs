#[path = "assistance_store.rs"]
mod assistance;
#[path = "check_store.rs"]
mod checks;
#[path = "codex_store.rs"]
mod codex;
#[path = "integration_store.rs"]
mod integrations;
#[path = "observation_store.rs"]
mod observation;
#[path = "plan_store.rs"]
mod plan;
#[path = "plugin_loop.rs"]
mod plugin_loop;
#[path = "plugin_store.rs"]
mod plugins;
#[path = "proposal_store.rs"]
mod proposals;
#[path = "local_source_store.rs"]
mod sources;
use crate::memory_types::{key_field, text_field};
use crate::*;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params};
use std::path::Path;
use std::time::Duration;
const SCHEMA: &str = include_str!("schema.sql");
const APPLICATION_ID: i32 = 0x414d_4231;
const EVIDENCE_SELECT: &str = "SELECT e.id,e.task_id,e.text,e.origin,e.observed_at_utc,e.completeness,e.kind,b.namespace,b.scope,b.object_id,b.version FROM evidence e LEFT JOIN source_bindings b ON b.evidence_id=e.id AND b.namespace IS NOT NULL";

pub struct Store {
    connection: Connection,
    local_read: Option<LocalReadOperation>,
    codex_read: Option<CodexReadOperation>,
    integration_reads: std::collections::BTreeMap<IntegrationKind, IntegrationRead>,
    next_source: u64,
    active_check: Option<ActiveModel>,
    model_session: ModelSession,
    high_capability: Option<(u64, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
    prepared: Option<std::sync::Arc<AssistanceSnapshot>>,
    next_request: u64,
    consumed_request: u64,
    observation: observation::ObservationState,
}
impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        match std::fs::metadata(path) {
            Ok(_) => {
                inspect_existing_schema(path)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(StoreError::Storage),
        }
        let mut c = Connection::open(path)?;
        c.busy_timeout(Duration::from_secs(2))?;
        let empty = inspect_schema(&c)?;
        c.pragma_update(None, "foreign_keys", true)?;
        let foreign: i32 = c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
        let journal: String =
            c.pragma_update_and_check(None, "journal_mode", "DELETE", |r| r.get(0))?;
        c.pragma_update(None, "secure_delete", true)?;
        let secure: i32 = c.pragma_query_value(None, "secure_delete", |r| r.get(0))?;
        if foreign != 1 || journal != "delete" || secure != 1 {
            return Err(StoreError::Storage);
        }
        if empty {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(SCHEMA)?;
            tx.execute(
                "INSERT INTO settings VALUES(1,0,0,?1,0,0,0,0)",
                [DEFAULT_PER_HOUR_LIMIT],
            )?;
            tx.execute("INSERT INTO local_source_clock VALUES(1,0)", [])?;
            // Secrets in native stores are bound to this random identity of one memory database.
            tx.execute("INSERT INTO store_identity VALUES(1,randomblob(16))", [])?;
            tx.execute("INSERT INTO integration_switches VALUES('local-projects',1),('codex',1),('github',1),('jira',1),('todoist',1)", [])?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", 17)?;
            tx.commit()?;
        }
        // A prior process may have submitted a request without receiving or saving its usage.
        c.execute(
            "UPDATE connection_checks SET stage=2,outcome=7 WHERE stage=1",
            [],
        )?;
        c.execute("UPDATE observation_sends SET outcome=7 WHERE outcome=0", [])?;
        // A call admitted before a restart may have reached the plugin server: it was interrupted.
        c.execute(
            "UPDATE plugin_calls SET stage=2,outcome=9 WHERE stage=1",
            [],
        )?;
        Ok(Self {
            connection: c,
            local_read: None,
            codex_read: None,
            integration_reads: std::collections::BTreeMap::new(),
            next_source: 1,
            active_check: None,
            model_session: ModelSession::default(),
            high_capability: None,
            prepared: None,
            next_request: 1,
            consumed_request: 0,
            observation: observation::ObservationState::default(),
        })
    }
    fn change<T>(
        &mut self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = operation(&tx)?;
        proposals::clear_revoked(&tx)?;
        tx.commit()?;
        self.invalidate_stale_highlight();
        self.invalidate_stale_assistance();
        self.invalidate_stale_source();
        self.invalidate_stale_codex();
        self.invalidate_stale_integrations();
        Ok(result)
    }
    /// Cancels every source read; each later fence also rejects it on its own.
    pub fn cancel_all_reads(&self) {
        self.cancel_local_read(0);
        self.cancel_codex_read(0);
        self.cancel_integration_reads();
    }
    pub fn manual_pause(&self) -> Result<bool, StoreError> {
        pause(&self.connection)
    }
    pub fn set_manual_pause(&mut self, paused: bool) -> Result<(), StoreError> {
        self.change(|c| {
            c.execute(
                "UPDATE settings SET manual_pause=?1 WHERE singleton=1",
                [paused],
            )?;
            Ok(())
        })?;
        if paused {
            self.cancel_all_reads();
            self.cancel_check();
            self.clear_assistance();
            self.stop_observation();
        }
        Ok(())
    }
    pub fn create_task(&mut self, text: &str) -> Result<Task, StoreError> {
        self.change(|c| insert_task(c, text))
    }
    pub fn task(&self, id: TaskId) -> Result<Task, StoreError> {
        read_task(&self.connection, id)
    }
    pub fn edit_task(&mut self, id: TaskId, expected: u64, text: &str) -> Result<Task, StoreError> {
        text_field(text)?;
        self.change(|c| {
            let old = fence(c, id, expected)?;
            byte_room(c, text.len() as i64 - old.text.len() as i64)?;
            if old.lifecycle == TaskLifecycle::Open {
                plan::invalidate_all(c, id)?;
            }
            bump(c, id)?;
            c.execute("UPDATE tasks SET text=?1 WHERE id=?2", params![text, id.0])?;
            read_task(c, id)
        })
    }
    pub fn delete_task(
        &mut self,
        id: TaskId,
        expected: u64,
        others: &[TaskVersion],
    ) -> Result<(), StoreError> {
        self.change(|c|{fence(c,id,expected)?;
            fence_impact(c,id,None,others)?;bump(c,id)?;
            c.execute("DELETE FROM decisions WHERE task_id=?1 OR other_task_id=?1 OR evidence_id IN (SELECT id FROM evidence WHERE task_id=?1)",[id.0])?;
            c.execute("DELETE FROM codex_metadata WHERE binding_id IN (SELECT id FROM source_bindings WHERE task_id=?1)",[id.0])?;
            c.execute("DELETE FROM integration_metadata WHERE binding_id IN (SELECT id FROM source_bindings WHERE task_id=?1)",[id.0])?;
            c.execute("UPDATE source_bindings SET namespace=NULL,scope=NULL,object_id=NULL,version=NULL WHERE task_id=?1",[id.0])?;
            c.execute("UPDATE evidence SET text=NULL,origin=NULL,observed_at_utc=NULL,completeness=NULL,kind=NULL WHERE task_id=?1",[id.0])?;
            plan::clear_plan(c,id)?; c.execute("UPDATE useful_references SET text=NULL WHERE task_id=?1",[id.0])?;c.execute("UPDATE tasks SET text=NULL WHERE id=?1",[id.0])?;Ok(())})
    }
    pub fn deletion_impact(
        &self,
        task: TaskId,
        evidence: Option<EvidenceId>,
        after: i64,
        limit: usize,
    ) -> Result<Page<Task>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let owner = read_task(&c, task)?;
        if let Some(id) = evidence {
            owned_evidence(&c, id, task)?;
        }
        let mut q = c.prepare(&impact_sql(evidence.is_some(), true))?;
        let rows = q.query_map(
            params![
                task.0,
                evidence.map_or(0, |id| id.0),
                after,
                (limit + 1) as i64
            ],
            decode_task,
        )?;
        let page = collect_page(rows, limit, |t| t.id.0, |t| 40 + t.text.len())?;
        Ok(plan::plan_page(page, &owner))
    }
    pub fn tasks(&self, after: i64, limit: usize) -> Result<Page<Task>, StoreError> {
        page_args(after, limit)?;
        let mut q=self.connection.prepare("SELECT id,revision,text,lifecycle,initiative_suppressed,plan_completeness FROM tasks WHERE id>?1 AND text IS NOT NULL ORDER BY id LIMIT ?2")?;
        let rows = q.query_map(params![after, (limit + 1) as i64], decode_task)?;
        collect_page(rows, limit, |t| t.id.0, |t| 40 + t.text.len())
    }
    pub fn add_note(
        &mut self,
        task: TaskId,
        expected: u64,
        text: &str,
        action_at_utc: i64,
    ) -> Result<Evidence, StoreError> {
        text_field(text)?;
        self.change(|c|{plan::open_fence(c,task,expected)?;reserve(c,"evidence",1)?;byte_room(c,text.len() as i64)?;c.execute("INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,0,?3,2,2)",params![task.0,text,action_at_utc])?;let id=EvidenceId(c.last_insert_rowid());bump(c,task)?;read_evidence(c,id)})
    }
    pub fn edit_evidence(
        &mut self,
        id: EvidenceId,
        task: TaskId,
        expected: u64,
        text: &str,
        action_at_utc: i64,
    ) -> Result<(), StoreError> {
        text_field(text)?;
        self.change(|c| {
            fence(c, task, expected)?;
            let old = owned_evidence(c, id, task)?;
            if let Origin::External { key, version } = &old.origin
                && key.namespace == CODEX_NAMESPACE
            {
                let m = codex::stored_metadata(c, id)?;
                if text.len()
                    + key.bytes()
                    + version.as_ref().map_or(0, String::len)
                    + m.identity.variable_bytes()
                    > MAX_TEXT_BYTES
                {
                    return Err(StoreError::TextTooLong);
                }
            }
            if let Origin::External { key, version } = &old.origin
                && IntegrationKind::from_namespace(&key.namespace).is_some()
            {
                let m = integrations::stored_metadata(c, id)?;
                let o = IntegrationObject {
                    key: key.clone(),
                    version: version.clone().ok_or(StoreError::UnsupportedSchema)?,
                    text: text.into(),
                    observed_at_utc: Some(action_at_utc),
                    metadata: m.metadata,
                    bindable: true,
                };
                if combined_bytes(&o)? > MAX_TEXT_BYTES {
                    return Err(StoreError::TextTooLong);
                }
            }
            if let Origin::External { key, .. } = &old.origin
                && key.namespace == PLUGIN_NAMESPACE
            {
                // The edited text and the saved plugin metadata share the 16 KiB object limit.
                let m = plugins::stored_metadata(c, id)?;
                if plugins::result_bytes(&m.metadata, text.len())? > MAX_TEXT_BYTES {
                    return Err(StoreError::TextTooLong);
                }
            }
            byte_room(c, text.len() as i64 - old.text.len() as i64)?;
            c.execute(
                "UPDATE evidence SET text=?1,kind=2,observed_at_utc=?2 WHERE id=?3",
                params![text, action_at_utc, id.0],
            )?;
            record_decision(c, task, id, true, action_at_utc)?;
            bump(c, task)
        })
    }
    pub fn delete_evidence(
        &mut self,
        id: EvidenceId,
        task: TaskId,
        expected: u64,
        others: &[TaskVersion],
    ) -> Result<(), StoreError> {
        self.change(|c|{fence(c,task,expected)?;owned_evidence(c,id,task)?;fence_impact(c,task,Some(id),others)?;
            // Identity and reserved digest rows survive; deletion needs no new capacity.
            c.execute("DELETE FROM decisions WHERE evidence_id=?1",[id.0])?;
            c.execute("DELETE FROM codex_metadata WHERE binding_id IN (SELECT id FROM source_bindings WHERE evidence_id=?1)",[id.0])?;
            c.execute("DELETE FROM integration_metadata WHERE binding_id IN (SELECT id FROM source_bindings WHERE evidence_id=?1)",[id.0])?;
            c.execute("UPDATE source_bindings SET namespace=NULL,scope=NULL,object_id=NULL,version=NULL WHERE evidence_id=?1",[id.0])?;
            c.execute("UPDATE evidence SET text=NULL,origin=NULL,observed_at_utc=NULL,completeness=NULL,kind=NULL WHERE id=?1",[id.0])?;bump(c,task)})
    }
    pub fn move_evidence(
        &mut self,
        id: EvidenceId,
        from: TaskId,
        from_revision: u64,
        to: TaskId,
        to_revision: u64,
        action_at_utc: i64,
    ) -> Result<(), StoreError> {
        if from == to {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c|{fence(c,from,from_revision)?;plan::open_fence(c,to,to_revision)?;let old=owned_evidence(c,id,from)?;
            if let Origin::External{key,version}=old.origin {
                let digest=key.digest()?;
                let existing:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM source_bindings WHERE task_id=?1 AND digest=?2)",params![to.0,digest.as_slice()],|r|r.get(0))?;
                if existing{return Err(StoreError::InvalidInput);}
                reserve(c,"source_bindings",1)?;
                c.execute("UPDATE source_bindings SET namespace=NULL,scope=NULL,object_id=NULL,version=NULL WHERE task_id=?1 AND evidence_id=?2",params![from.0,id.0])?;
                insert_binding(c,to,id,&key,version.as_deref())?;
                c.execute("UPDATE codex_metadata SET binding_id=?1 WHERE binding_id IN (SELECT id FROM source_bindings WHERE task_id=?2 AND evidence_id=?3)",params![c.last_insert_rowid(),from.0,id.0])?;
                c.execute("UPDATE integration_metadata SET binding_id=?1 WHERE binding_id IN (SELECT id FROM source_bindings WHERE task_id=?2 AND evidence_id=?3)",params![c.last_insert_rowid(),from.0,id.0])?;
            }
            c.execute("UPDATE evidence SET task_id=?1 WHERE id=?2",params![to.0,id.0])?;
            record_decision(c,from,id,false,action_at_utc)?;record_decision(c,to,id,true,action_at_utc)?;
            bump(c,from)?;bump(c,to)})
    }
    pub fn evidence(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<Evidence>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let owner = read_task(&c, task)?;
        let mut q=c.prepare(&format!("{EVIDENCE_SELECT} WHERE e.task_id=?1 AND e.text IS NOT NULL AND e.id>?2 ORDER BY e.id LIMIT ?3"))?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], decode_evidence)?;
        let page = collect_page(
            rows,
            limit,
            |e| e.id.0,
            |e| {
                88 + e.text.len()
                    + match &e.origin {
                        Origin::User => 0,
                        Origin::External { key, version } => {
                            key.bytes() + version.as_ref().map_or(0, String::len)
                        }
                    }
            },
        )?;
        Ok(plan::plan_page(page, &owner))
    }
    pub fn sources(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<SourceBinding>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let owner = read_task(&c, task)?;
        let mut q=c.prepare("SELECT id,task_id,evidence_id,namespace,scope,object_id,version FROM source_bindings WHERE task_id=?1 AND id>?2 AND namespace IS NOT NULL ORDER BY id LIMIT ?3")?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], |r| {
            Ok(SourceBinding {
                id: r.get(0)?,
                task_id: TaskId(r.get(1)?),
                evidence_id: EvidenceId(r.get(2)?),
                key: SourceKey {
                    namespace: r.get(3)?,
                    scope: r.get(4)?,
                    object_id: r.get(5)?,
                },
                version: r.get(6)?,
            })
        })?;
        let page = collect_page(
            rows,
            limit,
            |s| s.id,
            |s| 64 + s.key.bytes() + s.version.as_ref().map_or(0, String::len),
        )?;
        Ok(plan::plan_page(page, &owner))
    }
    pub fn ingest_source(
        &mut self,
        task: TaskId,
        expected: u64,
        key: &SourceKey,
        version: Option<&str>,
        input: &SourceEvidence,
    ) -> Result<IngestOutcome, StoreError> {
        if input.kind == AssertionKind::UserConfirmation {
            return Err(StoreError::InvalidInput);
        }
        self.source_write(task, expected, key, version, input, None)
    }
    pub fn bind_source(
        &mut self,
        task: TaskId,
        expected: u64,
        key: &SourceKey,
        version: Option<&str>,
        input: &SourceEvidence,
        action_at_utc: i64,
    ) -> Result<EvidenceId, StoreError> {
        match self.source_write(task, expected, key, version, input, Some(action_at_utc))? {
            IngestOutcome::Applied(id) => Ok(id),
            IngestOutcome::Forgotten => Err(StoreError::Deleted),
            IngestOutcome::UserProtected => Err(StoreError::InvalidInput),
        }
    }
    fn source_write(
        &mut self,
        task: TaskId,
        expected: u64,
        key: &SourceKey,
        version: Option<&str>,
        input: &SourceEvidence,
        explicit: Option<i64>,
    ) -> Result<IngestOutcome, StoreError> {
        if reserved_namespace(&key.namespace) {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c| {
            write_source(
                c,
                TaskVersion {
                    task_id: task,
                    revision: expected,
                },
                key,
                version,
                input,
                explicit,
            )
        })
    }

    pub fn set_link(
        &mut self,
        a: TaskId,
        a_revision: u64,
        b: TaskId,
        b_revision: u64,
        related: bool,
        action_at_utc: i64,
    ) -> Result<(), StoreError> {
        if a == b {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c|{fence(c,a,a_revision)?;fence(c,b,b_revision)?;let (left,right)=if a.0<b.0 {(a,b)} else {(b,a)};
            let id:Option<i64>=c.query_row("SELECT id FROM decisions WHERE task_id=?1 AND other_task_id=?2",params![left.0,right.0],|r|r.get(0)).optional()?;
            if let Some(id)=id {c.execute("UPDATE decisions SET accepted=?1,action_at_utc=?2 WHERE id=?3",params![related,action_at_utc,id])?;} else {reserve(c,"decisions",1)?;c.execute("INSERT INTO decisions(task_id,other_task_id,accepted,action_at_utc) VALUES(?1,?2,?3,?4)",params![left.0,right.0,related,action_at_utc])?;}
            bump(c,a)?;bump(c,b)})
    }
    pub fn links(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<TaskLink>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let owner = read_task(&c, task)?;
        let mut q=c.prepare("SELECT d.id,t.id,d.accepted,d.action_at_utc,t.revision,t.text,t.lifecycle,t.initiative_suppressed,t.plan_completeness FROM decisions d JOIN tasks t ON t.id=CASE WHEN d.task_id=?1 THEN d.other_task_id ELSE d.task_id END WHERE d.other_task_id IS NOT NULL AND (d.task_id=?1 OR d.other_task_id=?1) AND d.id>?2 ORDER BY d.id LIMIT ?3")?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], |r| {
            Ok(TaskLink {
                id: r.get(0)?,
                other_task_id: TaskId(r.get(1)?),
                related: r.get(2)?,
                action_at_utc: r.get(3)?,
                other_task_revision: u64::try_from(r.get::<_, i64>(4)?)
                    .ok()
                    .filter(|v| *v > 0)
                    .ok_or(rusqlite::Error::InvalidQuery)?,
                other_task_text: r.get(5)?,
                other_task_lifecycle: match r.get::<_, i64>(6)? {
                    0 => TaskLifecycle::Open,
                    1 => TaskLifecycle::Completed,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                },
                other_task_initiative_suppressed: r.get(7)?,
                other_task_plan_completeness: plan::decode_completeness(r.get(8)?)?,
            })
        })?;
        let page = collect_page(rows, limit, |l| l.id, |l| 64 + l.other_task_text.len())?;
        Ok(plan::plan_page(page, &owner))
    }
    pub fn decisions(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<BindingDecision>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let owner = read_task(&c, task)?;
        let mut q=c.prepare("SELECT id,evidence_id,accepted,action_at_utc FROM decisions WHERE task_id=?1 AND evidence_id IS NOT NULL AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], |r| {
            Ok(BindingDecision {
                id: r.get(0)?,
                evidence_id: EvidenceId(r.get(1)?),
                accepted: r.get(2)?,
                action_at_utc: r.get(3)?,
            })
        })?;
        let page = collect_page(rows, limit, |d| d.id, |_| 32)?;
        Ok(plan::plan_page(page, &owner))
    }
}
fn pause(c: &Connection) -> Result<bool, StoreError> {
    let (singleton, paused, count): (i64, i64, i64) = c
        .query_row(
            "SELECT singleton,manual_pause,(SELECT count(*) FROM settings) FROM settings",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| StoreError::UnsupportedSchema)?;
    if singleton != 1 || count != 1 || !matches!(paused, 0 | 1) {
        return Err(StoreError::UnsupportedSchema);
    }
    Ok(paused == 1)
}
fn insert_task(c: &Connection, text: &str) -> Result<Task, StoreError> {
    text_field(text)?;
    reserve(c, "tasks", 1)?;
    let live: i64 = c.query_row(
        "SELECT count(*) FROM tasks WHERE text IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    if live >= MAX_LIVE_TASKS {
        return Err(StoreError::ResourceLimit);
    }
    byte_room(c, text.len() as i64)?;
    c.execute("INSERT INTO tasks(revision,text) VALUES(1,?1)", [text])?;
    read_task(c, TaskId(c.last_insert_rowid()))
}
fn read_task(c: &Connection, id: TaskId) -> Result<Task, StoreError> {
    if id.0 <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let live: Option<bool> = c
        .query_row(
            "SELECT text IS NOT NULL FROM tasks WHERE id=?1",
            [id.0],
            |r| r.get(0),
        )
        .optional()?;
    match live { None => Err(StoreError::NotFound), Some(false) => Err(StoreError::Deleted), Some(true) => c.query_row("SELECT id,revision,text,lifecycle,initiative_suppressed,plan_completeness FROM tasks WHERE id=?1",[id.0],decode_task).map_err(|_|StoreError::UnsupportedSchema) }
}

fn fence(c: &Connection, id: TaskId, expected: u64) -> Result<Task, StoreError> {
    if expected == 0 || expected > i64::MAX as u64 {
        return Err(StoreError::InvalidInput);
    }
    let task = read_task(c, id)?;
    if task.revision != expected {
        Err(StoreError::StaleRevision)
    } else {
        Ok(task)
    }
}
fn bump(c: &Connection, id: TaskId) -> Result<(), StoreError> {
    let task = read_task(c, id)?;
    if task.revision == i64::MAX as u64 {
        return Err(StoreError::ResourceLimit);
    }
    c.execute("UPDATE tasks SET revision=revision+1 WHERE id=?1", [id.0])?;
    Ok(())
}
fn decode_task(r: &Row<'_>) -> rusqlite::Result<Task> {
    let id: i64 = r.get(0)?;
    let revision: i64 = r.get(1)?;
    let text: String = r.get(2)?;
    if id <= 0 || revision <= 0 || text_field(&text).is_err() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(Task {
        id: TaskId(id),
        revision: revision as u64,
        text,
        lifecycle: match r.get::<_, i64>(3)? {
            0 => TaskLifecycle::Open,
            1 => TaskLifecycle::Completed,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        initiative_suppressed: r.get(4)?,
        plan_completeness: plan::decode_completeness(r.get(5)?)?,
    })
}
fn decode_evidence(r: &Row<'_>) -> rusqlite::Result<Evidence> {
    let id: i64 = r.get(0)?;
    let task: i64 = r.get(1)?;
    let text: String = r.get(2)?;
    let raw: i64 = r.get(3)?;
    let observed_at_utc: Option<i64> = r.get(4)?;
    let completeness = match r.get::<_, i64>(5)? {
        0 => Completeness::Unknown,
        1 => Completeness::Partial,
        2 => Completeness::Complete,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let kind = match r.get::<_, i64>(6)? {
        0 => AssertionKind::SourceFact,
        1 => AssertionKind::Assumption,
        2 => AssertionKind::UserConfirmation,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let origin = match raw {
        0 if observed_at_utc.is_some() => Origin::User,
        1 => {
            let key = SourceKey {
                namespace: r.get(7)?,
                scope: r.get(8)?,
                object_id: r.get(9)?,
            };
            let version: Option<String> = r.get(10)?;
            if key.validate().is_err()
                || version.as_ref().is_some_and(|v| key_field(v, 256).is_err())
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Origin::External { key, version }
        }
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    if id <= 0 || task <= 0 || text_field(&text).is_err() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(Evidence {
        id: EvidenceId(id),
        task_id: TaskId(task),
        text,
        origin,
        observed_at_utc,
        completeness,
        kind,
    })
}
fn read_evidence(c: &Connection, id: EvidenceId) -> Result<Evidence, StoreError> {
    if id.0 <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let live: Option<bool> = c
        .query_row(
            "SELECT text IS NOT NULL FROM evidence WHERE id=?1",
            [id.0],
            |r| r.get(0),
        )
        .optional()?;
    match live {
        None => return Err(StoreError::NotFound),
        Some(false) => return Err(StoreError::Deleted),
        _ => {}
    }
    c.query_row(
        &format!("{EVIDENCE_SELECT} WHERE e.id=?1"),
        [id.0],
        decode_evidence,
    )
    .map_err(|_| StoreError::UnsupportedSchema)
}
fn owned_evidence(c: &Connection, id: EvidenceId, task: TaskId) -> Result<Evidence, StoreError> {
    let e = read_evidence(c, id)?;
    if e.task_id != task {
        Err(StoreError::StaleRevision)
    } else {
        Ok(e)
    }
}
/// Namespaces written only by their own typed paths (Codex, integrations, plugin results, screen
/// quotes of the observation answer).
fn reserved_namespace(namespace: &str) -> bool {
    namespace == CODEX_NAMESPACE
        || namespace == PLUGIN_NAMESPACE
        || namespace == SCREEN_NAMESPACE
        || IntegrationKind::from_namespace(namespace).is_some()
}
fn reserve(c: &Connection, table: &str, count: i64) -> Result<(), StoreError> {
    let (rows, max): (i64, i64) = c.query_row(
        &format!("SELECT count(*),coalesce(max(id),0) FROM {table}"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if rows + count > MAX_ROWS || max > i64::MAX - count {
        Err(StoreError::ResourceLimit)
    } else {
        Ok(())
    }
}
fn retained_bytes(c: &Connection) -> Result<i64, StoreError> {
    Ok(c.query_row("SELECT (SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM tasks)+(SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM evidence)+(SELECT coalesce(sum(coalesce(length(CAST(namespace AS BLOB)),0)+coalesce(length(CAST(scope AS BLOB)),0)+coalesce(length(CAST(object_id AS BLOB)),0)+coalesce(length(CAST(version AS BLOB)),0)),0) FROM source_bindings)+(SELECT coalesce(sum(coalesce(length(CAST(description AS BLOB)),0)+coalesce(length(CAST(prompt AS BLOB)),0)+coalesce(length(CAST(reference AS BLOB)),0)),0) FROM plan_steps)+(SELECT coalesce(sum(length(CAST(description AS BLOB))),0) FROM plan_checks)+(SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM useful_references)+(SELECT coalesce(sum(length(CAST(turn_id AS BLOB))+coalesce(length(CAST(item_id AS BLOB)),0)),0) FROM codex_metadata)+(SELECT coalesce(sum(length(CAST(data AS BLOB))),0) FROM integration_metadata)+(SELECT coalesce(sum(length(CAST(selected AS BLOB))+length(CAST(sources AS BLOB))),0) FROM assistant_workspaces)+(SELECT coalesce(sum(length(CAST(message AS BLOB))+length(CAST(rationale AS BLOB))+length(CAST(citations AS BLOB))),0) FROM assistant_proposals)+(SELECT coalesce(sum(length(CAST(id AS BLOB))+length(CAST(model_question AS BLOB))+length(CAST(user_reply AS BLOB))),0) FROM assistant_replies)+(SELECT coalesce(sum(length(CAST(name AS BLOB))+length(CAST(version AS BLOB))+length(CAST(package AS BLOB))),0) FROM plugins)+(SELECT coalesce(sum(length(CAST(definition AS BLOB))),0) FROM plugin_tools)+(SELECT coalesce(sum(length(CAST(plugin_name AS BLOB))+length(CAST(arguments AS BLOB))),0) FROM plugin_calls)",[],|r|r.get(0))?)
}
fn byte_room(c: &Connection, delta: i64) -> Result<(), StoreError> {
    if retained_bytes(c)? + delta > MAX_RETAINED_BYTES {
        Err(StoreError::ResourceLimit)
    } else {
        Ok(())
    }
}
fn insert_binding(
    c: &Connection,
    task: TaskId,
    id: EvidenceId,
    key: &SourceKey,
    version: Option<&str>,
) -> Result<(), StoreError> {
    c.execute("INSERT INTO source_bindings(task_id,evidence_id,digest,namespace,scope,object_id,version) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![task.0,id.0,key.digest()?.as_slice(),key.namespace,key.scope,key.object_id,version])?;
    Ok(())
}
fn record_decision(
    c: &Connection,
    task: TaskId,
    evidence: EvidenceId,
    accepted: bool,
    at: i64,
) -> Result<(), StoreError> {
    let existing: Option<i64> = c
        .query_row(
            "SELECT id FROM decisions WHERE task_id=?1 AND evidence_id=?2",
            params![task.0, evidence.0],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        c.execute(
            "UPDATE decisions SET accepted=?1,action_at_utc=?2 WHERE id=?3",
            params![accepted, at, id],
        )?;
    } else {
        reserve(c, "decisions", 1)?;
        c.execute(
            "INSERT INTO decisions(task_id,evidence_id,accepted,action_at_utc) VALUES(?1,?2,?3,?4)",
            params![task.0, evidence.0, accepted, at],
        )?;
    }
    Ok(())
}
fn page_args(after: i64, limit: usize) -> Result<(), StoreError> {
    if after < 0 || limit == 0 || limit > MAX_PAGE_ITEMS {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn collect_page<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
    limit: usize,
    id: impl Fn(&T) -> i64,
    size: impl Fn(&T) -> usize,
) -> Result<Page<T>, StoreError> {
    let mut page = Page {
        items: Vec::new(),
        next_after: None,
        payload_bytes: 40,
        task_revision: None,
        plan_completeness: None,
    };
    for row in rows {
        let value = row.map_err(|_| StoreError::UnsupportedSchema)?;
        let bytes = size(&value);
        if page.items.len() == limit || page.payload_bytes + bytes > MAX_PAGE_BYTES {
            page.next_after = page.items.last().map(&id);
            break;
        }
        page.payload_bytes += bytes;
        page.items.push(value);
    }
    Ok(page)
}
fn validate_database(c: &Connection) -> Result<(), StoreError> {
    let integrity: String = c.pragma_query_value(None, "quick_check", |r| r.get(0))?;
    if integrity != "ok" {
        return Err(StoreError::UnsupportedSchema);
    }
    pause(c)?;
    let foreign_bad: bool = c.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if foreign_bad {
        return Err(StoreError::UnsupportedSchema);
    }
    for table in [
        "tasks",
        "evidence",
        "source_bindings",
        "decisions",
        "plan_steps",
        "plan_checks",
        "plan_dependencies",
        "useful_references",
    ] {
        let count: i64 = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        if count > MAX_ROWS {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    let live: i64 = c.query_row(
        "SELECT count(*) FROM tasks WHERE text IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    if live > MAX_LIVE_TASKS || retained_bytes(c)? > MAX_RETAINED_BYTES {
        return Err(StoreError::UnsupportedSchema);
    }
    let inconsistent:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM evidence e JOIN tasks t ON e.task_id=t.id WHERE e.text IS NOT NULL AND t.text IS NULL) OR EXISTS(SELECT 1 FROM source_bindings b JOIN evidence e ON e.id=b.evidence_id WHERE b.namespace IS NOT NULL AND (e.text IS NULL OR e.origin!=1 OR e.task_id!=b.task_id)) OR EXISTS(SELECT 1 FROM evidence e WHERE e.text IS NOT NULL AND e.origin=1 AND (SELECT count(*) FROM source_bindings b WHERE b.evidence_id=e.id AND b.namespace IS NOT NULL)!=1) OR EXISTS(SELECT 1 FROM evidence e WHERE e.origin=0 AND EXISTS(SELECT 1 FROM source_bindings b WHERE b.evidence_id=e.id AND b.namespace IS NOT NULL)) OR EXISTS(SELECT 1 FROM decisions d JOIN tasks t ON d.task_id=t.id LEFT JOIN tasks o ON d.other_task_id=o.id LEFT JOIN evidence e ON d.evidence_id=e.id WHERE t.text IS NULL OR (o.id IS NOT NULL AND o.text IS NULL) OR (e.id IS NOT NULL AND e.text IS NULL)) OR EXISTS(SELECT 1 FROM decisions GROUP BY task_id,other_task_id,evidence_id HAVING count(*)>1)",[],|r|r.get(0))?;
    if inconsistent {
        return Err(StoreError::UnsupportedSchema);
    }
    let mut q=c.prepare("SELECT namespace,scope,object_id,version,digest FROM source_bindings WHERE namespace IS NOT NULL")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        let key = SourceKey {
            namespace: r.get(0)?,
            scope: r.get(1)?,
            object_id: r.get(2)?,
        };
        let version: Option<String> = r.get(3)?;
        let digest: Vec<u8> = r.get(4)?;
        if key
            .digest()
            .map_err(|_| StoreError::UnsupportedSchema)?
            .as_slice()
            != digest
            || version.as_ref().is_some_and(|v| key_field(v, 256).is_err())
        {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    plan::validate_plans(c)?;
    checks::validate_checks(c)?;
    sources::validate_roots(c).map_err(|_| StoreError::UnsupportedSchema)?;
    codex::validate_codex(c).map_err(|_| StoreError::UnsupportedSchema)?;
    integrations::validate_integrations(c).map_err(|_| StoreError::UnsupportedSchema)?;
    plugins::validate_plugins(c).map_err(|_| StoreError::UnsupportedSchema)?;
    proposals::validate_workspaces(c).map_err(|_| StoreError::UnsupportedSchema)?;
    observation::validate_observation(c).map_err(|_| StoreError::UnsupportedSchema)?;
    Ok(())
}

/// Whether `write_source(explicit = None)` would write this key for the task now: the same
/// forgotten-binding, user-edit and forgotten-elsewhere rules, kept next to it so they cannot
/// drift (plugin citability is promised from this answer and the publication writes later).
pub(super) fn implicit_write_allowed(
    c: &Connection,
    task: TaskId,
    key: &SourceKey,
) -> Result<bool, StoreError> {
    let digest = key.digest()?;
    let binding: Option<(i64, Option<String>)> = c
        .query_row(
            "SELECT evidence_id,namespace FROM source_bindings WHERE task_id=?1 AND digest=?2",
            params![task.0, digest.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match binding {
        Some((_, None)) => false,
        Some((id, Some(_))) => {
            owned_evidence(c, EvidenceId(id), task)?.kind != AssertionKind::UserConfirmation
        }
        None => !c.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_bindings WHERE digest=?1 AND namespace IS NULL)",
            [digest.as_slice()],
            |r| r.get::<_, bool>(0),
        )?,
    })
}
fn write_source(
    c: &Connection,
    owner: TaskVersion,
    key: &SourceKey,
    version: Option<&str>,
    input: &SourceEvidence,
    explicit: Option<i64>,
) -> Result<IngestOutcome, StoreError> {
    let task = owner.task_id;
    let expected = owner.revision;
    key.validate()?;
    if let Some(v) = version {
        key_field(v, 256)?;
    }
    text_field(&input.text)?;
    let digest = key.digest()?;
    plan::open_fence(c, task, expected)?;
    let binding: Option<(i64, Option<String>)> = c
        .query_row(
            "SELECT evidence_id,namespace FROM source_bindings WHERE task_id=?1 AND digest=?2",
            params![task.0, digest.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let id = if let Some((id, namespace)) = binding {
        if namespace.is_none() {
            return Ok(IngestOutcome::Forgotten);
        }
        let old = owned_evidence(c, EvidenceId(id), task)?;
        if explicit.is_none() && old.kind == AssertionKind::UserConfirmation {
            return Ok(IngestOutcome::UserProtected);
        }
        let old_version = match old.origin {
            Origin::External {
                key: old_key,
                version,
            } if old_key == *key => version,
            _ => return Err(StoreError::UnsupportedSchema),
        };
        byte_room(
            c,
            input.text.len() as i64 - old.text.len() as i64 + version.map_or(0, str::len) as i64
                - old_version.as_ref().map_or(0, String::len) as i64,
        )?;
        c.execute(
            "UPDATE source_bindings SET version=?1 WHERE task_id=?2 AND digest=?3",
            params![version, task.0, digest.as_slice()],
        )?;
        EvidenceId(id)
    } else {
        // A forgotten known key cannot acquire an implicit new task binding. Other live bindings above still update.
        let forgotten: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM source_bindings WHERE digest=?1 AND namespace IS NULL)",
            [digest.as_slice()],
            |r| r.get(0),
        )?;
        if explicit.is_none() && forgotten {
            return Ok(IngestOutcome::Forgotten);
        }
        reserve(c, "evidence", 1)?;
        reserve(c, "source_bindings", 1)?;
        byte_room(
            c,
            (input.text.len() + key.bytes() + version.map_or(0, str::len)) as i64,
        )?;
        c.execute("INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,1,?3,?4,?5)",params![task.0,input.text,input.observed_at_utc,input.completeness as u32,input.kind as u32])?;
        let id = EvidenceId(c.last_insert_rowid());
        insert_binding(c, task, id, key, version)?;
        id
    };
    // Binding confirmation belongs to decisions; source content remains refreshable until the user edits it.
    c.execute(
        "UPDATE evidence SET text=?1,observed_at_utc=?2,completeness=?3,kind=?4 WHERE id=?5",
        params![
            input.text,
            input.observed_at_utc,
            input.completeness as u32,
            input.kind as u32,
            id.0
        ],
    )?;
    if let Some(at) = explicit {
        record_decision(c, task, id, true, at)?;
    }
    bump(c, task)?;
    Ok(IngestOutcome::Applied(id))
}

fn impact_sql(note: bool, paged: bool) -> String {
    let condition = if note {
        "evidence_id=?2"
    } else {
        "task_id=?1 OR other_task_id=?1 OR evidence_id IN (SELECT id FROM evidence WHERE task_id=?1)"
    };
    format!(
        "SELECT id,revision,text,lifecycle,initiative_suppressed,plan_completeness FROM tasks WHERE id!=?1 AND text IS NOT NULL AND id IN (SELECT task_id FROM decisions WHERE {condition} UNION SELECT other_task_id FROM decisions WHERE {condition}){} ORDER BY id{}",
        if paged { " AND id>?3" } else { "" },
        if paged { " LIMIT ?4" } else { "" }
    )
}
fn fence_impact(
    c: &Connection,
    task: TaskId,
    evidence: Option<EvidenceId>,
    versions: &[TaskVersion],
) -> Result<(), StoreError> {
    if versions.len() > MAX_LIVE_TASKS as usize {
        return Err(StoreError::InvalidInput);
    }
    let mut supplied = std::collections::BTreeMap::new();
    for v in versions {
        if v.task_id.0 <= 0
            || v.revision == 0
            || v.revision > i64::MAX as u64
            || supplied.insert(v.task_id.0, v.revision).is_some()
        {
            return Err(StoreError::InvalidInput);
        }
    }
    let mut q = c.prepare(&impact_sql(evidence.is_some(), false))?;
    let expected: Vec<Task> = if evidence.is_some() {
        q.query_map(params![task.0, evidence.map_or(0, |id| id.0)], decode_task)?
            .collect::<Result<_, _>>()?
    } else {
        q.query_map([task.0], decode_task)?
            .collect::<Result<_, _>>()?
    };
    // Recompute the exact affected set under the write lock; paged previews cannot authorize stale cleanup.
    if expected.len() != supplied.len()
        || expected
            .iter()
            .any(|t| supplied.get(&t.id.0) != Some(&t.revision))
    {
        return Err(StoreError::StaleRevision);
    }
    for t in expected {
        bump(c, t.id)?;
    }
    Ok(())
}

fn inspect_existing_schema(path: &Path) -> Result<(), StoreError> {
    let absolute = std::path::absolute(path).map_err(|_| StoreError::Storage)?;
    let mut uri = url::Url::from_file_path(absolute).map_err(|_| StoreError::Storage)?;
    uri.query_pairs_mut()
        .append_pair("mode", "ro")
        .append_pair("readonly_shm", "1");
    // SQLite's bundled VFS reads the logical WAL schema without writing SHM read marks.
    // Missing SHM fails closed; this admission check must never recover a rejected database.
    let probe = Connection::open_with_flags(
        uri.as_str(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    inspect_schema(&probe)?;
    Ok(())
}
fn inspect_schema(c: &Connection) -> Result<bool, StoreError> {
    let version: i32 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let identity: i32 = c.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let objects: Vec<(String, String)> = {
        let mut q = c.prepare(
            "SELECT name,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
        )?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    if version == 0 && identity == 0 && objects.is_empty() {
        return Ok(true);
    }
    let mut expected: Vec<_> = SCHEMA
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            (
                s.split_whitespace()
                    .nth(if s.starts_with("CREATE UNIQUE INDEX") {
                        3
                    } else {
                        2
                    })
                    .unwrap_or("")
                    .to_owned(),
                s.to_owned(),
            )
        })
        .collect();
    expected.sort();
    if version != 17 || identity != APPLICATION_ID || objects != expected {
        return Err(StoreError::UnsupportedSchema);
    }
    validate_database(c)?;
    Ok(false)
}

#[cfg(test)]
#[path = "assistance_tests.rs"]
mod assistance_tests;
