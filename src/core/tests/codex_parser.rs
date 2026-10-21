use ambient_core::*;
use serde_json::{Value, json};

#[test]
fn nonadjacent_duplicate_ordinals_exclude_every_ambiguous_record() -> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = item(1, "UserMessage", "ambiguous A");
    bytes.extend(line(json!({"ordinal":2,"type":"world_state","payload":{}})));
    bytes.extend(item(1, "UserMessage", "ambiguous B"));
    bytes.extend(line(json!({"ordinal":3,"type":"world_state","payload":{}})));
    bytes.extend(item(4, "UserMessage", "independent"));
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(
        w.events
            .iter()
            .map(|e| e.evidence.text.as_str())
            .collect::<Vec<_>>(),
        ["independent"]
    );
    assert!(w.reasons & 16 != 0);
    assert_eq!(w.next_before, 0);
    Ok(())
}

#[test]
fn oversized_header_is_a_size_limit_and_not_a_version_error() {
    let mut bytes = vec![b' '; MAX_CODEX_LINE];
    bytes.push(b'\n');
    assert_eq!(
        parse_codex_header(&bytes).err(),
        Some(StoreError::ResourceLimit)
    );
}
#[test]
fn malformed_identity_order_compaction_and_no_complete_line_never_fabricate_input()
-> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = line(
        json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":1,"type":"compacted","payload":{"message":"summary","replacement_history":[{"type":"message","role":"user","content":[{"type":"input_text","text":"not a new prompt"}]}]}}),
    );
    bytes.extend_from_slice(b"\xff\n");
    let mut wrong: Value = serde_json::from_slice(&item(3, "UserMessage", "wrong thread"))
        .map_err(|_| StoreError::InvalidInput)?;
    wrong["payload"]["thread_id"] = json!("other-thread");
    bytes.extend(line(wrong.clone()));
    wrong["ordinal"] = json!(4);
    wrong["payload"]["thread_id"] = json!("thread-one");
    wrong["payload"]["turn_id"] = Value::Null;
    bytes.extend(line(wrong));
    bytes.extend(line(json!({"type":"world_state","payload":{}})));
    bytes.extend(item(6, "UserMessage", "retained"));
    bytes.extend(item(8, "AgentMessage", "after gap"));
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(
        w.events
            .iter()
            .map(|e| e.evidence.text.as_str())
            .collect::<Vec<_>>(),
        ["retained", "after gap"]
    );
    assert_eq!(w.reasons & (4 | 16 | 32 | 128), 4 | 16 | 32 | 128);
    let bytes = item(1, "UserMessage", "duplicate");
    let mut both = bytes.clone();
    both.extend(bytes);
    let w = parse_codex_window(&h, &both, 0, true)?;
    assert!(w.events.is_empty());
    assert!(w.reasons & 16 != 0);
    let w = parse_codex_window(&h, b"unfinished record", 500, false)?;
    assert!(w.events.is_empty());
    assert_eq!(w.next_before, 500);
    assert_eq!(w.physical_lines, 0);
    assert!(w.reasons & 1 != 0);
    Ok(())
}
#[test]
fn subagent_prefix_filters_terminals_before_projection_and_empty_text_stays_empty()
-> Result<(), StoreError> {
    let mut meta: Value =
        serde_json::from_slice(&header()).map_err(|_| StoreError::InvalidInput)?;
    for field in ["cli_version", "history_mode"] {
        let mut missing = meta.clone();
        missing["payload"]
            .as_object_mut()
            .ok_or(StoreError::InvalidInput)?
            .remove(field);
        assert!(parse_codex_header(&line(missing)).is_err());
    }
    meta["payload"]["subagent_history_start_ordinal"] = json!(5);
    let h = parse_codex_header(&line(meta))?;
    let mut bytes = line(
        json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":4,"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-one"}}),
    );
    bytes.extend(item(5, "UserMessage", ""));
    bytes.extend(line(json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":6,"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-one","reason":"interrupted"}})));
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(w.events.len(), 2);
    assert_eq!(w.events[0].evidence.text, "");
    assert_eq!(w.events[1].identity.kind(), CodexEventKind::Aborted);
    assert!(w.reasons & 64 != 0);
    Ok(())
}

#[test]
fn pinned_service_events_are_ignored_but_unknown_events_mark_incompleteness()
-> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = Vec::new();
    for (ordinal, tag) in [
        (1, "exec_command_begin"),
        (2, "agent_message_content_delta"),
        (3, "plan_update"),
    ] {
        bytes.extend(line(json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":ordinal,"type":"event_msg","payload":{"type":tag}})));
    }
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert!(w.events.is_empty());
    assert_eq!(w.reasons, 0);
    bytes.extend(line(json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":4,"type":"event_msg","payload":{"type":"future_event"}})));
    assert!(parse_codex_window(&h, &bytes, 0, true)?.reasons & 8 != 0);
    Ok(())
}

fn header() -> Vec<u8> {
    line(
        json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"thread-one","cli_version":"0.163.0-alpha.10.2","history_mode":"paginated"}}),
    )
}
fn line(v: Value) -> Vec<u8> {
    let mut b = v.to_string().into_bytes();
    b.push(b'\n');
    b
}
fn item(ordinal: u64, kind: &str, text: &str) -> Vec<u8> {
    let content = if kind == "AgentMessage" {
        json!([{"type":"Text","text":text}])
    } else {
        json!([{"type":"text","text":text}])
    };
    line(
        json!({"timestamp":"2026-10-20T08:00:01.123Z","ordinal":ordinal,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-one","turn_id":"turn-one","item":{"id":format!("item-{ordinal}"),"type":kind,"content":content},"completed_at_ms":1792483201123i64}}),
    )
}
#[test]
fn canonical_text_is_exact_and_final_phase_does_not_invent_completion() -> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = item(1, "UserMessage", " exact\nтекст \0 ");
    let mut agent: Value = serde_json::from_slice(&item(2, "AgentMessage", "answer"))
        .map_err(|_| StoreError::InvalidInput)?;
    agent["payload"]["item"]["phase"] = json!("final_answer");
    bytes.extend(line(agent));
    bytes.extend(line(json!({"timestamp":"2026-10-20T08:00:02.000Z","ordinal":3,"type":"response_item","payload":{"type":"message","role":"user","content":[]}})));
    let w = parse_codex_window(&h, &bytes, 100, true)?;
    assert_eq!(w.events.len(), 2);
    assert_eq!(w.events[0].evidence.text, " exact\nтекст \0 ");
    assert_eq!(w.events[0].identity.kind(), CodexEventKind::User);
    assert_eq!(w.events[1].identity.kind(), CodexEventKind::Agent);
    assert_eq!(w.events[0].evidence.kind, AssertionKind::SourceFact);
    assert_eq!(w.events[0].evidence.observed_at_utc, Some(1792483201));
    assert_eq!(w.next_before, 100);
    Ok(())
}
#[test]
fn header_rejects_missing_wrong_version_mode_and_every_nonnull_fork() {
    let base: Value = serde_json::from_slice(&header()).unwrap_or_else(|_| panic!("fixture"));
    for (field, value) in [
        ("cli_version", Value::Null),
        ("cli_version", json!(123)),
        ("cli_version", json!("0.163.1")),
        ("history_mode", Value::Null),
        ("history_mode", json!("legacy")),
        ("history_base", json!({})),
        ("forked_from_id", json!("parent")),
        ("forked_from_ordinal_exclusive", json!(0)),
        ("subagent_history_start_ordinal", json!("0")),
    ] {
        let mut v = base.clone();
        v["payload"][field] = value;
        assert!(parse_codex_header(&line(v)).is_err(), "{field}");
    }
    let mut v = base.clone();
    v["payload"]
        .as_object_mut()
        .unwrap_or_else(|| panic!("fixture"))
        .remove("id");
    v["payload"]["session_id"] = json!("wrong-fallback");
    assert!(parse_codex_header(&line(v)).is_err());
}
#[test]
fn terminal_without_start_has_labelled_projection_and_missing_id_has_no_fact()
-> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = Vec::new();
    for (ordinal, payload) in [
        (
            1,
            json!({"type":"task_complete","turn_id":"done","error":null,"last_agent_message":"do not import"}),
        ),
        (
            2,
            json!({"type":"turn_complete","turn_id":"failed","error":{"message":"secret raw error"}}),
        ),
        (3, json!({"type":"turn_aborted","reason":"interrupted"})),
        (4, json!({"type":"task_started","turn_id":"began"})),
    ] {
        bytes.extend(line(json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":ordinal,"type":"event_msg","payload":payload})));
    }
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(w.events.len(), 3);
    assert_eq!(w.events[0].identity.kind(), CodexEventKind::Completed);
    assert!(w.events[0].evidence.text.contains("App projection"));
    assert!(
        !w.events.iter().any(
            |e| e.evidence.text.contains("secret") || e.evidence.text.contains("do not import")
        )
    );
    assert!(w.reasons & CodexReason::Turn as u32 != 0);
    Ok(())
}
#[test]
fn physical_cursor_advances_through_service_only_windows_without_skipping_items()
-> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = Vec::new();
    for ordinal in 1..=5000 {
        bytes.extend(line(
            json!({"ordinal":ordinal,"type":"world_state","payload":{}}),
        ));
    }
    let first = parse_codex_window(&h, &bytes, 0, true)?;
    assert!(first.events.is_empty());
    assert_eq!(first.physical_lines, 4096);
    assert!(first.next_before > 0);
    let earlier = parse_codex_window(&h, &bytes[..first.next_before as usize], 0, true)?;
    assert_eq!(earlier.physical_lines, 904);
    assert_eq!(earlier.next_before, 0);
    let mut bytes = Vec::new();
    for n in 1..=129 {
        bytes.extend(item(n, "UserMessage", "text"));
    }
    let first = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(first.events.len(), 128);
    assert_eq!(first.events[0].ordinal, 2);
    let earlier = parse_codex_window(&h, &bytes[..first.next_before as usize], 0, true)?;
    assert_eq!(earlier.events.len(), 1);
    assert_eq!(earlier.events[0].ordinal, 1);
    assert_eq!(earlier.next_before, 0);
    Ok(())
}
#[test]
fn byte_limit_leaves_unaccepted_records_available_and_never_truncates_text()
-> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut bytes = Vec::new();
    let text = "я".repeat(1500);
    for n in 1..=100 {
        bytes.extend(item(n, "UserMessage", &text));
    }
    let first = parse_codex_window(&h, &bytes, 0, true)?;
    assert!(first.events.len() < 100);
    assert!(first.next_before > 0);
    let second = parse_codex_window(&h, &bytes[..first.next_before as usize], 0, true)?;
    let events: Vec<_> = second.events.iter().chain(&first.events).collect();
    assert_eq!(events.len(), 100);
    assert!(events.iter().all(|e| e.evidence.text == text));
    assert_eq!(
        events.iter().map(|e| e.ordinal).collect::<Vec<_>>(),
        (1..=100).collect::<Vec<_>>()
    );
    Ok(())
}
#[test]
fn fragments_oversize_ordinals_and_subagent_prefix_have_explicit_losses() -> Result<(), StoreError>
{
    let mut v: Value = serde_json::from_slice(&header()).map_err(|_| StoreError::InvalidInput)?;
    v["payload"]["subagent_history_start_ordinal"] = json!(5);
    let h = parse_codex_header(&line(v))?;
    let mut bytes = item(4, "UserMessage", "inherited");
    bytes.extend(item(5, "UserMessage", "own"));
    bytes.extend_from_slice(b"partial tail");
    let w = parse_codex_window(&h, &bytes, 50, true)?;
    assert_eq!(w.events.len(), 1);
    assert_eq!(w.events[0].evidence.text, "own");
    assert_eq!(w.reasons & 65, 65);
    let h = parse_codex_header(&header())?;
    let mut bytes = vec![0xf0, 0x9f, 0x8c, 0xbf, b'\n'];
    bytes.extend(item(2, "UserMessage", "whole"));
    let w = parse_codex_window(&h, &bytes[2..], 102, false)?;
    assert_eq!(w.events.len(), 1);
    assert_eq!(w.next_before, 105);
    let mut bytes = vec![b'x'; MAX_CODEX_LINE + 1];
    bytes.push(b'\n');
    bytes.extend(item(2, "UserMessage", "keep"));
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(w.events.len(), 1);
    assert!(w.reasons & 2 != 0);
    assert_eq!(w.next_before, 0);
    let mut bytes = item(1, "UserMessage", "keep");
    bytes.extend(item(3, "UserMessage", "ambiguous"));
    bytes.extend(item(2, "UserMessage", "ambiguous"));
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(w.events.len(), 1);
    assert_eq!(w.events[0].ordinal, 1);
    assert!(w.reasons & 16 != 0);
    Ok(())
}
#[test]
fn snapshots_text_projection_and_delegation_keep_distinct_provenance() -> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    let mut a: Value = serde_json::from_slice(&item(1, "UserMessage", "first"))
        .map_err(|_| StoreError::InvalidInput)?;
    let mut b = a.clone();
    b["ordinal"] = json!(2);
    b["payload"]["item"]["content"] = json!([{"type":"text","text":"exact "},{"type":"image","image_url":"not-read"},{"type":"text","text":"parts"}]);
    let mut bytes = line(a.clone());
    bytes.extend(line(b));
    a["ordinal"] = json!(3);
    a["payload"]["item"] = json!({"type":"FunctionCallOutput","id":"delegate","namespace":"codex_app","name":"create_thread","output":"delegated input"});
    bytes.extend(line(a.clone()));
    a["ordinal"] = json!(4);
    a["payload"]["item"]["output"] = json!([{"type":"input_text","text":"unsupported array"}]);
    bytes.extend(line(a));
    let w = parse_codex_window(&h, &bytes, 0, true)?;
    assert_eq!(w.events.len(), 2);
    assert_eq!(w.events[0].ordinal, 2);
    assert_eq!(w.events[0].evidence.text, "exact parts");
    assert_eq!(w.events[0].evidence.completeness, Completeness::Partial);
    assert_eq!(w.events[1].identity.kind(), CodexEventKind::DelegatedCreate);
    assert!(w.reasons & 8 != 0);
    let other = parse_codex_window(&h, &bytes, 100, true)?;
    assert_eq!(other.events[0].version, w.events[0].version);
    Ok(())
}
#[test]
fn wrong_terminal_error_type_does_not_create_observed_status() -> Result<(), StoreError> {
    let h = parse_codex_header(&header())?;
    for error in [json!(false), json!(0), json!("oops"), json!({})] {
        let bytes = line(
            json!({"timestamp":"2026-10-20T08:00:00.000Z","ordinal":1,"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn","error":error}}),
        );
        let w = parse_codex_window(&h, &bytes, 0, true)?;
        assert!(w.events.is_empty());
        assert!(w.reasons & 4 != 0);
    }
    Ok(())
}
