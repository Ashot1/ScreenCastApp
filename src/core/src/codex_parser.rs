use crate::codex_types::{codex_event_version, codex_hash};
use crate::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn parse_codex_header(bytes: &[u8]) -> Result<CodexHeader, StoreError> {
    if bytes.len() > MAX_CODEX_LINE {
        return Err(StoreError::ResourceLimit);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(StoreError::InvalidInput);
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| StoreError::InvalidInput)?;
    if v["type"] != "session_meta" || v["ordinal"].as_u64().is_none() {
        return Err(StoreError::InvalidInput);
    }
    let p = &v["payload"];
    if p["cli_version"] != CODEX_VERSION
        || p["history_mode"] != "paginated"
        || [
            "history_base",
            "forked_from_id",
            "forked_from_ordinal_exclusive",
        ]
        .iter()
        .any(|k| !p[*k].is_null())
    {
        return Err(StoreError::InvalidInput);
    }
    let thread = id(&p["id"], 256)?;
    let subagent_start = if p["subagent_history_start_ordinal"].is_null() {
        None
    } else {
        Some(
            p["subagent_history_start_ordinal"]
                .as_u64()
                .ok_or(StoreError::InvalidInput)?,
        )
    };
    Ok(CodexHeader {
        thread_id: thread.into(),
        subagent_start,
        fingerprint: codex_hash(&[bytes]),
    })
}
fn id(v: &Value, max: usize) -> Result<&str, StoreError> {
    let s = v.as_str().ok_or(StoreError::InvalidInput)?;
    crate::memory_types::key_field(s, max)?;
    Ok(s)
}
fn timestamp(v: &Value) -> Result<Option<i64>, StoreError> {
    let t = v["timestamp"].as_str().ok_or(StoreError::InvalidInput)?;
    time::OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339)
        .map(|v| Some(v.unix_timestamp()))
        .map_err(|_| StoreError::InvalidInput)
}
fn project(h: &CodexHeader, v: &Value, ordinal: u64) -> Result<Option<CodexEvent>, CodexReason> {
    let outer = v["type"].as_str().ok_or(CodexReason::Malformed)?;
    match outer {
        "compacted" => return Err(CodexReason::Compacted),
        "session_meta"
        | "response_item"
        | "inter_agent_communication"
        | "inter_agent_communication_metadata"
        | "turn_context"
        | "token_usage_record"
        | "world_state"
        | "retained_context"
        | "security_risk_score"
        | "realtime_item" => return Ok(None),
        "event_msg" => {}
        _ => return Err(CodexReason::Unsupported),
    }
    let p = &v["payload"];
    let typ = p["type"].as_str().ok_or(CodexReason::Malformed)?;
    let observed = timestamp(v).map_err(|_| CodexReason::Malformed)?;
    let turn = || {
        id(&p["turn_id"], 2048)
            .map(str::to_owned)
            .map_err(|_| CodexReason::Turn)
    };
    let (identity, text, partial, time) = match typ {
        "item_completed" => {
            if p["thread_id"].as_str() != Some(&h.thread_id) {
                return Err(CodexReason::Turn);
            }
            let turn_id = turn()?;
            let item = &p["item"];
            let item_id = id(&item["id"], 2048)
                .map_err(|_| CodexReason::Turn)?
                .to_owned();
            let (kind, text, partial) = match item["type"].as_str() {
                Some("UserMessage") | Some("AgentMessage") => {
                    let agent = item["type"] == "AgentMessage";
                    let mut text = String::new();
                    let mut partial = false;
                    for part in item["content"].as_array().ok_or(CodexReason::Malformed)? {
                        match part["type"].as_str() {
                            Some(t) if t == if agent { "Text" } else { "text" } => {
                                text.push_str(part["text"].as_str().ok_or(CodexReason::Malformed)?)
                            }
                            Some(
                                "image" | "local_image" | "audio" | "local_audio" | "skill"
                                | "mention",
                            ) if !agent => partial = true,
                            _ => return Err(CodexReason::Unsupported),
                        }
                    }
                    (
                        if agent {
                            CodexEventKind::Agent
                        } else {
                            CodexEventKind::User
                        },
                        text,
                        partial,
                    )
                }
                Some("Plan") => (
                    CodexEventKind::Plan,
                    item["text"].as_str().ok_or(CodexReason::Malformed)?.into(),
                    false,
                ),
                Some("FunctionCallOutput") => {
                    if item["namespace"] != "codex_app" {
                        return Ok(None);
                    }
                    let kind = match item["name"].as_str() {
                        Some("create_thread") => CodexEventKind::DelegatedCreate,
                        Some("send_message_to_thread") => CodexEventKind::DelegatedSend,
                        _ => return Ok(None),
                    };
                    (
                        kind,
                        item["output"]
                            .as_str()
                            .ok_or(CodexReason::Unsupported)?
                            .into(),
                        false,
                    )
                }
                Some(_) => return Err(CodexReason::Unsupported),
                None => return Err(CodexReason::Malformed),
            };
            let time = match p.get("completed_at_ms") {
                Some(Value::Number(n)) => match n.as_i64() {
                    Some(0) => observed,
                    Some(ms) => Some(ms.div_euclid(1000)),
                    None => return Err(CodexReason::Malformed),
                },
                None => observed,
                _ => return Err(CodexReason::Malformed),
            };
            (
                CodexEventIdentity::Item {
                    turn_id,
                    item_id,
                    kind,
                },
                text,
                partial,
                time,
            )
        }
        "task_started" | "turn_started" | "task_complete" | "turn_complete" | "turn_aborted" => {
            let turn_id = turn()?;
            if !p["error"].is_null() && p["error"]["message"].as_str().is_none() {
                return Err(CodexReason::Malformed);
            }
            if typ == "turn_aborted"
                && !matches!(
                    p["reason"].as_str(),
                    Some("interrupted" | "replaced" | "review_ended" | "budget_limited")
                )
            {
                return Err(CodexReason::Malformed);
            }
            let time_field = if matches!(typ, "task_started" | "turn_started") {
                "started_at"
            } else {
                "completed_at"
            };
            let source_time = if p[time_field].is_null() {
                observed
            } else {
                Some(p[time_field].as_i64().ok_or(CodexReason::Malformed)?)
            };
            let kind = match typ {
                "task_started" | "turn_started" => CodexEventKind::Started,
                "turn_aborted" => CodexEventKind::Aborted,
                _ if p["error"].is_null() => CodexEventKind::Completed,
                _ => CodexEventKind::Failed,
            };
            let description = match kind {
                CodexEventKind::Started => "observed turn start",
                CodexEventKind::Completed => {
                    "observed turn completion without an error; user task completion is unverified"
                }
                CodexEventKind::Failed => "observed turn failure",
                _ => "observed turn interruption",
            };
            (
                CodexEventIdentity::Turn { turn_id, kind },
                format!(
                    "App projection of a structured Codex event (not a quotation): {description}."
                ),
                false,
                source_time,
            )
        }
        "context_compacted" => return Err(CodexReason::Compacted),
        _ if CODEX_SERVICE_EVENTS.contains(&typ) => return Ok(None),
        _ => return Err(CodexReason::Unsupported),
    };
    let key = identity.source_key(&h.thread_id);
    let evidence = SourceEvidence {
        text,
        observed_at_utc: time,
        completeness: if partial {
            Completeness::Partial
        } else {
            Completeness::Complete
        },
        kind: AssertionKind::SourceFact,
    };
    let version = codex_event_version(&h.thread_id, &identity, &evidence);
    Ok(Some(CodexEvent {
        ordinal,
        identity,
        key,
        version,
        evidence,
    }))
}
/// Consumes complete physical records from newest to oldest, leaving unaccepted records before the cursor.
pub fn parse_codex_window(
    h: &CodexHeader,
    bytes: &[u8],
    start: u64,
    starts_at_line: bool,
) -> Result<CodexWindow, StoreError> {
    if bytes.len() > MAX_CODEX_WINDOW || start.checked_add(bytes.len() as u64).is_none() {
        return Err(StoreError::ResourceLimit);
    }
    let mut w = CodexWindow {
        events: Vec::new(),
        reasons: 0,
        next_before: start,
        physical_lines: 0,
    };
    let leading = if starts_at_line {
        0
    } else {
        w.reasons |= CodexReason::Partial as u32;
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |p| p + 1)
    };
    let trailing = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1);
    if trailing < bytes.len() {
        w.reasons |= CodexReason::Partial as u32;
    }
    let mut ranges = Vec::new();
    let mut end = trailing;
    while end > leading && ranges.len() < 4096 {
        let begin = bytes[leading..end - 1]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(leading, |p| leading + p + 1);
        ranges.push((begin, end));
        end = begin;
    }
    if end > leading {
        w.reasons |= CodexReason::Limit as u32;
    }
    let mut rows = Vec::new();
    let mut next_ordinal = None;
    let mut seen_ordinals = BTreeMap::new();
    let mut bad = BTreeSet::new();
    for (i, (a, b)) in ranges.iter().copied().enumerate() {
        if b - a > MAX_CODEX_LINE {
            rows.push(None);
            continue;
        }
        let value = serde_json::from_slice::<Value>(&bytes[a..b]).ok();
        if let Some(o) = value.as_ref().and_then(|v| v["ordinal"].as_u64()) {
            // Any repeated ordinal is ambiguous, even with service records between its occurrences.
            if let Some(prior) = seen_ordinals.insert(o, i) {
                bad.insert(prior);
                bad.insert(i);
                w.reasons |= CodexReason::Ordinal as u32;
            }
            if let Some((n, j)) = next_ordinal {
                if o >= n {
                    bad.insert(i);
                    bad.insert(j);
                    w.reasons |= CodexReason::Ordinal as u32;
                } else if o.checked_add(1) != Some(n) {
                    w.reasons |= CodexReason::Ordinal as u32;
                }
            }
            next_ordinal = Some((o, i));
        }
        rows.push(value);
    }
    let mut accepted: BTreeMap<String, CodexEvent> = BTreeMap::new();
    let mut conflicted = BTreeSet::new();
    let mut total = 0;
    w.next_before = start + trailing as u64;
    for (i, ((a, b), value)) in ranges.into_iter().zip(rows).enumerate() {
        let mut event = None;
        if b - a > MAX_CODEX_LINE {
            w.reasons |= CodexReason::Limit as u32;
        } else if let Some(v) = value {
            if let Some(o) = v["ordinal"].as_u64() {
                if bad.contains(&i) {
                } else if h.subagent_start.is_some_and(|boundary| o < boundary) {
                    w.reasons |= CodexReason::Filtered as u32;
                } else {
                    match project(h, &v, o) {
                        Ok(e) => event = e,
                        Err(r) => w.reasons |= r as u32,
                    }
                }
            } else {
                w.reasons |= CodexReason::Ordinal as u32;
            }
        } else {
            w.reasons |= CodexReason::Malformed as u32;
        }
        if let Some(e) = event {
            let key = e.key.object_id.clone();
            if let Some(old) = accepted.get(&key) {
                if old.identity.kind() != e.identity.kind() {
                    conflicted.insert(key);
                    w.reasons |= CodexReason::Turn as u32;
                }
            } else if !conflicted.contains(&key) {
                let size = e.variable_bytes() + 256;
                if size > MAX_CODEX_LINE {
                    w.reasons |= CodexReason::Limit as u32;
                } else if accepted.len() >= MAX_CODEX_ITEMS || total + size > MAX_CODEX_LINE {
                    w.reasons |= CodexReason::Limit as u32;
                    break;
                } else {
                    total += size;
                    accepted.insert(key, e);
                }
            }
        }
        w.physical_lines += 1;
        w.next_before = start + a as u64;
    }
    if w.physical_lines == 0 {
        w.next_before = start;
    }
    w.events = accepted
        .into_iter()
        .filter(|(k, _)| !conflicted.contains(k))
        .map(|(_, v)| v)
        .collect();
    w.events.sort_by_key(|e| e.ordinal);
    Ok(w)
}

// Non-projecting EventMsg tags from the pinned protocol, not an open-ended future-schema fallback.
const CODEX_SERVICE_EVENTS: &[&str] = &[
    "error",
    "warning",
    "auth_recovery_started",
    "auth_recovery_completed",
    "guardian_warning",
    "realtime_conversation_started",
    "realtime_conversation_realtime",
    "realtime_conversation_closed",
    "realtime_conversation_sdp",
    "model_reroute",
    "model_verification",
    "turn_moderation_metadata",
    "safety_buffering",
    "thread_rolled_back",
    "thread_settings_applied",
    "token_count",
    "agent_message",
    "user_message",
    "agent_reasoning",
    "agent_reasoning_raw_content",
    "agent_reasoning_section_break",
    "session_configured",
    "environment_connected",
    "environment_disconnected",
    "thread_goal_updated",
    "thread_queue_changed",
    "mcp_startup_update",
    "mcp_startup_complete",
    "mcp_tool_call_begin",
    "mcp_tool_call_end",
    "web_search_begin",
    "web_search_end",
    "image_generation_begin",
    "image_generation_end",
    "exec_command_begin",
    "exec_command_output_delta",
    "terminal_interaction",
    "exec_command_end",
    "view_image_tool_call",
    "exec_approval_request",
    "request_permissions",
    "request_user_input",
    "dynamic_tool_call_request",
    "dynamic_tool_call_response",
    "elicitation_request",
    "elicitation_abandoned",
    "apply_patch_approval_request",
    "guardian_assessment",
    "deprecation_notice",
    "stream_error",
    "patch_apply_begin",
    "patch_apply_updated",
    "patch_apply_end",
    "turn_diff",
    "realtime_conversation_list_voices_response",
    "plan_update",
    "shutdown_complete",
    "entered_review_mode",
    "exited_review_mode",
    "raw_response_item",
    "raw_response_completed",
    "item_started",
    "hook_started",
    "hook_completed",
    "agent_message_content_delta",
    "plan_delta",
    "reasoning_content_delta",
    "reasoning_raw_content_delta",
    "collab_agent_spawn_begin",
    "collab_agent_spawn_end",
    "collab_agent_interaction_begin",
    "collab_agent_interaction_end",
    "collab_waiting_begin",
    "collab_waiting_end",
    "collab_close_begin",
    "collab_close_end",
    "collab_resume_begin",
    "collab_resume_end",
    "sub_agent_activity",
];
