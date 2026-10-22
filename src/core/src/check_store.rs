use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
// Fourteen 8-byte integers plus record/rowid/cell headers fit a conservative 160-byte row charge.
const ROW_BYTES: i64 = 160;
// Live rows only; older completed rows are folded into one totals row per purpose.
const MAX_CHECKS: i64 = 4096;
const MAX_CHECK_BYTES: i64 = 1024 * 1024;
impl Store {
    pub fn prepare_check(
        &mut self,
        user_reported: bool,
        profile: ModelProfile,
    ) -> Result<CheckAdmission, StoreError> {
        if !user_reported {
            return Err(StoreError::ReportRequired);
        }
        self.reserve_model(ModelPurpose::Connectivity, profile, None)
    }
    pub(super) fn reserve_model(
        &mut self,
        purpose: ModelPurpose,
        profile: ModelProfile,
        request_id: Option<u64>,
    ) -> Result<CheckAdmission, StoreError> {
        if self.manual_pause()? {
            return Err(StoreError::Paused);
        }
        if self.active_check.is_some() {
            return Err(StoreError::Busy);
        }
        let cancelled = if purpose == ModelPurpose::Assistance {
            self.current_snapshot(request_id.ok_or(StoreError::InvalidInput)?)?
                .cancelled
                .clone()
        } else {
            Arc::new(AtomicBool::new(false))
        };
        let epoch = self.model_session.epoch;
        self.active_check = Some(ActiveModel {
            cancelled: cancelled.clone(),
            purpose,
            profile,
            epoch,
            request_id,
            attempt: None,
            observation: None,
        });
        Ok(CheckAdmission {
            cancelled,
            purpose,
            profile,
            epoch,
            request_id,
        })
    }
    pub fn cancel_check(&self) {
        if let Some(active) = &self.active_check {
            active.cancelled.store(true, Ordering::Release)
        }
    }
    pub fn release_check(&mut self, flag: &Arc<AtomicBool>) {
        if self
            .active_check
            .as_ref()
            .is_some_and(|v| Arc::ptr_eq(&v.cancelled, flag))
        {
            self.active_check = None;
        }
    }
    /// Consuming wrapper for the single-POST connectivity path.
    pub fn dispatch_check(
        &mut self,
        admission: CheckAdmission,
        at_utc: i64,
    ) -> Result<CheckDispatch, StoreError> {
        let (id, snapshot) = self.dispatch_attempt(&admission, at_utc)?;
        let observation = self
            .active_check
            .as_ref()
            .and_then(|a| a.observation.clone());
        Ok(CheckDispatch {
            purpose: admission.purpose,
            profile: admission.profile,
            snapshot,
            observation,
            id,
            cancelled: admission.cancelled,
        })
    }
    /// Admits one POST of the admission it borrows: every POST of a plugin loop passes the same
    /// pause, cancel, session and snapshot fences and commits its possible-send row first.
    pub(super) fn dispatch_attempt(
        &mut self,
        admission: &CheckAdmission,
        at_utc: i64,
    ) -> Result<(AttemptId, Option<Arc<AssistanceSnapshot>>), StoreError> {
        if !self
            .active_check
            .as_ref()
            .is_some_and(|v| Arc::ptr_eq(&v.cancelled, &admission.cancelled))
        {
            return Err(StoreError::InvalidInput);
        }
        let snapshot = if admission.purpose == ModelPurpose::Assistance {
            self.require_assistance()?;
            let snapshot =
                self.current_snapshot(admission.request_id.ok_or(StoreError::InvalidInput)?)?;
            if snapshot.epoch != admission.epoch {
                return Err(StoreError::Cancelled);
            }
            Some(snapshot)
        } else {
            None
        };
        let observation = if admission.purpose == ModelPurpose::Observation {
            self.require_observation(admission.epoch)?;
            Some(
                self.active_check
                    .as_ref()
                    .and_then(|a| a.observation.clone())
                    .ok_or(StoreError::InvalidInput)?,
            )
        } else {
            None
        };
        let id =
            self.change(|c| {
                if pause(c)? {
                    return Err(StoreError::Paused);
                }
                if admission.cancelled.load(Ordering::Acquire) {
                    return Err(StoreError::Cancelled);
                }
                let (count, maximum): (i64, i64) = c.query_row(
                    "SELECT count(*),coalesce(max(id),0) FROM connection_checks",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if count >= MAX_CHECKS {
                    fold(c, count - MAX_CHECKS + 1)?;
                }
                if (count.min(MAX_CHECKS - 1) + 1)
                    .checked_mul(ROW_BYTES)
                    .is_none_or(|v| v > MAX_CHECK_BYTES)
                    || maximum == i64::MAX
                {
                    return Err(StoreError::ResourceLimit);
                }
                let id = maximum + 1;
                c.execute(
                "INSERT INTO connection_checks VALUES(?1,?2,?3,1,0,1,?4,1,0,0,0,NULL,NULL,NULL)",
                params![id, admission.purpose as u32, at_utc,admission.profile as u32],
            )?;
                if let Some(request) = &observation {
                    observation::reserve_send(c, AttemptId(id), request, at_utc)?;
                }
                Ok(AttemptId(id))
            })?;
        // This committed possible-send record is the dispatch acceptance point, serialized with pause.
        if let Some(active) = &mut self.active_check {
            active.attempt = Some(id);
        }
        if let Some(request) = &observation {
            self.observation_sent(request);
        }
        Ok((id, snapshot))
    }
    pub fn finish_check(
        &mut self,
        id: AttemptId,
        result: ModelCheckResult,
    ) -> Result<ModelCheckResult, StoreError> {
        self.finish_attempt(id, result, |_, _| Ok(()))
            .map(|(result, _)| result)
    }
    /// Completes the attempt row; `apply` writes in the same transaction with the final result, so
    /// an observation answer and its send journal commit together with the accounting.
    pub(super) fn finish_attempt<T>(
        &mut self,
        id: AttemptId,
        mut result: ModelCheckResult,
        apply: impl FnOnce(&Connection, &ModelCheckResult) -> Result<T, StoreError>,
    ) -> Result<(ModelCheckResult, T), StoreError> {
        if result.outcome == CheckOutcome::Pending {
            return Err(StoreError::InvalidInput);
        }
        let active = self
            .active_check
            .as_ref()
            .filter(|a| a.attempt == Some(id))
            .ok_or(StoreError::InvalidInput)?;
        let (purpose, profile, epoch, cancelled, observation) = (
            active.purpose,
            active.profile,
            active.epoch,
            active.cancelled.clone(),
            active.observation.clone(),
        );
        if cancelled.load(Ordering::Acquire) {
            result.outcome = CheckOutcome::Cancelled;
        }
        if result.outcome == CheckOutcome::Confirmed && !result.actual.confirmed(profile) {
            result.outcome = CheckOutcome::Mismatch;
        }
        // An old completion can account usage, but cannot revoke or confirm a newer connection.
        // Observation failures never revoke manual assistance; only session errors do (D103).
        if purpose != ModelPurpose::Observation
            && epoch == self.model_session.epoch
            && result.outcome != CheckOutcome::Confirmed
            && result.outcome != CheckOutcome::Cancelled
        {
            self.cancel_all_reads();
            self.high_capability = None;
            self.prepared = None;
        }
        let commit = self.change(|c| {
   // BEGIN may wait for another writer; observe cancellation again after acquiring the write transaction.
   if cancelled.load(Ordering::Acquire) {result.outcome=CheckOutcome::Cancelled;}
   let changed=c.execute("UPDATE connection_checks SET stage=2,outcome=?1,actual_model=?2,actual_effort=?3,actual_tier=?4,input_tokens=?5,output_tokens=?6,total_tokens=?7 WHERE id=?8 AND stage=1",params![result.outcome as u32,result.actual.model as u32,result.actual.effort as u32,result.actual.tier as u32,result.usage.map(|v|v.input() as i64),result.usage.map(|v|v.output() as i64),result.usage.map(|v|v.total() as i64),id.0])?;
   if changed!=1 {return Err(StoreError::InvalidInput)}
   if let Some(request) = &observation {
       observation::settle_send(c, id, &result, request)?;
   }
   let applied=apply(c,&result)?;
   // Validate checked aggregates before committing; overflow leaves the durable unknown intact.
   summary(c)?; Ok(applied)
  });
        let applied = match commit {
            Ok(applied) => applied,
            Err(error) => {
                if purpose != ModelPurpose::Observation && epoch == self.model_session.epoch {
                    self.cancel_all_reads();
                    self.high_capability = None;
                    self.prepared = None;
                }
                return Err(error);
            }
        };
        // An observation result stays as committed, so the caller and the send journal agree.
        if purpose != ModelPurpose::Observation && cancelled.load(Ordering::Acquire) {
            result.outcome = CheckOutcome::Cancelled;
        }
        if purpose == ModelPurpose::Connectivity
            && profile == ModelProfile::High
            && result.outcome == CheckOutcome::Confirmed
            && result.actual.confirmed(ModelProfile::High)
            && !cancelled.load(Ordering::Acquire)
            && epoch != 0
            && epoch == self.model_session.epoch
            && self.model_session.valid
            && self.model_session.reported
        {
            self.high_capability = Some((epoch, cancelled));
        }
        Ok((result, applied))
    }
    pub fn check_summary(&self) -> Result<CheckSummary, StoreError> {
        summary(&self.connection)
    }
}
/// Folds the oldest completed attempts into their purpose totals in the inserting transaction.
/// Known token sums and the count of unknown usage are kept exactly; stage-1 rows never fold.
fn fold(c: &Connection, rows: i64) -> Result<(), StoreError> {
    let mut q = c.prepare(
        "SELECT id,purpose,input_tokens,output_tokens,total_tokens FROM connection_checks WHERE stage=2 ORDER BY id LIMIT ?1",
    )?;
    let folded = q
        .query_map([rows], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if (folded.len() as i64) < rows {
        return Err(StoreError::ResourceLimit);
    }
    for purpose in [1i64, 2, 3] {
        let (mut attempts, mut input, mut output, mut total, mut unknown): (i64, i64, i64, i64, i64) = c
            .query_row(
                "SELECT attempts,input_tokens,output_tokens,total_tokens,unknown FROM connection_check_totals WHERE purpose=?1",
                [purpose],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .unwrap_or_default();
        let mut changed = false;
        for (_, _, i, o, t) in folded.iter().filter(|r| r.1 == purpose) {
            changed = true;
            attempts = attempts.checked_add(1).ok_or(StoreError::ResourceLimit)?;
            match (i, o, t) {
                (Some(i), Some(o), Some(t)) => {
                    input = input.checked_add(*i).ok_or(StoreError::ResourceLimit)?;
                    output = output.checked_add(*o).ok_or(StoreError::ResourceLimit)?;
                    total = total.checked_add(*t).ok_or(StoreError::ResourceLimit)?;
                }
                _ => unknown = unknown.checked_add(1).ok_or(StoreError::ResourceLimit)?,
            }
        }
        if changed {
            c.execute(
                "INSERT INTO connection_check_totals VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(purpose) DO UPDATE SET attempts=excluded.attempts,input_tokens=excluded.input_tokens,output_tokens=excluded.output_tokens,total_tokens=excluded.total_tokens,unknown=excluded.unknown",
                params![purpose, attempts, input, output, total, unknown],
            )?;
        }
    }
    for (id, ..) in folded {
        c.execute("DELETE FROM connection_checks WHERE id=?1", [id])?;
    }
    Ok(())
}
fn summary(c: &Connection) -> Result<CheckSummary, StoreError> {
    let mut s = CheckSummary::default();
    let mut q = c.prepare(
        "SELECT attempts,input_tokens,output_tokens,total_tokens,unknown FROM connection_check_totals ORDER BY purpose",
    )?;
    let totals = q
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (attempts, input, output, total, unknown) in totals {
        let usage = CheckUsage::new(input as u64, output as u64, total as u64)
            .map_err(|_| StoreError::UnsupportedSchema)?;
        if attempts <= 0 || unknown < 0 || unknown > attempts {
            return Err(StoreError::UnsupportedSchema);
        }
        s.attempts = s
            .attempts
            .checked_add(attempts as u64)
            .ok_or(StoreError::ResourceLimit)?;
        s.unknown = s
            .unknown
            .checked_add(unknown as u64)
            .ok_or(StoreError::ResourceLimit)?;
        s.input = s
            .input
            .checked_add(usage.input())
            .ok_or(StoreError::ResourceLimit)?;
        s.output = s
            .output
            .checked_add(usage.output())
            .ok_or(StoreError::ResourceLimit)?;
        s.total = s
            .total
            .checked_add(usage.total())
            .ok_or(StoreError::ResourceLimit)?;
    }
    let mut live = 0u64;
    let mut q=c.prepare("SELECT id,at_utc,stage,outcome,actual_model,actual_effort,actual_tier,input_tokens,output_tokens,total_tokens,purpose,expected_effort FROM connection_checks ORDER BY id")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        if live >= MAX_CHECKS as u64 || (live + 1) * ROW_BYTES as u64 > MAX_CHECK_BYTES as u64 {
            return Err(StoreError::ResourceLimit);
        }
        live += 1;
        let id: i64 = r.get(0)?;
        if id <= 0 {
            return Err(StoreError::UnsupportedSchema);
        }
        let outcome = match r.get::<_, u32>(3)? {
            0 => CheckOutcome::Pending,
            1 => CheckOutcome::Confirmed,
            2 => CheckOutcome::Mismatch,
            3 => CheckOutcome::Provider,
            4 => CheckOutcome::Invalid,
            5 => CheckOutcome::Cancelled,
            6 => CheckOutcome::Timeout,
            7 => CheckOutcome::Interrupted,
            8 => CheckOutcome::Local,
            9 => CheckOutcome::Quota,
            _ => return Err(StoreError::UnsupportedSchema),
        };
        let actual = ActualConfig {
            model: match r.get::<_, u32>(4)? {
                0 => ActualModel::Missing,
                1 => ActualModel::Luna,
                2 => ActualModel::Other,
                _ => return Err(StoreError::UnsupportedSchema),
            },
            effort: match r.get::<_, u32>(5)? {
                0 => ActualEffort::Missing,
                1 => ActualEffort::Low,
                2 => ActualEffort::Other,
                3 => ActualEffort::High,
                _ => return Err(StoreError::UnsupportedSchema),
            },
            tier: match r.get::<_, u32>(6)? {
                0 => ActualTier::Missing,
                1 => ActualTier::Priority,
                2 => ActualTier::Fast,
                3 => ActualTier::Default,
                4 => ActualTier::Other,
                _ => return Err(StoreError::UnsupportedSchema),
            },
        };
        let usage = match (
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<i64>>(8)?,
            r.get::<_, Option<i64>>(9)?,
        ) {
            (Some(i), Some(o), Some(t)) if i >= 0 && o >= 0 && t >= 0 => {
                Some(CheckUsage::new(i as u64, o as u64, t as u64)?)
            }
            (None, None, None) => None,
            _ => return Err(StoreError::UnsupportedSchema),
        };
        s.attempts += 1;
        if let Some(u) = usage {
            s.input = s
                .input
                .checked_add(u.input())
                .ok_or(StoreError::ResourceLimit)?;
            s.output = s
                .output
                .checked_add(u.output())
                .ok_or(StoreError::ResourceLimit)?;
            s.total = s
                .total
                .checked_add(u.total())
                .ok_or(StoreError::ResourceLimit)?;
        } else {
            s.unknown += 1;
        }
        let purpose = match r.get::<_, u32>(10)? {
            1 => ModelPurpose::Connectivity,
            2 => ModelPurpose::Assistance,
            3 => ModelPurpose::Observation,
            _ => return Err(StoreError::UnsupportedSchema),
        };
        let profile = match r.get::<_, u32>(11)? {
            1 => ModelProfile::Low,
            2 => ModelProfile::High,
            _ => return Err(StoreError::UnsupportedSchema),
        };
        if purpose != ModelPurpose::Connectivity && profile != ModelProfile::High {
            return Err(StoreError::UnsupportedSchema);
        }
        // Totals include observation; the latest check stays connectivity or assistance, and
        // observation attempts are shown by the observation journal.
        if purpose == ModelPurpose::Observation {
            continue;
        }
        s.latest = Some(CheckAttempt {
            purpose,
            profile,
            id: AttemptId(id),
            at_utc: r.get(1)?,
            stage: r.get(2)?,
            result: ModelCheckResult {
                outcome,
                actual,
                usage,
            },
        });
    }
    Ok(s)
}
pub(super) fn validate_checks(c: &Connection) -> Result<(), StoreError> {
    summary(c)
        .map(|_| ())
        .map_err(|_| StoreError::UnsupportedSchema)
}
