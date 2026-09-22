---
name: research
description: >-
  Lite research orchestrator. Frames the question, runs 2-4 bounded
  scouts, dedupes sources, gathers Context7 / OmniSearch / Reddit
  evidence, captures citations, names contradictions and unknowns, and
  stops on budget.
---

# Research

A concise research orchestrator. It does not replace deep work — it
frames the question, fans out to bounded scouts, dedupes, names what is
contradicted and what is unknown, and stops on budget.

Lite version: provider-neutral and route-aware. When MCP tools
(Context7, web search, Reddit search) are available, use them. When
MiniMax routes are healthy, prefer them for scout work. Never invent
sources or pay for routes the user has not approved.

## Stages

**1. Frame the question.** State in 2-3 lines:

- the actual question to answer (not the topic)
- what would count as a decisive answer
- what would count as a contradiction
- the budget: max wall time, max scouts, max sources cited
- what is out of scope

If the question cannot be made falsifiable, ask the user to sharpen it.

**2. Decompose into 2-4 bounded scout tasks.** Avoid overlap. Each
scout gets:

```
SCOUT N / N
- question:           [one sentence, falsifiable]
- sources to check:   [Context7 docs, OmniSearch web, Reddit threads,
                       repo grep, file paths — whatever applies]
- deliverable:        [one-paragraph answer + cited evidence]
- budget:             [time, max sources]
- stop condition:     [when to return early]
```

**3. Run scouts (sequential or parallel, budget-permitting).** Each
scout returns a single report with:

- answer (PASS / FAIL / INCONCLUSIVE)
- evidence quoted with source link or path
- confidence (low / medium / high)
- what was checked and what was not

**4. Dedupe and rank.** Build a single source set, dropping duplicates.
Rank evidence by recency, authority, and independence. Note when
sources converge and when they disagree.

**5. Synthesize.** Produce the answer in this shape:

```
## Question
[restated as decided]

## Decisive answer
[1-3 sentences]

## Evidence
- [source] — [what it says] — [confidence]
- [source] — [what it says] — [confidence]

## Contradictions
[where credible sources disagree and what would resolve it]

## Unknowns
[what we could not check and why]

## Budget consumed
- scouts: N / max
- sources cited: N
- wall time: Xm
- stop reason: [budget / answered / convergence reached]
```

## Rules

- Quote with a link or path. Never paraphrase a number or a quote
  without the source attached.
- If a scout cannot check something, it must say so explicitly.
  Silence is not evidence.
- Don't stack speculative sources to manufacture confidence. A thin,
  honest answer beats a padded, dishonest one.
- If sources contradict, name the contradiction. Do not silently pick a
  side.
- Don't run past the declared budget. If the answer isn't there, say so
  and propose the next bounded step.
- Cite URLs, file paths, or PR numbers — never "according to recent
  reports."
