---
name: debate
description: >-
  Активує дебатний клуб: різна провідна модель-координатор і два
  опоненти на різних не-second-party маршрутах, трьома раундами, з
  модератором-суддею. Для відкритих спірних питань без фіксованих
  ролей. Trigger: /debate, /дебати, "нехай
---

# Debate

The coordinator (the active main decision route) is moderator and judge.
Never play a debater yourself.

## Setup (coordinator)

1. State the thesis as a binary or clearly oppositional question.
2. Choose tier:

**STANDARD tier** — typical contested questions (default):
   - Debater A: implementation pool, medium
   - Debater B: implementation pool, medium (separate session for independence)
   - Debater C: independent non-second-party, medium
   - Judge: coordinator; non-second-party routes only for panel independence.

**SERIOUS tier** — significant architecture / product / personal decisions
(irreversible, costly, strategic):
   - Debater A: main coordinator, high
   - Debater B: independent non-second-party, high
   - Debater C: implementation pool, high
   - Judge: coordinator on main coordinator, max.

If a model is unavailable, fall back to a different non-second-party route
and explicitly note reduced provider diversity.

3. Generate a persona for each: name, perspective (one lens, not "for/against"),
style from presets: aggressive, curious, diplomatic, analytical, provocateur,
balanced. Personas should cover DIFFERENT lenses of the question
(practitioner-executor, long-horizon architect, cost-skeptic).

## Round 1 — Opening (parallel)

Spawn three concurrently. Prompt each:
"You are <persona, style>. Topic: <thesis>. Give opening: position,
3 strongest arguments, 1 prediction of what happens if we do the opposite.
Under 250 words. Research the repo or web if needed."

## Round 2 — Rebuttal (parallel)

DM each with full texts of BOTH opponents:
"Opponents said: <A>, <B>. Your rebuttal: attack the weakest argument of
each (concrete, quote), defend your strongest, name one point where the
opponent is right (required). Under 200 words."

## Round 3 — Convergence (parallel, final)

"Final round. Considering the whole discussion: what do you now consider
true, where did you change your mind, what conditions would make the
opposite position right. Under 150 words."

Stop: 3 rounds max. If all converge after Round 2 — skip Round 3, go to
synthesis.

## Synthesis (you, the judge)

Format:

```
## Debate: <thesis>
### Participants
[persona (model): position in one line]

### Points of agreement
[what all accepted — most reliable conclusions]

### Points of disagreement
[living conflicts + WHY unresolved: different facts or different values]

### Strongest argument of the debate
[one, with author]

### Judge verdict
Recommendation + confidence (%). Consensus level: full / partial / split.
Review conditions: [what would change the verdict]
```

Judge rules: weight arguments, not votes. Argument with verified fact beats
pretty rhetoric. If split and stakes are irreversible, do a separate max
arbitration pass on a non-second-party main route first; only escalate if that
fails to resolve.

## Saving

Write the transcript to `debates/<slug>-<date>.md` in the current project
(format: participants, rounds, synthesis) so the debate can be read and
cited later.
