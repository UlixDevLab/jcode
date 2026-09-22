---
name: ops-diagnosis
description: >-
  Активує воркфлоу операційних проблем. Ops-diagnosis: конверсія
  впала, клієнти йдуть, процес зламався, команда не тягне.
  Діагноз. Послідовність: Statistician → Anthropologist + Seller →
  System-Architect → синтез. Trigger: /ops-diagnosis, /діагноз,
  "чому впало", operational diagnosis, root cause, funnel break.
---

# Ops Diagnosis

Order is fixed because each stage narrows the next. The coordinator on the
active non-second-party route synthesizes; roles run as workers.

## Stage 1 — Facts (Statistician, first and mandatory)

Spawn a worker on an independent non-second-party route, high. Prompt: "Read
statistician/SKILL.md. Strictly in role. Problem: <description>. Data:
<what's available, or ask user for specific numbers>. Return: WHAT DATA
SAYS (no interpretation), base rate, whether the change is significant,
WHAT DATA IS MISSING."

If the Statistician says "change is within noise" — stop, tell the user,
don't run the next stages. This is the main value of the workflow.

## Stage 2 — Behavior (parallel, 2 workers)

Run only if the effect is real. Both get Stage 1 conclusion.

| Role | SKILL.md | Route | Question |
|------|----------|-------|----------|
| Anthropologist | anthropologist/SKILL.md | independent non-second-party, medium | what people DO vs say, which ritual broke |
| Seller | seller/SKILL.md | implementation pool, medium | what's heard in the field: objections, where dialogue gets stuck |

## Stage 3 — System (System-Architect)

main coordinator, high. Give it Stage 1 + Stage 2. Prompt: "Read
system-architect/SKILL.md. Build the causal chain: symptom ← intermediates
← root cause. Find feedback loops and the LEVER — point of minimum
intervention with maximum effect."

Optional: if Stage 2 shows an external factor — add Competitor in parallel
with Stage 3.

## Stage 4 — Synthesis (you, coordinator)

```
## Diagnosis
Root cause:  [one sentence]
Chain:       [symptom ← ... ← cause]
Lever:       [concrete intervention]
Verification:[which indicator and when confirms/refutes]
DO NOT DO:   [tempting but wrong interventions]
```
