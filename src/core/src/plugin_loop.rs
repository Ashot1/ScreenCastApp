//! The plugin tool loop inside one assistance dispatch (ADR-021). Core admits and accounts every
//! POST and every call; the bridge alternates "lock Store, Core step" with providers I/O.
use super::*;
use crate::codex_types::codex_hex;
use crate::plugin_package::sha256;
use plugins::{RESULT_ROOM, RESULT_TEXT, result_key};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

const MAX_POSTS: u32 = 5;
const MAX_CALLS: usize = 8;
const LOOP: Duration = Duration::from_secs(180);
const POST: Duration = Duration::from_secs(60);
const CALL: Duration = Duration::from_secs(20);
/// Below this remaining time the next POST is the final one without tools.
const FINAL_MARGIN: Duration = Duration::from_secs(70);
/// A call needs its 20 s plus the reserve of the final POST.
const CALL_MARGIN: Duration = CALL.saturating_add(FINAL_MARGIN);
/// The first call of a local server in a request also starts it (ADR-022): 20 + 30 + 70 s.
const LOCAL_MARGIN: Duration = CALL_MARGIN.saturating_add(LOCAL_START);
const MAX_READ: usize = 256 * 1024;

fn error_code(o: PluginCallOutcome) -> &'static str {
    match o {
        PluginCallOutcome::Completed => "completed",
        PluginCallOutcome::ToolError => "tool_error",
        PluginCallOutcome::Changed => "tool_changed",
        PluginCallOutcome::LoginRequired => "login_required",
        PluginCallOutcome::Unavailable => "unavailable",
        PluginCallOutcome::Invalid => "invalid_response",
        PluginCallOutcome::Timeout => "timeout",
        PluginCallOutcome::Cancelled => "cancelled",
        PluginCallOutcome::Interrupted => "interrupted",
        PluginCallOutcome::NotConfigured => "not_configured",
        PluginCallOutcome::StopUnconfirmed => "stop_unconfirmed",
    }
}
/// The longest UTF-8 prefix within `limit` bytes.
fn prefix(text: &str, limit: usize) -> &str {
    &text[..text.floor_char_boundary(limit)]
}
pub(super) fn result_metadata(tool: &PinnedTool, r: &ToolResult) -> PluginResultMetadata {
    PluginResultMetadata {
        plugin_id: tool.plugin,
        plugin_version: tool.plugin_version.clone(),
        plugin_name: tool.plugin_name.clone(),
        server: tool.server.clone(),
        tool: tool.name.clone(),
        arguments: r.arguments.clone(),
        called_at_utc: r.called_at_utc,
        content_hash: codex_hex(&sha256(r.text.as_bytes())),
        complete: r.complete,
    }
}
pub(super) fn result_evidence(r: &ToolResult) -> SourceEvidence {
    SourceEvidence {
        text: r.text.clone(),
        observed_at_utc: Some(r.called_at_utc),
        completeness: if r.complete {
            Completeness::Complete
        } else {
            Completeness::Partial
        },
        kind: AssertionKind::SourceFact,
    }
}

impl Store {
    /// The loop plan: offered tools and skills come from the prepared snapshot, keys only name
    /// (plugin, server, generation). An assistance request without tools is one exact POST.
    pub fn begin_model_loop(
        &mut self,
        admission: CheckAdmission,
        keys: &[PluginKeyRef],
    ) -> Result<ModelLoop, StoreError> {
        if admission.purpose != ModelPurpose::Assistance
            || !self
                .active_check
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(&v.cancelled, &admission.cancelled))
            || keys.len() > MAX_PLUGIN_KEYS
        {
            return Err(StoreError::InvalidInput);
        }
        let request = admission.request_id.ok_or(StoreError::InvalidInput)?;
        let s = self.current_snapshot(request)?;
        let mut unique = std::collections::BTreeSet::new();
        for k in keys {
            if !unique.insert((k.plugin, k.server.clone())) {
                return Err(StoreError::InvalidInput);
            }
        }
        let functions = s
            .plugins
            .tools
            .iter()
            .map(|t| {
                let d: Value = serde_json::from_str(&t.definition)
                    .map_err(|_| StoreError::UnsupportedSchema)?;
                Ok(StepFunction {
                    name: t.model_name.clone(),
                    description: d
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    parameters: d
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({"type":"object","properties":{}})),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let skills = s
            .plugins
            .skills
            .iter()
            .map(|k| StepSkill {
                plugin: k.plugin.clone(),
                name: k.skill.name.clone(),
                description: k.skill.description.clone(),
                text: k.skill.text.clone(),
            })
            .collect();
        Ok(ModelLoop {
            admission,
            request,
            context: s.preview.context.clone(),
            started: Instant::now(),
            keys: keys.iter().cloned().collect(),
            functions,
            skills,
            history: Vec::new(),
            posts: 0,
            mode: StepMode::Plain,
            last: None,
            pending: Default::default(),
            results: Vec::new(),
            listed: Default::default(),
            results_bytes: 0,
            forwarded_bytes: 0,
            done: false,
        })
    }
    /// Admits the next POST by reference to the loop's admission. The final POST goes without
    /// tools on any trigger, so the request always ends with an answer attempt.
    pub fn dispatch_model_step<'l>(
        &mut self,
        l: &'l mut ModelLoop,
        at_utc: i64,
    ) -> Result<ModelStep<'l>, StoreError> {
        if l.done || !l.pending.is_empty() {
            return Err(StoreError::InvalidInput);
        }
        let remaining = LOOP.saturating_sub(l.started.elapsed());
        let (mode, timeout) = if l.functions.is_empty() {
            if l.posts > 0 {
                return Err(StoreError::InvalidInput);
            }
            (StepMode::Plain, POST)
        } else {
            let last = l.posts + 1 >= MAX_POSTS
                || l.results.len() >= MAX_CALLS
                || remaining < FINAL_MARGIN
                || l.results_bytes + RESULT_ROOM > MAX_LOOP_RESULTS
                || l.forwarded_bytes + MAX_RESPONSE_ITEMS > MAX_FORWARDED_ITEMS;
            if l.mode == StepMode::Final || remaining.is_zero() {
                return Err(StoreError::ResourceLimit);
            }
            (
                if last {
                    StepMode::Final
                } else {
                    StepMode::Tools
                },
                POST.min(remaining),
            )
        };
        let (id, _) = self.dispatch_attempt(&l.admission, at_utc)?;
        l.posts += 1;
        l.mode = mode;
        l.last = Some(id);
        Ok(ModelStep {
            id,
            mode,
            context: &l.context,
            functions: if mode == StepMode::Tools {
                &l.functions
            } else {
                &[]
            },
            skills: if mode == StepMode::Plain {
                &[]
            } else {
                &l.skills
            },
            history: &l.history,
            timeout,
            cancellation: l.admission.cancellation(),
        })
    }
    /// Accounts one POST with the existing `finish_check`. A tool step is `Confirmed` when the
    /// actual configuration matches; any other step ends the request through `finish_assistance`.
    pub fn finish_model_step(
        &mut self,
        l: &mut ModelLoop,
        id: AttemptId,
        mut response: ModelResponse,
    ) -> Result<StepNext, StoreError> {
        if l.done || l.last != Some(id) {
            return Err(StoreError::InvalidInput);
        }
        if l.mode == StepMode::Tools && !response.calls.is_empty() {
            let result = self.finish_check(id, response.check)?;
            if result.outcome != CheckOutcome::Confirmed {
                l.done = true;
                return Ok(StepNext::Done(None));
            }
            for item in response.forwarded {
                l.forwarded_bytes += item.to_string().len();
                l.history.push(HistoryItem::Forwarded(item));
            }
            l.pending.extend(response.calls);
            return Ok(StepNext::Tools);
        }
        l.done = true;
        if !response.calls.is_empty() {
            // Calls in a POST that offered no tools are an invalid answer.
            if response.check.outcome == CheckOutcome::Confirmed {
                response.check.outcome = CheckOutcome::Invalid;
            }
            response.answer = None;
        }
        let results = std::mem::take(&mut l.results);
        self.finish_assistance_with(id, response.check, response.answer, &results)
            .map(StepNext::Done)
    }
    /// Admits the next pending call. Calls that need no network (limits, unknown tool, missing
    /// key, known changed definition) get their error envelope here and are skipped.
    pub fn permit_tool_call(
        &mut self,
        l: &mut ModelLoop,
        at_utc: i64,
    ) -> Result<Option<ToolCallPermit>, StoreError> {
        while let Some(call) = l.pending.pop_front() {
            self.require_assistance()?;
            if l.admission.cancelled.load(Ordering::Acquire) {
                return Err(StoreError::Cancelled);
            }
            let s = self.current_snapshot(l.request)?;
            if s.epoch != l.admission.epoch {
                return Err(StoreError::Cancelled);
            }
            let index = l.results.len() + 1;
            let tool = s
                .plugins
                .tools
                .iter()
                .position(|t| t.model_name == call.name);
            if index > MAX_CALLS {
                l.history.push(HistoryItem::Output {
                    output: json!({"tool":call.name,"error":"call_limit"}).to_string(),
                    call_id: call.call_id,
                });
                continue;
            }
            let Some(t) = tool else {
                l.history.push(HistoryItem::Output {
                    output: json!({"tool":call.name,"error":"unknown_tool"}).to_string(),
                    call_id: call.call_id,
                });
                continue;
            };
            let pinned = &s.plugins.tools[t];
            let arguments = Value::Object(call.arguments).to_string();
            let generation = plugins::generation(&self.connection, pinned.plugin, &pinned.server)?;
            let key = l
                .keys
                .iter()
                .find(|k| {
                    k.plugin == pinned.plugin
                        && k.server == pinned.server
                        && k.generation == generation
                })
                .cloned();
            let listed = l.listed.get(&(pinned.plugin, pinned.server.clone()));
            let list_first = listed.is_none();
            let changed = listed.is_some_and(|m| m.get(&pinned.name) != Some(&pinned.hash));
            // A local or managed server is a process started by its first call in the request.
            let starts = list_first && !matches!(pinned.kind, ServerKind::Remote { .. });
            let margin = if starts { LOCAL_MARGIN } else { CALL_MARGIN };
            let local = if LOOP.saturating_sub(l.started.elapsed()) < margin {
                Some("time_limit")
            } else if let ServerKind::Managed { id } = pinned.kind
                && !id.allows(&pinned.name)
            {
                // A name dropped from the allowlist by a later release stays pinned but unused.
                Some("not_allowed")
            } else if arguments.len() > MAX_TOOL_ARGUMENTS {
                Some("arguments_too_long")
            } else if pinned.kind.needs_key() && key.is_none() {
                Some(match pinned.kind {
                    ServerKind::Remote { .. } | ServerKind::Managed { .. } => "login_required",
                    ServerKind::Local { .. } => "not_configured",
                })
            } else if changed {
                Some("tool_changed")
            } else {
                None
            };
            if let Some(error) = local {
                self.record(
                    l,
                    t,
                    &pinned.model_name,
                    call.call_id,
                    arguments,
                    at_utc,
                    Err(error),
                );
                continue;
            }
            let (plugin, plugin_name, server, name) = (
                pinned.plugin,
                pinned.plugin_name.clone(),
                pinned.server.clone(),
                pinned.name.clone(),
            );
            let log = self.change(|c| {
                let max: i64 =
                    c.query_row("SELECT coalesce(max(id),0) FROM plugin_calls", [], |r| {
                        r.get(0)
                    })?;
                let id = max.checked_add(1).ok_or(StoreError::ResourceLimit)?;
                c.execute(
                    "INSERT INTO plugin_calls VALUES(?1,?2,?3,?4,?5,?6,?7,1,0,0)",
                    params![
                        id,
                        at_utc,
                        plugin as i64,
                        plugin_name,
                        server,
                        name,
                        arguments
                    ],
                )?;
                // The ring keeps the 256 newest calls in the same transaction.
                c.execute(
                    "DELETE FROM plugin_calls WHERE id<=?1",
                    [id - MAX_CALL_LOG as i64],
                )?;
                Ok(id)
            })?;
            return Ok(Some(ToolCallPermit {
                target: pinned.kind.target(),
                plugin: pinned.plugin,
                server: pinned.server.clone(),
                key,
                tool: pinned.name.clone(),
                list_first,
                pinned: pinned.hash,
                timeout: CALL,
                cancellation: l.admission.cancellation(),
                tool_index: t,
                call_id: call.call_id,
                generation,
                log,
                at_utc,
                arguments,
            }));
        }
        Ok(None)
    }
    /// Receives one call: the call log reaches stage 2 first, a 401 revokes exactly the sent
    /// generation (which cancels this request through its snapshot fence), then the result is
    /// compared, bounded and judged citable.
    pub fn accept_tool_result(
        &mut self,
        l: &mut ModelLoop,
        permit: ToolCallPermit,
        result: ToolCallResult,
    ) -> Result<(), StoreError> {
        let s = self
            .prepared
            .as_ref()
            .filter(|s| s.preview.request_id == l.request)
            .cloned()
            .ok_or(StoreError::StaleRevision)?;
        let pinned = s
            .plugins
            .tools
            .get(permit.tool_index)
            .ok_or(StoreError::InvalidInput)?;
        let mut outcome = result.outcome;
        if let Some(listed) = result.listed {
            if outcome == PluginCallOutcome::Completed
                && listed.get(&pinned.name) != Some(&pinned.hash)
            {
                outcome = PluginCallOutcome::Changed;
            }
            l.listed
                .insert((pinned.plugin, pinned.server.clone()), listed);
        }
        let read = result.text.len().min(MAX_READ);
        let (plugin, server, generation, log) = (
            pinned.plugin,
            pinned.server.clone(),
            permit.generation,
            permit.log,
        );
        self.change(|c| {
            c.execute(
                "UPDATE plugin_calls SET stage=2,outcome=?1,result_bytes=?2 WHERE id=?3 AND stage=1",
                params![outcome as u32, read as i64, log],
            )?;
            if outcome == PluginCallOutcome::LoginRequired
                && plugins::generation(c, plugin, &server).ok() == Some(generation)
            {
                plugins::revoke(c, plugin, &server)?;
            }
            Ok(())
        })?;
        let r = if outcome == PluginCallOutcome::Completed {
            Ok(result.text)
        } else {
            Err(error_code(outcome))
        };
        self.record(
            l,
            permit.tool_index,
            &pinned.model_name,
            permit.call_id,
            permit.arguments,
            permit.at_utc,
            r,
        );
        if l.admission.cancelled.load(Ordering::Acquire) {
            return Err(StoreError::Cancelled);
        }
        Ok(())
    }
    /// Adds one numbered result envelope to the history and decides citability at receipt.
    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        l: &mut ModelLoop,
        tool: usize,
        model_name: &str,
        call_id: String,
        arguments: String,
        at_utc: i64,
        result: Result<String, &str>,
    ) {
        let index = l.results.len() + 1;
        let id = format!("tool:{index}");
        let (envelope, entry) = match result {
            Ok(text) => {
                let shown = prefix(&text, RESULT_TEXT).to_owned();
                let mut entry = ToolResult {
                    tool,
                    arguments,
                    called_at_utc: at_utc,
                    complete: shown.len() == text.len(),
                    text: shown,
                    citable: false,
                };
                entry.citable = self.citable(l, &entry).unwrap_or(false);
                (
                    json!({"tool_result_id":id,"tool":model_name,"untrusted":true,"complete":entry.complete,"citable":entry.citable,"text":entry.text}),
                    entry,
                )
            }
            Err(error) => (
                json!({"tool_result_id":id,"tool":model_name,"error":error}),
                ToolResult {
                    tool,
                    arguments,
                    called_at_utc: at_utc,
                    text: String::new(),
                    complete: false,
                    citable: false,
                },
            ),
        };
        let output = envelope.to_string();
        l.results_bytes += output.len();
        l.results.push(entry);
        l.history.push(HistoryItem::Output { call_id, output });
    }
    /// A result may become a saved ground only while its key is writable for the task and the
    /// selection count, context, memory bytes and row reserves leave room for every citable one.
    fn citable(&self, l: &ModelLoop, r: &ToolResult) -> Result<bool, StoreError> {
        let s = self.current_snapshot(l.request)?;
        let pinned = s
            .plugins
            .tools
            .get(r.tool)
            .ok_or(StoreError::InvalidInput)?;
        let prior = l.results.iter().filter(|r| r.citable).collect::<Vec<_>>();
        if r.text.is_empty() || prior.len() >= s.plugins.citable {
            return Ok(false);
        }
        let c = &self.connection;
        let task = s.preview.task.task_id;
        let m = result_metadata(pinned, r);
        if !implicit_write_allowed(c, task, &result_key(&m))? {
            return Ok(false);
        }
        let bytes = |r: &ToolResult| -> Result<usize, StoreError> {
            let p = s
                .plugins
                .tools
                .get(r.tool)
                .ok_or(StoreError::InvalidInput)?;
            plugins::result_bytes(&result_metadata(p, r), r.text.len())
        };
        let own = bytes(r)?;
        let mut memory = own;
        let mut context = s.preview.context.len() + r.text.len() + 1024;
        for p in &prior {
            memory += bytes(p)?;
            context += p.text.len() + 1024;
        }
        let count = prior.len() as i64 + 1;
        Ok(own <= MAX_TEXT_BYTES
            && context <= MAX_ASSISTANCE_CONTEXT
            && byte_room(c, memory as i64).is_ok()
            && reserve(c, "evidence", count).is_ok()
            && reserve(c, "source_bindings", count).is_ok())
    }
}
#[cfg(test)]
#[path = "plugin_loop_tests.rs"]
mod tests;
