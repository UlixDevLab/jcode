---
name: decision-board
description: >-
  Активує раду ролей для стратегічних рішень (запуск, найм, інвестиція,
  зміна моделі, ціни). CFO, Skeptic, Client, Premortem/Taleb паралельно
  на різних не-second-party маршрутах, потім синтез координатором.
  Trigger: /decision-board,
---

# Decision Board

Deterministic sequence. The coordinator (active main non-second-party route)
does not play roles itself. Each role is a separate worker on its own
model.

## Stage 0 — Frame (coordinator, no delegation)

State in 3-5 lines: decision, options (A/B/at least two), context,
constraints, what is known from numbers. This is the single brief all roles
receive. No sensitive data the role doesn't need.

## Stage 1 — Parallel voices (one stage, 4-5 workers)

Spawn 4-5 workers concurrently. Each prompt: "Read <path to role SKILL.md>.
Respond strictly in this role. Brief: <frame>. Return: VERDICT (1 line),
3-5 ARGUMENTS with numbers where possible, MAIN RISK, WHAT WOULD CHANGE
YOUR MIND."

| Role | SKILL.md | Route | Effort |
|------|----------|-------|--------|
| CFO | cfo/SKILL.md | main coordinator | high |
| Skeptic | skeptic/SKILL.md | independent non-second-party | high |
| Client | client/SKILL.md | implementation pool | medium |
| Premortem | premortem/SKILL.md | independent non-second-party | high |
| Taleb (optional) | taleb/SKILL.md | implementation pool | medium |

Optionally add (by decision type): Investor (scale), Competitor (market),
PE-Analyst (structure) — on any route from a different provider to keep
prior independence.

Workers return compact STATE, EVIDENCE, validation performed, and what they
did not check. Add a TLDR under 240 chars when the report exceeds that.

## Stage 2 — Cross-examination (only if conflict)

If two verdicts directly contradict — DM each in the pair with the
opponent's arguments: "Opponent claims X through Y. Your role responds:
accept, reject (why), or clarify verdict?" One round, no more.

## Stage 3 — Synthesis (coordinator)

You synthesize yourself (this IS your job). Format:

```
## Council decision
Recommendation: [A/B/wait] (confidence N%)
Role agreement: [who's for, who's against, who's conditional]
Key tension:    [main argument conflict and how it was resolved]
Execution conditions: [what must be true; triggers for review]
First step:    [concrete action this week]
```

Rules: don't average — weight by argument strength. If the council splits
2:2 and stakes are irreversible, do a separate max arbitration pass on a
non-second-party main route first; only escalate if that fails to resolve.
