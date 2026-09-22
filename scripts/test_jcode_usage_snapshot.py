#!/usr/bin/env python3
"""Focused tests for scripts/jcode_usage_snapshot.py."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).with_name("jcode_usage_snapshot.py")
SPEC = importlib.util.spec_from_file_location("jcode_usage_snapshot", SCRIPT)
assert SPEC and SPEC.loader
snapshot = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(snapshot)


def message(timestamp: str, usage: dict | None = None, text: str = "") -> dict:
    value = {
        "role": "assistant" if usage else "user",
        "timestamp": timestamp,
        "content": [{"type": "text", "text": text}],
    }
    if usage is not None:
        value["token_usage"] = usage
    return value


def write_session(root: pathlib.Path, name: str, provider: str, model: str,
                  messages: list[dict]) -> pathlib.Path:
    path = root / f"{name}.json"
    path.write_text(json.dumps({
        "id": name,
        "provider_key": provider,
        "model": model,
        "messages": messages,
    }))
    return path


class UsageSnapshotTests(unittest.TestCase):
    def test_effective_context_supports_subset_and_split_accounting(self) -> None:
        self.assertEqual(snapshot.effective_context_tokens("openai", 100, 80, 0), 100)
        self.assertEqual(snapshot.effective_context_tokens("claude", 20, 80, 5), 105)
        self.assertEqual(snapshot.effective_context_tokens("openai", 10, 20, 0), 30)

    def test_snapshot_uses_raw_per_call_values_and_classifies_automation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            write_session(root, "openai", "openai", "gpt", [
                message("2026-08-14T00:00:01Z", text="[automated follow-up] Continue the work below"),
                message("2026-08-14T00:00:02Z", {"input_tokens": 100, "output_tokens": 10,
                                                     "cache_read_input_tokens": 80}),
                message("2026-08-14T00:00:03Z", {"input_tokens": 300, "output_tokens": 20,
                                                     "cache_read_input_tokens": 200}),
            ])
            write_session(root, "claude", "claude", "sonnet", [
                message("2026-08-14T00:00:04Z", text="[automated todo quality review] incomplete todos"),
                message("2026-08-14T00:00:05Z", {"input_tokens": 20, "output_tokens": 10,
                                                     "cache_read_input_tokens": 80,
                                                     "cache_creation_input_tokens": 5}),
            ])
            start = snapshot._parse_stamp("2026-08-14T00:00:00Z")
            end = snapshot._parse_stamp("2026-08-14T01:00:00Z")
            result = snapshot.build_snapshot(root, start, end, root, "UTC")

        totals = result["totals"]
        self.assertEqual(totals["calls"], 3)
        self.assertEqual(totals["effective_tokens"], 110 + 320 + 115)
        self.assertEqual(totals["automated_followup"], 1)
        self.assertEqual(totals["automated_review"], 1)
        self.assertEqual(totals["incomplete_todo_nudge"], 1)
        self.assertEqual(result["per_call_effective_tokens"]["p50"], 115)
        self.assertEqual(result["per_call_effective_tokens"]["max"], 320)

    def test_window_excludes_late_messages_and_outputs_are_parseable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            write_session(root, "window", "openai", "gpt", [
                message("2026-08-13T23:59:59Z", {"input_tokens": 999}),
                message("2026-08-14T00:00:01Z", {"input_tokens": 40, "output_tokens": 2}),
                message("2026-08-14T01:00:00Z", {"input_tokens": 999}),
                message("2026-08-14T01:00:01Z", {"input_tokens": 999}),
            ])
            json_out, md_out = root / "out.json", root / "out.md"
            rc = snapshot.main([
                "--start", "2026-08-14T00:00:00Z",
                "--end", "2026-08-14T01:00:00Z",
                "--sessions-root", str(root),
                "--git-dir", str(root),
                "--local-tz", "UTC",
                "--json-out", str(json_out),
                "--markdown-out", str(md_out),
            ])
            data = json.loads(json_out.read_text())
            markdown = md_out.read_text()

        self.assertEqual(rc, 0)
        self.assertEqual(data["totals"]["calls"], 1)
        self.assertEqual(data["totals"]["effective_tokens"], 42)
        self.assertIn("not certified billing usage", markdown)

    def test_export_includes_sessions_beyond_the_display_top_twenty(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for i in range(25):
                path = write_session(root, f"s{i}", "openai", "gpt", [
                    message("2026-08-14T00:00:01Z", {"input_tokens": i + 1}),
                ])
                data = json.loads(path.read_text())
                data["working_dir"] = f"/project/{i}"
                path.write_text(json.dumps(data))
            result = snapshot.build_snapshot(
                root, snapshot._parse_stamp("2026-08-14T00:00:00Z"),
                snapshot._parse_stamp("2026-08-15T00:00:00Z"), root, "UTC")
        self.assertEqual(len(result["sessions"]), 25)
        self.assertEqual(len(result["top_sessions"]), 20)
        self.assertEqual(sum(s["effective_tokens"] for s in result["sessions"]),
                         result["totals"]["effective_tokens"])
        self.assertEqual(len({s["working_dir"] for s in result["sessions"]}), 25)
        self.assertEqual(result["model_attribution"],
                         "current_session_metadata_not_historical_call")


if __name__ == "__main__":
    unittest.main()
