---
name: design-variants
description: >-
  Use when UI planning has several directions. Build three real-page candidates
  on one axis.
metadata:
  visibility: exported
  provenance:
    adapted_from: https://github.com/jakubkrehel/skills/tree/ca483852de23d48ab4f4ea71da37dad12bd70a95/skills/variant
    license: MIT
---

# Design Variants

Answer "which direction?" before production code makes the choice expensive.
This skill creates candidates. It does not quietly choose the winner.

## Automatic planning trigger

Load this skill when a new or redesigned UI has multiple credible directions
that would change structure, density, emphasis, typography, or interaction.
Skip it when an approved mock, project style lock, or explicit user direction
already resolves the choice.

Use one surface per run. A hero, settings header, command result card, or pricing
section is a surface. A whole application is not.

## Ground every candidate

Read the real page, neighboring components, design tokens, approved references,
and `.jcode/design/style-lock.md` when it exists. Use realistic content and the
actual target viewports. Project rules and approved references outrank this
skill.

Every candidate must clear the same floor:

- semantic structure and readable content
- keyboard access and visible focus
- sufficient contrast and target sizes
- no clipping at the required narrow viewport
- reduced-motion support where motion exists
- the project's component, token, and icon vocabulary

A visually exciting candidate that breaks the floor is not a candidate.

## Vary one primary axis

Choose one primary axis before building:

- structure
- density
- emphasis
- typography
- interaction or motion model

Create three named positions on that one primary axis. Examples are `Quiet`,
`Editorial`, and `Dense`, not `A`, `B`, and `C`. Secondary choices may follow
the primary axis for coherence, but do not vary everything at once. If two
variants differ only by color or copy, replace one.

## Build in the real page

Render one candidate at a time in the real page with its real chrome,
neighbors, and realistic data. Select it through a shareable URL parameter such
as `?variant=quiet` or the nearest project-native equivalent. A small neutral
picker may switch variants without becoming part of the design under review.

When a real route cannot safely host the work, use a self-contained harness
that imports the production tokens and components. Production must never import
the temporary harness.

Before presenting:

1. load every candidate at each required viewport
2. exercise its primary interaction
3. confirm the console is clean
4. record the accessibility floor checks
5. summarize the distinct tradeoff each candidate makes

## Hand the decision to the user

Present a neutral table:

| Variant | Axis position | Best when | Cost |
| --- | --- | --- | --- |

Do not mark a favorite unless the user asks. If asked, ground the recommendation
in product frequency, audience, and the approved design language.

## Promote and clean up

After the user chooses, promote the winner into the approved mock or production
plan. Record the choice in the normal approval artifact and, when the direction
is reusable, in `.jcode/design/style-lock.md`. Delete the losing candidates,
picker, flags, and temporary fixtures after promotion. Until then, keep the
harness isolated and reviewable.

## Evidence

Report the route, variant parameter, viewports, realistic fixture used, checks
run, and the user's chosen direction. Screenshots are evidence of rendering, not
an approval by themselves.

## Provenance

Adapted from Jakub Krehel's `variant` skill at commit
`ca483852de23d48ab4f4ea71da37dad12bd70a95` under the MIT License.
Copyright (c) 2026 Jakub Krehel.
