use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn decode_completeness(v: i64) -> rusqlite::Result<Completeness> {
    match v {
        0 => Ok(Completeness::Unknown),
        1 => Ok(Completeness::Partial),
        2 => Ok(Completeness::Complete),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
pub(super) fn open_fence(c: &Connection, task: TaskId, expected: u64) -> Result<Task, StoreError> {
    let t = fence(c, task, expected)?;
    if t.lifecycle == TaskLifecycle::Completed {
        Err(StoreError::TaskCompleted)
    } else {
        Ok(t)
    }
}
fn step_input(input: &StepInput) -> Result<(), StoreError> {
    text_field(&input.description)?;
    if let Some(s) = &input.prompt {
        text_field(s)?;
    }
    if let Some(s) = &input.reference {
        key_field(s, 2048)?;
    }
    Ok(())
}
fn step(c: &Connection, task: TaskId, id: StepId) -> Result<(), StoreError> {
    if id.0 <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let row: Option<(i64, bool)> = c
        .query_row(
            "SELECT task_id,description IS NOT NULL FROM plan_steps WHERE id=?1",
            [id.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Err(StoreError::NotFound),
        Some((_, false)) => Err(StoreError::Deleted),
        Some((t, true)) if t == task.0 => Ok(()),
        _ => Err(StoreError::InvalidInput),
    }
}
fn check_step(c: &Connection, task: TaskId, id: CheckId) -> Result<StepId, StoreError> {
    if id.0 <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let row: Option<(i64, i64, bool)> = c
        .query_row(
            "SELECT task_id,step_id,description IS NOT NULL FROM plan_checks WHERE id=?1",
            [id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match row {
        None => Err(StoreError::NotFound),
        Some((_, _, false)) => Err(StoreError::Deleted),
        Some((t, s, true)) if t == task.0 => Ok(StepId(s)),
        _ => Err(StoreError::InvalidInput),
    }
}
fn partial(c: &Connection, task: TaskId) -> Result<(), StoreError> {
    c.execute("UPDATE tasks SET plan_completeness=1 WHERE id=?1", [task.0])?;
    Ok(())
}
fn room(c: &Connection, sql: &str, id: i64, max: i64) -> Result<(), StoreError> {
    let n: i64 = c.query_row(sql, [id], |r| r.get(0))?;
    if n >= max {
        Err(StoreError::ResourceLimit)
    } else {
        Ok(())
    }
}
fn descendants(c: &Connection, task: TaskId, root: StepId) -> Result<Vec<i64>, StoreError> {
    // UNION deduplicates a single ID; 65 is a refusal sentinel, never evidence that a truncated traversal is acyclic.
    let mut q=c.prepare("WITH RECURSIVE descendants(id) AS (SELECT id FROM plan_steps WHERE id=?2 AND task_id=?1 AND description IS NOT NULL UNION SELECT d.dependent FROM plan_dependencies d JOIN descendants p ON d.predecessor=p.id WHERE d.task_id=?1 AND d.predecessor IS NOT NULL LIMIT 65) SELECT id FROM descendants")?;
    let ids = q
        .query_map(params![task.0, root.0], |r| r.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    if ids.len() > 64 {
        return Err(StoreError::ResourceLimit);
    }
    Ok(ids)
}
fn invalidate(c: &Connection, task: TaskId, root: StepId) -> Result<(), StoreError> {
    for id in descendants(c, task, root)? {
        c.execute(
            "UPDATE plan_steps SET completed_at_utc=NULL WHERE id=?1",
            [id],
        )?;
        c.execute("UPDATE plan_checks SET result=0,confirmed_at_utc=NULL WHERE step_id=?1 AND description IS NOT NULL",[id])?;
    }
    Ok(())
}
pub(super) fn invalidate_all(c: &Connection, task: TaskId) -> Result<(), StoreError> {
    partial(c, task)?;
    c.execute(
        "UPDATE plan_steps SET completed_at_utc=NULL WHERE task_id=?1",
        [task.0],
    )?;
    c.execute("UPDATE plan_checks SET result=0,confirmed_at_utc=NULL WHERE task_id=?1 AND description IS NOT NULL",[task.0])?;
    Ok(())
}
pub(super) fn clear_plan(c: &Connection, task: TaskId) -> Result<(), StoreError> {
    c.execute(
        "UPDATE plan_dependencies SET predecessor=NULL,dependent=NULL WHERE task_id=?1",
        [task.0],
    )?;
    c.execute("UPDATE plan_checks SET description=NULL,result=NULL,confirmed_at_utc=NULL WHERE task_id=?1",[task.0])?;
    c.execute("UPDATE plan_steps SET description=NULL,prompt=NULL,reference=NULL,completed_at_utc=NULL WHERE task_id=?1",[task.0])?;
    c.execute("UPDATE tasks SET plan_completeness=0 WHERE id=?1", [task.0])?;
    Ok(())
}
pub(super) fn predecessors_done(
    c: &Connection,
    task: TaskId,
    id: StepId,
) -> Result<bool, StoreError> {
    Ok(c.query_row("SELECT NOT EXISTS(SELECT 1 FROM plan_dependencies d JOIN plan_steps p ON p.id=d.predecessor WHERE d.task_id=?1 AND d.dependent=?2 AND d.predecessor IS NOT NULL AND (p.completed_at_utc IS NULL OR EXISTS(SELECT 1 FROM plan_checks k WHERE k.step_id=p.id AND k.description IS NOT NULL AND k.result!=1)))",params![task.0,id.0],|r|r.get(0))?)
}
pub(super) fn plan_page<T>(mut page: Page<T>, task: &Task) -> Page<T> {
    page.task_revision = Some(task.revision);
    page.plan_completeness = Some(task.plan_completeness);
    page
}
impl Store {
    pub fn set_plan_completeness(
        &mut self,
        task: TaskId,
        expected: u64,
        value: Completeness,
    ) -> Result<(), StoreError> {
        self.change(|c| {
            open_fence(c, task, expected)?;
            c.execute(
                "UPDATE tasks SET plan_completeness=?1 WHERE id=?2",
                params![value as u32, task.0],
            )?;
            bump(c, task)
        })
    }
    pub fn add_step(
        &mut self,
        task: TaskId,
        expected: u64,
        input: &StepInput,
    ) -> Result<StepId, StoreError> {
        step_input(input)?;
        self.change(|c| {
            open_fence(c, task, expected)?;
            room(
                c,
                "SELECT count(*) FROM plan_steps WHERE task_id=?1 AND description IS NOT NULL",
                task.0,
                64,
            )?;
            reserve(c, "plan_steps", 1)?;
            byte_room(
                c,
                (input.description.len()
                    + input.prompt.as_ref().map_or(0, String::len)
                    + input.reference.as_ref().map_or(0, String::len)) as i64,
            )?;
            c.execute(
                "INSERT INTO plan_steps(task_id,description,prompt,reference) VALUES(?1,?2,?3,?4)",
                params![task.0, input.description, input.prompt, input.reference],
            )?;
            let id = StepId(c.last_insert_rowid());
            partial(c, task)?;
            bump(c, task)?;
            Ok(id)
        })
    }
    pub fn edit_step(
        &mut self,
        task: TaskId,
        expected: u64,
        id: StepId,
        input: &StepInput,
    ) -> Result<(), StoreError> {
        step_input(input)?;
        self.change(|c| {
            open_fence(c, task, expected)?;
            step(c, task, id)?;
            c.execute(
                "UPDATE plan_steps SET description=?1,prompt=?2,reference=?3 WHERE id=?4",
                params![input.description, input.prompt, input.reference, id.0],
            )?;
            byte_room(c, 0)?;
            invalidate(c, task, id)?;
            partial(c, task)?;
            bump(c, task)
        })
    }
    pub fn delete_step(
        &mut self,
        task: TaskId,
        expected: u64,
        id: StepId,
    ) -> Result<(), StoreError> {
        self.change(|c|{open_fence(c,task,expected)?;step(c,task,id)?;
            let used:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM plan_dependencies WHERE predecessor IS NOT NULL AND (predecessor=?1 OR dependent=?1))",[id.0],|r|r.get(0))?;
            if used { return Err(StoreError::InvalidInput); }
            c.execute("UPDATE plan_checks SET description=NULL,result=NULL,confirmed_at_utc=NULL WHERE step_id=?1",[id.0])?;
            c.execute("UPDATE plan_steps SET description=NULL,prompt=NULL,reference=NULL,completed_at_utc=NULL WHERE id=?1",[id.0])?;partial(c,task)?;bump(c,task)})
    }
    pub fn set_step_completed(
        &mut self,
        task: TaskId,
        expected: u64,
        id: StepId,
        at: Option<i64>,
    ) -> Result<(), StoreError> {
        self.change(|c|{open_fence(c,task,expected)?;step(c,task,id)?;
            if at.is_some() {
                let own:bool=c.query_row("SELECT NOT EXISTS(SELECT 1 FROM plan_checks WHERE step_id=?1 AND description IS NOT NULL AND result!=1)",[id.0],|r|r.get(0))?;
                if !own || !predecessors_done(c,task,id)? { return Err(StoreError::InvalidInput); }
                c.execute("UPDATE plan_steps SET completed_at_utc=?1 WHERE id=?2",params![at,id.0])?;
            } else { invalidate(c,task,id)?; }
            bump(c,task)})
    }
    pub fn add_check(
        &mut self,
        task: TaskId,
        expected: u64,
        id: StepId,
        description: &str,
    ) -> Result<CheckId, StoreError> {
        text_field(description)?;
        self.change(|c| {
            open_fence(c, task, expected)?;
            step(c, task, id)?;
            room(
                c,
                "SELECT count(*) FROM plan_checks WHERE step_id=?1 AND description IS NOT NULL",
                id.0,
                8,
            )?;
            reserve(c, "plan_checks", 1)?;
            byte_room(c, description.len() as i64)?;
            c.execute(
                "INSERT INTO plan_checks(task_id,step_id,description,result) VALUES(?1,?2,?3,0)",
                params![task.0, id.0, description],
            )?;
            let check = CheckId(c.last_insert_rowid());
            invalidate(c, task, id)?;
            partial(c, task)?;
            bump(c, task)?;
            Ok(check)
        })
    }
    pub fn edit_check(
        &mut self,
        task: TaskId,
        expected: u64,
        id: CheckId,
        description: &str,
    ) -> Result<(), StoreError> {
        text_field(description)?;
        self.change(|c| {
            open_fence(c, task, expected)?;
            let step = check_step(c, task, id)?;
            c.execute(
                "UPDATE plan_checks SET description=?1 WHERE id=?2",
                params![description, id.0],
            )?;
            byte_room(c, 0)?;
            invalidate(c, task, step)?;
            partial(c, task)?;
            bump(c, task)
        })
    }
    pub fn delete_check(
        &mut self,
        task: TaskId,
        expected: u64,
        id: CheckId,
    ) -> Result<(), StoreError> {
        self.change(|c|{open_fence(c,task,expected)?;let step=check_step(c,task,id)?;c.execute("UPDATE plan_checks SET description=NULL,result=NULL,confirmed_at_utc=NULL WHERE id=?1",[id.0])?;invalidate(c,task,step)?;partial(c,task)?;bump(c,task)})
    }
    pub fn set_check_result(
        &mut self,
        task: TaskId,
        expected: u64,
        id: CheckId,
        result: CheckResult,
        at: Option<i64>,
    ) -> Result<(), StoreError> {
        if (result == CheckResult::Unknown) != at.is_none() {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c| {
            open_fence(c, task, expected)?;
            let step = check_step(c, task, id)?;
            let old: i64 =
                c.query_row("SELECT result FROM plan_checks WHERE id=?1", [id.0], |r| {
                    r.get(0)
                })?;
            if old != result as i64 && result != CheckResult::Passed {
                invalidate(c, task, step)?;
            }
            // Keep the user's explicit failed result after invalidating the other confirmations.
            c.execute(
                "UPDATE plan_checks SET result=?1,confirmed_at_utc=?2 WHERE id=?3",
                params![result as u32, at, id.0],
            )?;
            bump(c, task)
        })
    }
    pub fn add_dependency(
        &mut self,
        task: TaskId,
        expected: u64,
        predecessor: StepId,
        dependent: StepId,
    ) -> Result<DependencyId, StoreError> {
        if predecessor == dependent {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c|{open_fence(c,task,expected)?;step(c,task,predecessor)?;step(c,task,dependent)?;
            if descendants(c,task,dependent)?.contains(&predecessor.0) { return Err(StoreError::InvalidInput); }
            let duplicate:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM plan_dependencies WHERE predecessor=?1 AND dependent=?2 AND predecessor IS NOT NULL)",params![predecessor.0,dependent.0],|r|r.get(0))?;
            if duplicate {return Err(StoreError::InvalidInput);}
            room(c,"SELECT count(*) FROM plan_dependencies WHERE task_id=?1 AND predecessor IS NOT NULL",task.0,512)?;reserve(c,"plan_dependencies",1)?;
            c.execute("INSERT INTO plan_dependencies(task_id,predecessor,dependent) VALUES(?1,?2,?3)",params![task.0,predecessor.0,dependent.0])?;let id=DependencyId(c.last_insert_rowid());invalidate(c,task,dependent)?;partial(c,task)?;bump(c,task)?;Ok(id)})
    }
    pub fn delete_dependency(
        &mut self,
        task: TaskId,
        expected: u64,
        id: DependencyId,
    ) -> Result<(), StoreError> {
        if id.0 <= 0 {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c|{open_fence(c,task,expected)?;
            let row:Option<(i64,i64,bool)>=c.query_row("SELECT task_id,coalesce(dependent,0),predecessor IS NOT NULL FROM plan_dependencies WHERE id=?1",[id.0],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let dependent=match row {None=>return Err(StoreError::NotFound),Some((_,_,false))=>return Err(StoreError::Deleted),Some((t,d,true)) if t==task.0=>StepId(d),_=>return Err(StoreError::InvalidInput)};
            c.execute("UPDATE plan_dependencies SET predecessor=NULL,dependent=NULL WHERE id=?1",[id.0])?;invalidate(c,task,dependent)?;partial(c,task)?;bump(c,task)})
    }
    pub fn steps(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<PlanStep>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let t = read_task(&c, task)?;
        let mut q=c.prepare("SELECT id,description,prompt,reference,completed_at_utc FROM plan_steps WHERE task_id=?1 AND description IS NOT NULL AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], |r| {
            Ok(PlanStep {
                id: StepId(r.get(0)?),
                task_id: task,
                description: r.get(1)?,
                prompt: r.get(2)?,
                reference: r.get(3)?,
                completed_at_utc: r.get(4)?,
                ready: false,
            })
        })?;
        let mut p = collect_page(
            rows,
            limit,
            |s| s.id.0,
            |s| {
                96 + s.description.len()
                    + s.prompt.as_ref().map_or(0, String::len)
                    + s.reference.as_ref().map_or(0, String::len)
            },
        )?;
        for s in &mut p.items {
            s.ready = t.plan_completeness == Completeness::Complete
                && s.completed_at_utc.is_none()
                && predecessors_done(&c, task, s.id)?;
        }
        Ok(plan_page(p, &t))
    }
    pub fn checks(
        &self,
        task: TaskId,
        id: StepId,
        after: i64,
        limit: usize,
    ) -> Result<Page<PlanCheck>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let t = read_task(&c, task)?;
        step(&c, task, id)?;
        let mut q=c.prepare("SELECT id,description,result,confirmed_at_utc FROM plan_checks WHERE step_id=?1 AND description IS NOT NULL AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = q.query_map(params![id.0, after, (limit + 1) as i64], |r| {
            Ok(PlanCheck {
                id: CheckId(r.get(0)?),
                task_id: task,
                step_id: id,
                description: r.get(1)?,
                result: match r.get::<_, i64>(2)? {
                    0 => CheckResult::Unknown,
                    1 => CheckResult::Passed,
                    2 => CheckResult::Failed,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                },
                confirmed_at_utc: r.get(3)?,
            })
        })?;
        Ok(plan_page(
            collect_page(rows, limit, |k| k.id.0, |k| 64 + k.description.len())?,
            &t,
        ))
    }
    pub fn dependencies(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<PlanDependency>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let t = read_task(&c, task)?;
        let mut q=c.prepare("SELECT id,predecessor,dependent FROM plan_dependencies WHERE task_id=?1 AND predecessor IS NOT NULL AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], |r| {
            Ok(PlanDependency {
                id: DependencyId(r.get(0)?),
                task_id: task,
                predecessor: StepId(r.get(1)?),
                dependent: StepId(r.get(2)?),
            })
        })?;
        Ok(plan_page(
            collect_page(rows, limit, |d| d.id.0, |_| 32)?,
            &t,
        ))
    }
    pub fn references(
        &self,
        task: TaskId,
        after: i64,
        limit: usize,
    ) -> Result<Page<UsefulReference>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let t = read_task(&c, task)?;
        let mut q=c.prepare("SELECT id,text FROM useful_references WHERE task_id=?1 AND text IS NOT NULL AND id>?2 ORDER BY id LIMIT ?3")?;
        let rows = q.query_map(params![task.0, after, (limit + 1) as i64], |r| {
            Ok(UsefulReference {
                id: ReferenceId(r.get(0)?),
                task_id: task,
                text: r.get(1)?,
            })
        })?;
        Ok(plan_page(
            collect_page(rows, limit, |r| r.id.0, |r| 24 + r.text.len())?,
            &t,
        ))
    }
    pub fn set_initiative_suppressed(
        &mut self,
        task: TaskId,
        expected: u64,
        value: bool,
    ) -> Result<(), StoreError> {
        self.change(|c| {
            fence(c, task, expected)?;
            c.execute(
                "UPDATE tasks SET initiative_suppressed=?1 WHERE id=?2",
                params![value, task.0],
            )?;
            bump(c, task)
        })
    }
    pub fn reopen_task(&mut self, task: TaskId, expected: u64) -> Result<(), StoreError> {
        self.change(|c| {
            let t = fence(c, task, expected)?;
            if t.lifecycle != TaskLifecycle::Completed {
                return Err(StoreError::InvalidInput);
            }
            c.execute(
                "UPDATE tasks SET lifecycle=0,plan_completeness=0 WHERE id=?1",
                [task.0],
            )?;
            bump(c, task)
        })
    }
    pub fn completion_impact(
        &self,
        task: TaskId,
        keep: &[TaskId],
        after: i64,
        limit: usize,
    ) -> Result<Page<Task>, StoreError> {
        page_args(after, limit)?;
        let c = self.connection.unchecked_transaction()?;
        let t = read_task(&c, task)?;
        let (sql, args) = completion_predicate(&c, task, keep)?;
        let mut q=c.prepare(&format!("SELECT id,revision,text,lifecycle,initiative_suppressed,plan_completeness FROM tasks WHERE id!=?1 AND text IS NOT NULL AND id IN (SELECT task_id FROM decisions WHERE {sql} UNION SELECT other_task_id FROM decisions WHERE {sql}) AND id>?{} ORDER BY id LIMIT ?{}",args.len()+1,args.len()+2))?;
        let mut args = args;
        args.push(after);
        args.push((limit + 1) as i64);
        let rows = q.query_map(rusqlite::params_from_iter(args), decode_task)?;
        Ok(plan_page(
            collect_page(rows, limit, |t| t.id.0, |t| 40 + t.text.len())?,
            &t,
        ))
    }
    pub fn complete_task(
        &mut self,
        task: TaskId,
        expected: u64,
        summary: &str,
        keep: &[TaskId],
        references: &[String],
        others: &[TaskVersion],
    ) -> Result<(), StoreError> {
        text_field(summary)?;
        if references.len() > 32 {
            return Err(StoreError::ResourceLimit);
        }
        for r in references {
            key_field(r, 2048)?;
        }
        self.change(|c|{open_fence(c,task,expected)?;let (predicate,args)=completion_predicate(c,task,keep)?;
            let mut q=c.prepare(&format!("SELECT id,revision FROM tasks WHERE id!=?1 AND text IS NOT NULL AND id IN (SELECT task_id FROM decisions WHERE {predicate} UNION SELECT other_task_id FROM decisions WHERE {predicate}) ORDER BY id LIMIT 1024"))?;
            let affected=q.query_map(rusqlite::params_from_iter(args.iter()),|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)? as u64)))?.collect::<Result<BTreeMap<_,_>,_>>()?;
            if affected.len()>1023 || others.len()>1023 {return Err(StoreError::ResourceLimit);}
            let mut supplied=BTreeMap::new();for v in others {if v.task_id.0<=0 || v.revision==0 || v.revision>i64::MAX as u64 || supplied.insert(v.task_id.0,v.revision).is_some() {return Err(StoreError::InvalidInput);}}
            if supplied!=affected {return Err(StoreError::StaleRevision);}
            for id in affected.keys() {bump(c,TaskId(*id))?;}
            c.execute(&format!("DELETE FROM decisions WHERE {predicate}"),rusqlite::params_from_iter(args))?;
            c.execute("DELETE FROM codex_metadata WHERE binding_id IN (SELECT id FROM source_bindings WHERE task_id=?1)",[task.0])?;
            c.execute("DELETE FROM integration_metadata WHERE binding_id IN (SELECT id FROM source_bindings WHERE task_id=?1)",[task.0])?;
            c.execute("UPDATE source_bindings SET namespace=NULL,scope=NULL,object_id=NULL,version=NULL WHERE task_id=?1",[task.0])?;
            c.execute("UPDATE evidence SET text=NULL,origin=NULL,observed_at_utc=NULL,completeness=NULL,kind=NULL WHERE task_id=?1",[task.0])?;
            clear_plan(c,task)?;
            c.execute("UPDATE useful_references SET text=NULL WHERE task_id=?1",[task.0])?;
            reserve(c,"useful_references",references.len() as i64)?;
            for r in references {c.execute("INSERT INTO useful_references(task_id,text) VALUES(?1,?2)",params![task.0,r])?;}
            c.execute("UPDATE tasks SET text=?1,lifecycle=1 WHERE id=?2",params![summary,task.0])?;
            // Derived discussion is erased in this same transaction before counting final retained content.
            proposals::clear_revoked(c)?;
            // Account for the final retained content after cleanup; rollback restores all original details on failure.
            byte_room(c,0)?;bump(c,task)})
    }
}
fn completion_predicate(
    c: &Connection,
    task: TaskId,
    keep: &[TaskId],
) -> Result<(String, Vec<i64>), StoreError> {
    if keep.len() > 1023 {
        return Err(StoreError::ResourceLimit);
    }
    let mut unique = BTreeSet::new();
    let mut args = vec![task.0];
    for id in keep {
        if id.0 <= 0 || *id == task || !unique.insert(id.0) {
            return Err(StoreError::InvalidInput);
        }
        let linked:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM decisions WHERE accepted=1 AND ((task_id=?1 AND other_task_id=?2) OR (task_id=?2 AND other_task_id=?1)))",params![task.0,id.0],|r|r.get(0))?;
        if !linked {
            return Err(StoreError::InvalidInput);
        }
        args.push(id.0);
    }
    let retained = if keep.is_empty() {
        String::new()
    } else {
        format!(
            " AND CASE WHEN task_id=?1 THEN other_task_id ELSE task_id END NOT IN ({})",
            (2..=args.len())
                .map(|n| format!("?{n}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    Ok((
        format!(
            "((other_task_id IS NOT NULL AND (task_id=?1 OR other_task_id=?1){retained}) OR (evidence_id IS NOT NULL AND (task_id=?1 OR evidence_id IN (SELECT id FROM evidence WHERE task_id=?1))))"
        ),
        args,
    ))
}
pub(super) fn validate_plans(c: &Connection) -> Result<(), StoreError> {
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM plan_steps s JOIN tasks t ON s.task_id=t.id WHERE s.description IS NOT NULL AND (t.text IS NULL OR t.lifecycle=1)) OR EXISTS(SELECT 1 FROM plan_checks k JOIN plan_steps s ON k.step_id=s.id WHERE k.description IS NOT NULL AND s.description IS NULL) OR EXISTS(SELECT 1 FROM plan_dependencies d JOIN plan_steps a ON a.id=d.predecessor JOIN plan_steps b ON b.id=d.dependent WHERE d.predecessor IS NOT NULL AND (a.description IS NULL OR b.description IS NULL)) OR EXISTS(SELECT 1 FROM useful_references r JOIN tasks t ON t.id=r.task_id WHERE r.text IS NOT NULL AND t.text IS NULL) OR EXISTS(SELECT 1 FROM evidence e JOIN tasks t ON t.id=e.task_id WHERE e.text IS NOT NULL AND t.lifecycle=1) OR EXISTS(SELECT task_id FROM plan_steps WHERE description IS NOT NULL GROUP BY task_id HAVING count(*)>64) OR EXISTS(SELECT step_id FROM plan_checks WHERE description IS NOT NULL GROUP BY step_id HAVING count(*)>8) OR EXISTS(SELECT task_id FROM plan_dependencies WHERE predecessor IS NOT NULL GROUP BY task_id HAVING count(*)>512) OR EXISTS(SELECT task_id FROM useful_references WHERE text IS NOT NULL GROUP BY task_id HAVING count(*)>32)",[],|r|r.get(0))?;
    if invalid {
        return Err(StoreError::UnsupportedSchema);
    }
    let mut q = c.prepare(
        "SELECT id,task_id,completed_at_utc FROM plan_steps WHERE description IS NOT NULL",
    )?;
    let rows = q.query_map([], |r| {
        Ok((
            StepId(r.get(0)?),
            TaskId(r.get(1)?),
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, task, completed) = row?;
        for child in descendants(c, task, id)? {
            if child!=id.0 && c.query_row("SELECT EXISTS(SELECT 1 FROM plan_dependencies WHERE predecessor=?1 AND dependent=?2 AND predecessor IS NOT NULL)",params![child,id.0],|r|r.get::<_,bool>(0))? {return Err(StoreError::UnsupportedSchema);}
        }
        if completed.is_some() {
            let own:bool=c.query_row("SELECT NOT EXISTS(SELECT 1 FROM plan_checks WHERE step_id=?1 AND description IS NOT NULL AND result!=1)",[id.0],|r|r.get(0))?;
            if !own || !predecessors_done(c, task, id)? {
                return Err(StoreError::UnsupportedSchema);
            }
        }
    }
    Ok(())
}
