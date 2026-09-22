---
name: idea-forge
description: >-
  Активує воркфлоу перевірки ідей. Idea-forge: новий офер, канал,
  продукт, фіча. Ідея. Послідовність: First-Principles + Wanderer →
  Client + CFO → Journalist → вердикт. Trigger: /idea-forge, /ідея,
  "перевір ідею", "є гіпотеза", idea validation, hypothesis test,
  pre-build check.
---

# Idea Forge

The goal is to kill a weak idea cheaply or strengthen a strong one. You
as coordinator on the active non-second-party route; roles are workers on
different models.

## Stage 1 — Tear down the frame (parallel, 2 workers)

Both get the same idea brief (3-5 lines).

| Role | SKILL.md | Route | Question |
|------|----------|-------|----------|
| First-Principles | first-principles/SKILL.md | main coordinator, high | what assumptions are baked in, what's bedrock, what's inertia |
| Wanderer | wanderer/SKILL.md | implementation pool, medium | naive questions: what's it even for, why not simpler |

After Stage 1 reformulate the idea accounting for the assumptions removed.
If the idea collapsed here — stop honestly and say why.

## Stage 2 — Demand and economics (parallel, 2 workers)

Give them the updated frame from Stage 1.

| Role | SKILL.md | Route | Question |
|------|----------|-------|----------|
| Client | client/SKILL.md | independent non-second-party, medium | buyer voice from inside: what hooked, what stopped, what would they pay for |
| CFO | cfo/SKILL.md | independent non-second-party, high | unit economics, breakeven, payback, smallest money test |

## Stage 3 — Contradictions (Journalist)

implementation pool, medium. Give ALL previous conclusions. Prompt: "Read
journalist/SKILL.md. Find internal contradictions between the frame, the
buyer voice, and the economics. Where beautiful words have no specifics.
What is the most uncomfortable question left unanswered."

## Stage 4 — Verdict (you, coordinator)

```
## Verdict on idea
Status:        [GO / KILL / PIVOT / TEST FIRST]
Strong core:   [what's actually valuable]
Fatal risk:    [what will kill it if unresolved]
Cheapest test: [what to check in a week and how much it costs]
Success bar:   [specific number]
```

For GO on serious money — additionally route through /decision-board.
