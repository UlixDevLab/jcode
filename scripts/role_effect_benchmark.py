#!/usr/bin/env python3
"""A/B benchmark: does having role files change agent behavior?

Runs the same task twice, once with the global role catalog present and once
with it hidden, and scores each answer against facts read from the repository
rather than from the answer.

Design decisions that matter for trusting the result:

- **Arms alternate and repeat.** A single pair cannot separate an effect from
  run-to-run variance, which is large for LLM runs. Order alternates per
  repetition so a warming cache cannot systematically favor one arm.
- **Ground truth is grep, not judgement.** Each task declares literal strings
  that must appear, taken from the repo. No model grades another model.
- **The control is the real absence of roles.** The global roles directory is
  renamed for the duration of a control run and restored immediately, including
  on crash, so a failed run cannot leave the operator without roles.
- **Failure is reported, not smoothed.** A timeout or non-zero exit is recorded
  as a failed run rather than dropped, because dropping failures is how a
  benchmark quietly starts measuring only its successes.

Usage:
    python3 role_effect_benchmark.py --reps 3
    python3 role_effect_benchmark.py --task devops-facts --reps 5
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

HOME = Path.home()
ROLES_DIR = HOME / ".jcode" / "roles"
ROLES_HIDDEN = HOME / ".jcode" / "roles.benchmark-hidden"
LEGRIN = HOME / "Documents" / "LeGrin.tech"

# Default to the locally built binary so a benchmark run measures the code under
# test rather than whatever happens to be first on PATH.
DEFAULT_BINARY = HOME / ".jcode" / "source" / "jcode" / "target" / "release" / "jcode"

RUN_TIMEOUT_SECONDS = 420


@dataclass
class Task:
    """One benchmark task.

    `expect` holds literal strings that a correct answer must contain. They are
    facts checked out of the repository, so scoring is mechanical.
    """

    id: str
    cwd: Path
    prompt: str
    expect: dict[str, str]
    # Strings whose presence indicates a specific known failure, e.g. asserting
    # that a fact is absent when it is committed.
    antipatterns: dict[str, str] = field(default_factory=dict)


TASKS: list[Task] = [
    Task(
        id="devops-facts",
        cwd=LEGRIN / "ULIX" / "UlixDevLab",
        prompt=(
            "What host does this project deploy to in production, what container name "
            "does the app run as, and what is the exact Docker socket path used to "
            "reach it? Answer in three short lines. Do not change anything."
        ),
        expect={
            "host": "ulix-dev-lab",
            "container": "ulix-prod-ulix-1",
            "socket": "/run/user/1100/docker.sock",
        },
        antipatterns={
            # The control arm produced this once: the hostname is in README.md.
            "claims-no-hostname": r"no hostname.{0,40}committed|hostname.{0,20}not committed",
        },
    ),
    Task(
        id="frontend-classify",
        cwd=LEGRIN / "Life OS" / "web",
        prompt=(
            "Before writing any code: does this frontend already use an atomic design "
            "structure, what is its token source file, and what command runs its tests? "
            "Answer in three short lines. Do not change anything."
        ),
        expect={
            "atomic": "atoms",
            "tokens": "tokens.css",
            # `npm test` here is a chain (gen:icons + lint scripts), so accept
            # the script name rather than a specific sub-command.
            "tests": "npm test",
        },
    ),
    Task(
        id="migration-safety",
        cwd=LEGRIN / "ULIX" / "UlixDevLab",
        prompt=(
            "Where do new database migrations go in this repository, what tool applies "
            "them, and what triggers the migration workflow? Answer in three short "
            "lines. Do not change anything."
        ),
        expect={
            "location": "supabase/migrations",
            "tool": "sqitch",
            "trigger": "workflow_dispatch",
        },
    ),
]


@dataclass
class Run:
    arm: str
    task: str
    rep: int
    ok: bool
    seconds: float
    facts_found: int
    facts_total: int
    antipatterns_hit: list[str]
    output: str


class RolesHidden:
    """Hide the global role catalog for the duration of a control run."""

    def __enter__(self) -> "RolesHidden":
        if ROLES_HIDDEN.exists():
            # A previous crashed run left roles hidden. Restore before doing
            # anything else, rather than stacking another move on top.
            shutil.move(str(ROLES_HIDDEN), str(ROLES_DIR))
        if ROLES_DIR.exists():
            shutil.move(str(ROLES_DIR), str(ROLES_HIDDEN))
        return self

    def __exit__(self, *_exc) -> None:
        if ROLES_HIDDEN.exists():
            shutil.move(str(ROLES_HIDDEN), str(ROLES_DIR))


def run_once(binary: Path, task: Task, arm: str, rep: int) -> Run:
    started = time.time()
    try:
        completed = subprocess.run(
            [str(binary), "run", task.prompt],
            cwd=task.cwd,
            capture_output=True,
            text=True,
            timeout=RUN_TIMEOUT_SECONDS,
        )
        output = completed.stdout + completed.stderr
        ok = completed.returncode == 0
    except subprocess.TimeoutExpired:
        output = f"(timed out after {RUN_TIMEOUT_SECONDS}s)"
        ok = False
    elapsed = time.time() - started

    lowered = output.lower()
    found = sum(1 for value in task.expect.values() if value.lower() in lowered)
    hits = [
        name
        for name, pattern in task.antipatterns.items()
        if re.search(pattern, lowered, re.I)
    ]

    return Run(
        arm=arm,
        task=task.id,
        rep=rep,
        ok=ok,
        seconds=elapsed,
        facts_found=found,
        facts_total=len(task.expect),
        antipatterns_hit=hits,
        output=output,
    )


def summarize(runs: list[Run], arm: str, task_id: str | None = None) -> dict:
    subset = [r for r in runs if r.arm == arm and (task_id is None or r.task == task_id)]
    if not subset:
        return {}
    accuracy = [r.facts_found / r.facts_total for r in subset]
    times = [r.seconds for r in subset]
    return {
        "runs": len(subset),
        "failed_runs": sum(1 for r in subset if not r.ok),
        "accuracy_mean": statistics.mean(accuracy),
        "accuracy_min": min(accuracy),
        "seconds_median": statistics.median(times),
        "antipattern_hits": sum(len(r.antipatterns_hit) for r in subset),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reps", type=int, default=3, help="repetitions per arm per task")
    parser.add_argument("--task", help="run only this task id")
    parser.add_argument("--binary", default=str(DEFAULT_BINARY))
    parser.add_argument("--json", help="write raw results here")
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.is_file():
        print(f"binary not found: {binary}", file=sys.stderr)
        return 3

    tasks = [t for t in TASKS if args.task is None or t.id == args.task]
    if not tasks:
        print(f"no task matching {args.task}", file=sys.stderr)
        return 3
    for task in tasks:
        if not task.cwd.is_dir():
            print(f"task {task.id}: missing repo {task.cwd}", file=sys.stderr)
            return 3

    runs: list[Run] = []
    for task in tasks:
        for rep in range(1, args.reps + 1):
            # Alternate which arm goes first so cache warming cannot
            # systematically favor one of them.
            order = ["with", "without"] if rep % 2 else ["without", "with"]
            for arm in order:
                print(f"  {task.id} rep{rep} [{arm}] ...", end="", flush=True)
                if arm == "without":
                    with RolesHidden():
                        run = run_once(binary, task, arm, rep)
                else:
                    run = run_once(binary, task, arm, rep)
                runs.append(run)
                flag = "" if run.ok else "  RUN FAILED"
                print(
                    f" {run.seconds:5.1f}s  {run.facts_found}/{run.facts_total}{flag}"
                )

    print()
    print(f"{'task':<20} {'arm':<9} {'runs':>4} {'acc':>6} {'median s':>9} {'anti':>5}")
    for task in tasks:
        for arm in ("with", "without"):
            s = summarize(runs, arm, task.id)
            if not s:
                continue
            print(
                f"{task.id:<20} {arm:<9} {s['runs']:>4} "
                f"{s['accuracy_mean']:>5.0%} {s['seconds_median']:>8.1f}s "
                f"{s['antipattern_hits']:>5}"
            )

    print()
    for arm in ("with", "without"):
        s = summarize(runs, arm)
        print(
            f"OVERALL {arm:<8} accuracy {s['accuracy_mean']:.0%} "
            f"(worst {s['accuracy_min']:.0%})  median {s['seconds_median']:.1f}s  "
            f"failed runs {s['failed_runs']}  antipatterns {s['antipattern_hits']}"
        )

    if args.json:
        Path(args.json).write_text(
            json.dumps([r.__dict__ for r in runs], indent=2, default=str)
        )
        print(f"\nraw results: {args.json}")

    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    finally:
        # Never leave the operator without roles, whatever happened above.
        if ROLES_HIDDEN.exists() and not ROLES_DIR.exists():
            shutil.move(str(ROLES_HIDDEN), str(ROLES_DIR))
