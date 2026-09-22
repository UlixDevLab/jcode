---
name: component-stress
description: >-
  Use before reusable-component signoff. Render relevant worst-case states and
  report observed failures.
metadata:
  visibility: exported
  provenance:
    adapted_from: https://github.com/jakubkrehel/skills/tree/ca483852de23d48ab4f4ea71da37dad12bd70a95/skills/break
    license: MIT
---

# Component Stress

Answer "does this component survive real use?" with a rendered state gallery.
This skill observes the real component. It does not review a lookalike or infer
failures from code alone.

## Automatic signoff trigger

Load this skill before signoff when a reusable component accepts variable
content, data quantity, interaction state, viewport width, theme, direction, or
motion preference. Skip it for fixed decorative elements whose public state
cannot materially vary.

Use one real component per run. A form field, result card, modal, navigation
item, metric tile, or pricing card is a component. A whole page is not.

## Derive relevant worst-case scenarios

Read the component API, call sites, fixtures, and production data shape. Keep
only scenario axes the component can actually receive:

- empty, loading, error, disabled, selected, open, and focused states
- very short, long, unbreakable, localized, or missing content
- zero, one, typical, and maximum realistic item counts
- narrow and wide containers
- light, dark, high-contrast, and project-specific themes
- keyboard, pointer, touch, and reduced motion where applicable

Write the selected scenarios and the intentionally omitted axes before building
the gallery. Do not run every generic state against every component.

## Render the real component

Create a temporary route, story, or harness inside the project's normal runtime.
Import the real component and project styles. Render one labeled instance per
scenario with deterministic fixtures. The harness may set container width and
props, but it must not restyle, retheme, or rebuild the component.

Production code must never import the harness. Do not connect the gallery to
live private data or mutable production state.

## Observe before judging

Load the gallery through the real browser or project preview path. Check the
required viewports and interactions once the whole scenario set renders.
Report only observed behavior, such as:

- text escaped its container
- focus was invisible
- the empty state left an unlabeled blank region
- the loading skeleton changed layout width
- reduced motion still moved the scene

A predicted problem is not an observed finding. If the page did not render,
report the harness failure instead of reviewing the source as a substitute.

## Output and repair loop

Return broken scenarios first:

| Scenario | Observed result | Owning rule or skill |
| --- | --- | --- |

`Everything survived` is a valid result when every selected scenario rendered
and was checked. When repair is requested, use the owning project rule,
`/ui-rules`, `/motion-review`, or `/frontend-design`, then rerun only the failed
and adjacent scenarios.

Keep the gallery available for user review until signoff. Remove the temporary
route and fixtures after acceptance unless the project intentionally promotes
it into Storybook or a permanent visual test surface.

## Evidence

Report the component path, harness route, selected and omitted axes, fixture
source, viewports, interactions, and each observed result.

## Provenance

Adapted from Jakub Krehel's `break` skill at commit
`ca483852de23d48ab4f4ea71da37dad12bd70a95` under the MIT License.
Copyright (c) 2026 Jakub Krehel.
