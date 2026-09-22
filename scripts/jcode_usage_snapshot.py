#!/usr/bin/env python3
"""Deterministic, provider-aware JCode usage snapshot.

Effective context cost mirrors
``crates/jcode-compaction-core/src/lib.rs::effective_context_tokens_from_usage``:
split accounting (Anthropic/Claude, ``cache_creation > 0``, or
``cache_read > input``) sums the three counters, otherwise the input already
includes any cached portion. Schema: ``jcode-usage-snapshot/v1``.

USAGE
    python3 scripts/jcode_usage_snapshot.py \
        --start 2026-08-13T22:00:00Z --end 2026-08-14T17:24:10Z \
        --json-out docs/benchmarks/jcode-usage-2026-08-14-baseline.json \
        --markdown-out docs/benchmarks/jcode-usage-2026-08-14-baseline.md
"""

from __future__ import annotations

import argparse
import collections
import datetime as dt
import json
import pathlib
import re
import subprocess
from typing import Any, Dict, List, Optional, Tuple

DEFAULT_SESSIONS_ROOT = pathlib.Path.home() / ".jcode" / "sessions"
DEFAULT_LOCAL_TZ = "Europe/Berlin"
DEFAULT_GIT_DIR = pathlib.Path(__file__).resolve().parent.parent
SCHEMA_VERSION = "jcode-usage-snapshot/v1"
SR_RE = re.compile(r"^\s*<system-reminder>", re.IGNORECASE)
AUTOMATED_REVIEW_RE = re.compile(r"\[\s*automated\s+(review|todo)\b", re.IGNORECASE)
AUTOMATED_FOLLOWUP_RE = re.compile(r"\[\s*automated\s+follow[-_]?up\b", re.IGNORECASE)
CONTINUE_RE = re.compile(r"continue the work below", re.IGNORECASE)
INCOMPLETE_TODO_RE = re.compile(r"incomplete todos", re.IGNORECASE)
EMPTY_RETRY_RE = re.compile(r"previous provider response was empty", re.IGNORECASE)
# Fields aggregated into totals, per-model, and top-sessions tables.
SESSION_FIELDS = (
    "messages", "calls", "input_tokens", "output_tokens",
    "cache_read_input_tokens", "cache_creation_input_tokens", "effective_tokens",
    "split_accounting_calls", "system_reminder", "automated_review",
    "automated_followup", "incomplete_todo_nudge", "empty_response_retry",
)


def effective_context_tokens(provider: str, input_tokens: int,
                             cache_read: int, cache_creation: int) -> int:
    """Provider-aware effective input matching the Rust context heuristic."""
    if input_tokens == 0:
        return 0
    p = provider.lower()
    split = ("anthropic" in p or "claude" in p
             or cache_creation > 0 or cache_read > input_tokens)
    return input_tokens + cache_read + cache_creation if split else input_tokens


def parse_ts(value: Any) -> Optional[dt.datetime]:
    if not value or not isinstance(value, str):
        return None
    try:
        ts = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if ts.tzinfo is None:
        ts = ts.replace(tzinfo=dt.timezone.utc)
    return ts


def classify_user_text(text: str) -> Dict[str, int]:
    """Return per-message automation counts (each is 0 or 1)."""
    return {
        "system_reminder": int(bool(SR_RE.match(text))),
        "automated_review": int(bool(AUTOMATED_REVIEW_RE.search(text))),
        "automated_followup": int(bool(AUTOMATED_FOLLOWUP_RE.search(text)
                                       or CONTINUE_RE.search(text))),
        "incomplete_todo_nudge": int(bool(INCOMPLETE_TODO_RE.search(text))),
        "empty_response_retry": int(bool(EMPTY_RETRY_RE.search(text))),
    }


def message_text(message: Dict[str, Any]) -> str:
    return "\n".join(b.get("text", "") for b in (message.get("content") or [])
                     if isinstance(b, dict) and isinstance(b.get("text"), str))


def aggregate_session(path: pathlib.Path, start: dt.datetime,
                      end: dt.datetime) -> Optional[Dict[str, Any]]:
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None
    provider = (data.get("provider_key") or "").strip() or "unknown"
    model = data.get("model") or "unknown"
    s: Dict[str, Any] = {"session": path.name, "provider": provider,
                          "model": model, "working_dir": data.get("working_dir"),
                          "model_attribution": "current_session_metadata_not_historical_call",
                          "_per_call_effective": []}
    for f in SESSION_FIELDS:
        s[f] = 0
    p_low = provider.lower()
    is_split = ("anthropic" in p_low or "claude" in p_low)
    for message in data.get("messages") or []:
        ts = parse_ts(message.get("timestamp"))
        if ts is None or not (start <= ts < end):
            continue
        s["messages"] += 1
        if message.get("role") == "user":
            for key, val in classify_user_text(message_text(message)).items():
                s[key] += val
        usage = message.get("token_usage")
        if not isinstance(usage, dict):
            continue
        inp = int(usage.get("input_tokens") or 0)
        out = int(usage.get("output_tokens") or 0)
        cread = int(usage.get("cache_read_input_tokens") or 0)
        ccreate = int(usage.get("cache_creation_input_tokens") or 0)
        s["calls"] += 1
        s["input_tokens"] += inp
        s["output_tokens"] += out
        s["cache_read_input_tokens"] += cread
        s["cache_creation_input_tokens"] += ccreate
        eff_total = effective_context_tokens(provider, inp, cread, ccreate) + out
        s["effective_tokens"] += eff_total
        s["_per_call_effective"].append(eff_total)
        if inp > 0 and (is_split or ccreate > 0 or cread > inp):
            s["split_accounting_calls"] += 1
    if s["messages"] == 0:
        return None
    return s


def per_call_stats(raw_per_call: List[int]) -> Dict[str, float]:
    if not raw_per_call:
        return {"p50": 0, "p90": 0, "p99": 0, "max": 0, "mean": 0.0}
    ordered = sorted(raw_per_call)
    n = len(ordered) - 1
    return {
        "p50": ordered[int(0.5 * n)],
        "p90": ordered[int(0.9 * n)],
        "p99": ordered[int(0.99 * n)],
        "max": ordered[-1],
        "mean": sum(ordered) / len(ordered),
    }


def aggregate(sessions: List[Dict[str, Any]], start: dt.datetime,
              end: dt.datetime) -> Dict[str, Any]:
    by_model: Dict[str, Dict[str, Any]] = collections.defaultdict(
        lambda: collections.Counter())
    counters: Dict[str, Any] = collections.Counter()
    raw_per_call: List[int] = []
    for s in sessions:
        for f in SESSION_FIELDS:
            counters[f] += s[f]
            by_model[f"{s['provider']}::{s['model']}"][f] += s[f]
        raw_per_call.extend(s["_per_call_effective"])
    hours = max((end - start).total_seconds() / 3600.0, 1e-9)
    models = [{"model": k, **dict(v)} for k, v in sorted(
        by_model.items(), key=lambda kv: kv[1]["effective_tokens"], reverse=True)]
    top = sorted(
        ({k: v for k, v in s.items() if k != "_per_call_effective"}
         for s in sessions),
        key=lambda s: s["effective_tokens"], reverse=True,
    )
    return {
        "totals": dict(counters),
        "per_call_effective_tokens": per_call_stats(raw_per_call),
        "models": models,
        "sessions": top,
        "top_sessions": top[:20],
        "session_coverage": "all_parsed_sessions",
        "model_attribution": "current_session_metadata_not_historical_call",
        "rates": {
            "calls_per_hour": counters["calls"] / hours,
            "effective_tokens_per_hour": counters["effective_tokens"] / hours,
        },
    }


def git_revision(repo: pathlib.Path) -> Tuple[str, bool]:
    try:
        sha = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"], cwd=str(repo),
            stderr=subprocess.DEVNULL).decode().strip()
        dirty = bool(subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=str(repo),
            stderr=subprocess.DEVNULL).decode().strip())
        return sha, dirty
    except (subprocess.CalledProcessError, OSError):
        return "unknown", False


def _tz(name: str) -> dt.tzinfo:
    try:
        from zoneinfo import ZoneInfo
        return ZoneInfo(name)
    except Exception:
        return dt.timezone.utc


def build_snapshot(sessions_root: pathlib.Path, start: dt.datetime,
                   end: dt.datetime, git_dir: pathlib.Path,
                   local_tz: str) -> Dict[str, Any]:
    sessions: List[Dict[str, Any]] = []
    files_parsed = parse_errors = 0
    for path in sorted(sessions_root.glob("*.json")):
        files_parsed += 1
        try:
            json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            parse_errors += 1
            continue
        stats = aggregate_session(path, start, end)
        if stats is not None:
            sessions.append(stats)
    snap = aggregate(sessions, start, end)
    sha, dirty = git_revision(git_dir)
    local_zone = _tz(local_tz)
    start_local = start.astimezone(local_zone)
    hours = (end - start).total_seconds() / 3600.0
    offset_h = int((start_local.utcoffset() or dt.timedelta(0)).total_seconds() // 3600)
    tz_label = "UTC" if not offset_h else "CEST" if offset_h == 2 else f"UTC{offset_h:+03d}:00"
    return {
        "schema": SCHEMA_VERSION,
        "interval": {
            "start_utc": start.astimezone(dt.timezone.utc).isoformat(),
            "end_utc": end.astimezone(dt.timezone.utc).isoformat(),
            "start_local": start_local.isoformat(),
            "timezone": tz_label,
            "hours": hours,
        },
        "source": {
            "session_root": str(sessions_root),
            "files_parsed": files_parsed,
            "parse_errors": parse_errors,
            "sessions_in_window": len(sessions),
        },
        "methodology": {
            "effective_input": ("split when provider contains anthropic/claude, "
                                "cache_creation>0, or cache_read>input; otherwise "
                                "input already includes cache"),
            "effective_tokens": "effective_input + output",
            "percentile": "nearest-rank on sorted per-call effective totals",
            "caveat": ("comparison metric, not certified billing usage; session-level "
                       "provider/model metadata can be stale after model switches, and "
                       "live session files can flush messages after a capture"),
            "automation_classes": (
                "system_reminder: text starts with <system-reminder>; "
                "automated_review: [automated review] or [automated todo]; "
                "automated_followup: [automated follow-up] or 'continue the work below'; "
                "incomplete_todo_nudge: 'incomplete todos'; "
                "empty_response_retry: 'previous provider response was empty'"),
            "git_revision": sha, "git_dirty": dirty, "git_dir": str(git_dir),
        },
        **snap,
    }


def render_markdown(snap: Dict[str, Any]) -> str:
    iv, src, meth, totals, pcd, rates, models, top = (
        snap["interval"], snap["source"], snap["methodology"], snap["totals"],
        snap["per_call_effective_tokens"], snap["rates"], snap["models"],
        snap["top_sessions"])
    L: List[str] = [f"# JCode Usage Snapshot ({snap['schema']})", ""]
    L.append(f"Window: `{iv['start_utc']}` → `{iv['end_utc']}` UTC "
             f"({iv['start_local']} {iv['timezone']}, {iv['hours']:.4f} h)")
    L.append(f"Source: `{src['session_root']}` — {src['files_parsed']} files, "
             f"{src['sessions_in_window']} sessions in window, "
             f"{src['parse_errors']} parse errors")
    L.append(f"Git revision: `{meth['git_revision']}` (dirty={meth['git_dirty']})")
    L += ["", "## Totals", ""]
    L.append(f"- Sessions: **{src['sessions_in_window']}**, messages: **{totals['messages']}**")
    L.append(f"- Calls: **{totals['calls']:,}** "
             f"(split {totals['split_accounting_calls']})")
    L.append(f"- Calls/hour: **{rates['calls_per_hour']:.4f}**")
    L.append(f"- Effective tokens: **{totals['effective_tokens']:,}**")
    L.append(f"- Effective tokens/hour: **{rates['effective_tokens_per_hour']:,.2f}**")
    L += ["", "## Per-call effective distribution", "",
          "| p50 | p90 | p99 | max | mean |",
          "| ---: | ---: | ---: | ---: | ---: |",
          f"| {pcd['p50']:,} | {pcd['p90']:,} | {pcd['p99']:,} | "
          f"{pcd['max']:,} | {pcd['mean']:.4f} |",
          "", "## Automation counts", ""]
    for label, key in (("System reminders", "system_reminder"),
                        ("Automated review prompts", "automated_review"),
                        ("Automated follow-ups", "automated_followup"),
                        ("Incomplete-todo nudges", "incomplete_todo_nudge"),
                        ("Empty-response retries", "empty_response_retry")):
        L.append(f"- {label}: {totals[key]}")
    L += ["", "## Model breakdown", "",
          "| Model | Calls | Input | Output | Cache read | Cache create | Effective |",
          "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for m in models:
        L.append(f"| `{m['model']}` | {m.get('calls', 0)} | {m.get('input_tokens', 0):,} | "
                 f"{m.get('output_tokens', 0):,} | {m.get('cache_read_input_tokens', 0):,} | "
                 f"{m.get('cache_creation_input_tokens', 0):,} | {m.get('effective_tokens', 0):,} |")
    L += ["", "## Top sessions", "",
          "| Session | Model | Calls | Effective |",
          "| --- | --- | ---: | ---: |"]
    for s in top:
        L.append(f"| `{s['session']}` | {s['model']} | {s['calls']} | "
                 f"{s['effective_tokens']:,} |")
    L += ["", "## Methodology", "", meth["effective_input"], "",
          "Effective tokens = effective_input + output_tokens.",
          f"Caveat: {meth['caveat']}", meth["automation_classes"], ""]
    return "\n".join(L)


def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--start", required=True, help="ISO-8601 start timestamp")
    p.add_argument("--end", required=True, help="ISO-8601 end timestamp")
    p.add_argument("--sessions-root", default=str(DEFAULT_SESSIONS_ROOT))
    p.add_argument("--json-out", default=None, help="Write JSON snapshot to this path")
    p.add_argument("--markdown-out", default=None, help="Write Markdown report to this path")
    p.add_argument("--git-dir", default=str(DEFAULT_GIT_DIR))
    p.add_argument("--local-tz", default=DEFAULT_LOCAL_TZ)
    return p.parse_args(argv)


def _parse_stamp(value: str) -> dt.datetime:
    stamp = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if stamp.tzinfo is None:
        stamp = stamp.replace(tzinfo=dt.timezone.utc)
    return stamp


def main(argv: Optional[List[str]] = None) -> int:
    args = parse_args(argv)
    start = _parse_stamp(args.start)
    end = _parse_stamp(args.end)
    if end <= start:
        raise SystemExit(f"--end must be after --start (got {start!r} -> {end!r})")
    sessions_root = pathlib.Path(args.sessions_root)
    if not sessions_root.exists():
        raise SystemExit(f"--sessions-root does not exist: {sessions_root}")
    snap = build_snapshot(sessions_root, start, end,
                          pathlib.Path(args.git_dir), args.local_tz)
    if args.json_out:
        out = pathlib.Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(snap, indent=2, sort_keys=False) + "\n")
    if args.markdown_out:
        out = pathlib.Path(args.markdown_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(render_markdown(snap) + "\n")
    if not args.json_out and not args.markdown_out:
        print(render_markdown(snap))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
