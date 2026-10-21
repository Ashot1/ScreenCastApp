import json
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest

from read_rollout import inspect_rollout


VERSION = "0.163.0-alpha.10.2"


def record(kind, payload):
    return json.dumps({"timestamp": "2026-10-18T19:00:00.000Z", "type": kind,
                       "payload": payload}, ensure_ascii=False).encode() + b"\n"


def metadata(thread="test-thread", version=VERSION, history_mode="paginated"):
    return record("session_meta", {"id": thread, "cli_version": version,
                                   "originator": "Codex Desktop", "source": "vscode", "history_mode": history_mode})


def event(kind, turn="turn-1", **fields):
    return record("event_msg", {"type": kind, "turn_id": turn, **fields})


class RolloutTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "rollout.jsonl"

    def read(self, data, thread="test-thread"):
        self.path.write_bytes(data)
        return inspect_rollout(self.path, thread)

    def test_saved_completion_preserves_prompt_plan_and_turn_identity(self):
        prompt = "Проверь π <элемент> без изменений.\nТочный текст."
        data = metadata() + event("task_started")
        data += record("inter_agent_communication_metadata", {"trigger_turn": {}})
        data += record("response_item", {"type": "message", "role": "user",
                       "content": [{"type": "input_text", "text": prompt}]})
        data += record("event_msg", {"type": "item_completed", "thread_id": "test-thread",
                       "turn_id": "turn-1", "item": {"type": "plan", "text": "1. Read; 2. Check"}})
        data += event("task_complete") + event("task_started", "turn-2")
        result = self.read(data)
        self.assertIsInstance(result, dict)
        self.assertEqual(result["thread_id"], "test-thread")
        self.assertEqual(result.get("history_mode"), "paginated")
        self.assertEqual(result.get("coverage"), "local_records_only")
        self.assertEqual(result["turns"][0]["messages"][0]["text"], prompt)
        self.assertEqual(result["turns"][0]["plans"], [{"type": "plan", "text": "1. Read; 2. Check"}])
        self.assertEqual([(t["id"], t["status"]) for t in result["turns"]],
                         [("turn-1", "completed"), ("turn-2", "completion_not_observed")])
        self.assertEqual(self.path.read_bytes(), data)

    def test_delegated_prompt_is_read_from_saved_tool_input_without_loss(self):
        text = "<codex_delegation>\n<input>Точная задача <tag> & π\n</input>\n</codex_delegation>"
        data = metadata() + event("task_started") + record("response_item", {
            "type": "function_call_output", "namespace": "codex_app", "name": "create_thread", "output": text})
        result = self.read(data)
        self.assertEqual(result["turns"][0]["messages"][0]["text"], text)

    def test_partial_completion_does_not_complete_until_append_finishes(self):
        base = metadata() + event("task_started")
        end = event("task_complete")
        result = self.read(base + end[:40])
        self.assertTrue(result["partial_tail"])
        self.assertEqual(result["turns"][0]["status"], "completion_not_observed")
        with self.path.open("ab") as target:
            target.write(end[40:])
        self.assertEqual(inspect_rollout(self.path, "test-thread")["turns"][0]["status"], "completed")

    def test_absent_terminal_is_unknown_and_abort_is_not_completion(self):
        result = self.read(metadata() + event("task_started") + event("turn_aborted"))
        self.assertEqual(result["turns"][0]["status"], "aborted")

    def test_failed_completion_and_unattributed_abort_are_not_success(self):
        result = self.read(metadata() + event("task_started") + event("task_complete", error={"code": "test"}))
        self.assertEqual(result["turns"][0]["status"], "failed")
        with self.assertRaises(ValueError):
            self.read(metadata() + event("task_started") + record("event_msg", {"type": "turn_aborted"}))

    def test_foreign_event_identity_and_incomplete_history_are_rejected(self):
        data = metadata() + event("task_started") + record("event_msg", {
            "type": "item_completed", "thread_id": "another-thread", "turn_id": "turn-1", "item": {"type": "plan"}})
        with self.assertRaises(ValueError):
            self.read(data)
        with self.assertRaises(ValueError):
            self.read(metadata(history_mode="legacy") + record("compacted", {"message": "missing prior context"}))

    def test_paginated_compaction_preserves_canonical_user_identity_and_completion(self):
        data = metadata() + event("task_started")
        data += record("response_item", {"type": "message", "role": "user", "id": "provider-message",
                       "content": [{"type": "input_text", "text": "Original user request π"}]})
        data += record("event_msg", {"type": "item_completed", "thread_id": "test-thread", "turn_id": "turn-1",
                       "item": {"type": "UserMessage", "id": "human-message", "client_id": None,
                                "content": [{"type": "text", "text": "Original user request π", "text_elements": []}]}})
        data += record("compacted", {"message": "summary", "window_number": 2,
                       "replacement_history": [{"type": "message", "role": "user", "content": [
                           {"type": "input_text", "text": "Replacement context must not become new input"}]}]})
        data += event("task_complete")
        try:
            result = self.read(data)
        except ValueError as error:
            self.fail(f"Known paginated compaction was rejected: {error}")
        self.assertEqual(result.get("compactions_observed"), 1)
        self.assertIs(result.get("model_context_reconstructed"), False)
        self.assertEqual(result["turns"][0]["status"], "completed")
        canonical = [m for m in result["turns"][0]["messages"] if m["role"] == "canonical_user"]
        self.assertEqual(canonical, [{"id": "human-message", "role": "canonical_user", "phase": None,
                                     "text": "Original user request π"}])
        self.assertFalse(any("Replacement context" in m["text"] for m in result["turns"][0]["messages"]))
        self.assertEqual(self.path.read_bytes(), data)

    def test_compaction_does_not_hide_unknown_record_or_unmatched_user_identity(self):
        with self.assertRaises(ValueError):
            self.read(metadata() + record("compacted", {}) + record("future_checkpoint", {}))
        with self.assertRaises(ValueError):
            self.read(metadata() + event("task_started") + record("event_msg", {
                "type": "item_completed", "thread_id": "test-thread", "turn_id": "missing-turn",
                "item": {"type": "UserMessage", "id": "human-message", "content": []}}))

    def test_foreign_thread_or_version_is_rejected(self):
        for data in (metadata("another-thread"), metadata(version="future-version")):
            with self.subTest(data=data), self.assertRaises(ValueError):
                self.read(data)

    def test_unknown_format_and_orphan_completion_are_rejected(self):
        for data in (b"{\"new_format\":true}\n", metadata() + b"bad json\n",
                     metadata() + event("task_complete"), metadata() + event("future_terminal")):
            with self.subTest(data=data), self.assertRaises(ValueError):
                self.read(data)

    def test_missing_and_permission_denied_files_are_reported(self):
        with self.assertRaises(FileNotFoundError):
            inspect_rollout(self.path, "test-thread")
        self.path.write_bytes(metadata())
        self.path.chmod(0)
        self.addCleanup(self.path.chmod, 0o600)
        with self.assertRaises(PermissionError):
            inspect_rollout(self.path, "test-thread")

    def test_large_history_is_rejected_without_unbounded_read(self):
        with self.path.open("wb") as target:
            target.truncate(16 * 1024 * 1024 + 1)
        with self.assertRaises(ValueError):
            inspect_rollout(self.path, "test-thread")

    def test_cli_observes_real_append_without_exporting_source_text(self):
        text = "Private fixture text must stay in memory."
        data = metadata() + event("task_started") + record("response_item", {
            "type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]})
        self.path.write_bytes(data)
        process = subprocess.Popen([sys.executable, "-B", str(Path(__file__).with_name("read_rollout.py")),
                                    str(self.path), "test-thread", "--watch-seconds", "3"],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            initial = process.stdout.readline()
            self.assertEqual(json.loads(initial)["turns"][0]["status"], "completion_not_observed")
            with self.path.open("ab") as target:
                target.write(event("task_complete"))
            output, errors = process.communicate(timeout=5)
            self.assertEqual(process.returncode, 0, errors)
            self.assertEqual(json.loads(output)["kind"], "terminal_observed")
            self.assertNotIn(text, initial + output)
            self.assertEqual(self.path.read_bytes(), data + event("task_complete"))
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()


if __name__ == "__main__":
    unittest.main()
