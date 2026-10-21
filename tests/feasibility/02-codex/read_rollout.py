"""Read only one explicitly authorized local rollout. Never starts Codex.

This is test tooling for one Desktop engine version, not a supported product API.
The Python API returns source text in memory; the CLI emits only metadata/digests.
"""

import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import time


VERSION = "0.163.0-alpha.10.2"
MAX_BYTES = 16 * 1024 * 1024


def inspect_rollout(path, expected_thread):
    with Path(path).open("rb") as source:
        data = source.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("history_exceeds_probe_limit")
    # The writer may be appending a record: only newline-terminated records count.
    lines = data.split(b"\n")
    partial_tail = bool(lines.pop())
    records = []
    for line in lines:
        try:
            value = json.loads(line)
        except (ValueError, UnicodeDecodeError) as error:
            raise ValueError("malformed_record") from error
        if not isinstance(value, dict) or not isinstance(value.get("payload"), dict):
            raise ValueError("unsupported_record_shape")
        records.append(value)
    if not records or records[0].get("type") != "session_meta":
        raise ValueError("missing_session_metadata")
    meta = records[0]["payload"]
    if meta.get("id") != expected_thread or not isinstance(expected_thread, str) or not expected_thread:
        raise ValueError("thread_identity_mismatch")
    if meta.get("cli_version") != VERSION:
        raise ValueError("unsupported_engine_version")
    turns = []
    current = None
    compactions_observed = 0
    for record in records[1:]:
        kind, payload = record.get("type"), record["payload"]
        if kind == "event_msg":
            event = payload.get("type")
            if payload.get("thread_id", expected_thread) != expected_thread:
                raise ValueError("event_thread_mismatch")
            if event == "task_started":
                turn_id = payload.get("turn_id")
                if not isinstance(turn_id, str) or not turn_id or any(t["id"] == turn_id for t in turns):
                    raise ValueError("invalid_or_duplicate_turn_id")
                current = {"id": turn_id, "status": "completion_not_observed", "messages": [], "plans": []}
                turns.append(current)
            elif event in ("task_complete", "turn_complete", "turn_aborted"):
                # No process-local status or mtime can supply a missing terminal identity.
                matches = [t for t in turns if t["id"] == payload.get("turn_id")]
                if not matches or matches[0]["status"] != "completion_not_observed":
                    raise ValueError("unmatched_terminal_event")
                matches[0]["status"] = "aborted" if event == "turn_aborted" else (
                    "failed" if payload.get("error") is not None else "completed")
                matches[0]["terminal_timestamp"] = record.get("timestamp")
            elif event == "item_completed":
                item = payload.get("item")
                if not isinstance(item, dict):
                    raise ValueError("invalid_completed_item")
                if item.get("type") == "plan":
                    matches = [t for t in turns if t["id"] == payload.get("turn_id")]
                    if not matches:
                        raise ValueError("unmatched_plan")
                    matches[0]["plans"].append(item)
                elif item.get("type") == "UserMessage":
                    matches = [t for t in turns if t["id"] == payload.get("turn_id")]
                    content = item.get("content")
                    if not matches or not isinstance(item.get("id"), str) or not item["id"]:
                        raise ValueError("unmatched_canonical_user_message")
                    if not isinstance(content, list) or any(not isinstance(c, dict) for c in content):
                        raise ValueError("invalid_canonical_user_content")
                    text = "".join(c["text"] for c in content if c.get("type") == "text")
                    # The visible item ID is distinct from the provider response-item ID.
                    matches[0]["messages"].append({"id": item["id"], "role": "canonical_user", "phase": None, "text": text})
            elif event not in ("token_count", "thread_settings_applied"):
                raise ValueError("unsupported_event_type")
        elif kind == "response_item":
            if payload.get("type") == "message" and payload.get("role") in ("user", "assistant"):
                if current is None:
                    raise ValueError("message_without_turn")
                content = payload.get("content")
                if not isinstance(content, list) or any(not isinstance(c, dict) for c in content):
                    raise ValueError("invalid_message_content")
                text = "".join(c["text"] for c in content if c.get("type") in ("input_text", "output_text", "text"))
                current["messages"].append({"role": payload["role"], "phase": payload.get("phase"), "text": text})
            elif payload.get("type") == "function_call_output" and payload.get("namespace") == "codex_app" and payload.get("name") in ("create_thread", "send_message_to_thread"):
                text = payload.get("output")
                if current is None or not isinstance(text, str) or not text.startswith("<codex_delegation>"):
                    raise ValueError("unsupported_delegation_input")
                current["messages"].append({"role": "delegated_input", "phase": None, "text": text})
        elif kind == "compacted":
            if meta.get("history_mode") != "paginated":
                raise ValueError("unsupported_compaction_history_mode")
            # Codex's paginated visible-history projection applies no changes for Compacted.
            # Its replacement history belongs to model-context reconstruction, not new input.
            compactions_observed += 1
        elif kind not in ("world_state", "turn_context", "token_usage_record", "inter_agent_communication_metadata"):
            # Compacted/paginated or future formats need new evidence, never a silent fallback.
            raise ValueError("unsupported_record_type")
    return {"thread_id": expected_thread, "engine_version": VERSION,
            "originator": meta.get("originator"), "source": meta.get("source"),
            "history_mode": meta.get("history_mode"), "coverage": "local_records_only",
            "compactions_observed": compactions_observed, "model_context_reconstructed": False,
            "turns": turns, "partial_tail": partial_tail, "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest()}


def summary(snapshot):
    result = {k: v for k, v in snapshot.items() if k != "turns"}
    result["turns"] = []
    for turn in snapshot["turns"]:
        result["turns"].append({
            "id": turn["id"], "status": turn["status"],
            "terminal_timestamp": turn.get("terminal_timestamp"),
            "plans": [{"chars": len(json.dumps(p, ensure_ascii=False)),
                       "sha256": hashlib.sha256(json.dumps(p, ensure_ascii=False).encode()).hexdigest()}
                      for p in turn["plans"]],
            "messages": [{"id": m.get("id"), "role": m["role"], "phase": m["phase"], "chars": len(m["text"]),
                          "sha256": hashlib.sha256(m["text"].encode()).hexdigest()}
                         for m in turn["messages"]]})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    parser.add_argument("thread_id")
    parser.add_argument("--watch-seconds", type=float, default=0)
    args = parser.parse_args()
    initial = inspect_rollout(args.path, args.thread_id)
    print(json.dumps({"kind": "snapshot", **summary(initial)}), flush=True)
    if args.watch_seconds <= 0:
        return
    if not initial["turns"] or initial["turns"][-1]["status"] != "completion_not_observed":
        print(json.dumps({"kind": "already_terminal_or_missing_turn"}), flush=True)
        return
    target = initial["turns"][-1]["id"]
    last_absent = time.time_ns()
    deadline = time.perf_counter() + args.watch_seconds
    while time.perf_counter() < deadline:
        snapshot = inspect_rollout(args.path, args.thread_id)
        observed = time.time_ns()
        turn = next(t for t in snapshot["turns"] if t["id"] == target)
        if turn["status"] != "completion_not_observed":
            event_ns = int(datetime.fromisoformat(turn["terminal_timestamp"].replace("Z", "+00:00")).timestamp() * 1e9)
            print(json.dumps({"kind": "terminal_observed", "thread_id": args.thread_id,
                              "turn_id": target, "status": turn["status"],
                              "terminal_timestamp": turn["terminal_timestamp"],
                              "last_absent_unix_ns": last_absent, "first_observed_unix_ns": observed,
                              "event_to_observed_ms": (observed - event_ns) / 1e6,
                              "poll_seconds": 0.25, "sha256": snapshot["sha256"]}), flush=True)
            return
        last_absent = observed
        time.sleep(0.25)
    print(json.dumps({"kind": "timeout_without_terminal", "turn_id": target}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({"kind": "probe_error", "error": type(error).__name__}), flush=True)
        raise SystemExit(2)
