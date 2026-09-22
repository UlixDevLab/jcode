---
name: landing-page
description: >-
  Use for landing pages. Align one offer, one audience, and one action with
  evidence and real acceptance.
metadata:
  visibility: exported
  provenance:
    adapted_from: https://github.com/elayadesign/ai-design-skills/tree/1c1e97cb9878e236552c772092dda7adcdddbcb2/skills/landing-page-design
    license: MIT
---

# Landing Page

A landing page wins one intent: **one offer, one audience, one primary action**.
It is not a generic homepage and it must not import marketing patterns into a
productivity interface.

## Automatic trigger

Load this skill when the affected surface is a campaign page, product landing
page, portfolio landing page, launch page, waitlist, comparison page, or other
marketing experience. Use `/frontend-design` as the parent delivery workflow.

If the page has multiple credible visual directions, load `/design-variants`
during planning. If motion carries the story, load `/motion-review`. Before
signoff for reusable cards, forms, or navigation, load `/component-stress`.
When scroll itself becomes the narrative timeline rather than ordinary page
movement, load `/scroll-craft` for grammar, score, and timeline verification.

## Establish the conversion brief

Recover these facts from the repository, product material, analytics, and user
request before inventing copy:

- the exact offer and what the visitor receives
- the intended audience and their current context
- the one primary action and what counts as conversion
- traffic source and what the visitor already knows
- the strongest objections and risk
- proof that can honestly support the claims
- required assets, language, SEO, performance, and mobile constraints

Ask only for missing facts that would materially change the page. Otherwise
state a bounded assumption and continue. Never invent customer logos, metrics,
testimonials, certifications, or outcomes.

## Build the argument

Choose the smallest structure that supports the offer:

1. **Hero:** specific outcome, audience, primary action, and one proof signal.
2. **Problem and mechanism:** what changes and how the product produces it.
3. **Benefits:** outcomes with concrete evidence, not a feature inventory.
4. **How it works:** only when the mechanism or adoption path needs explanation.
5. **Proof:** place evidence beside the claim it supports.
6. **Objection handling:** comparison, FAQ, security, process, or risk reversal.
7. **Final action:** repeat the same primary action without introducing a rival.

High-intent pages may be much shorter. Complex or unfamiliar offers may need a
longer narrative. Do not add sections to satisfy a template.

## Copy contract

- Prefer concrete outcomes and named audiences over words such as streamline,
  optimize, revolutionary, or powerful.
- CTA text states the action and result, not `Learn more` or `Submit`.
- Keep one primary CTA hierarchy above the fold.
- Separate proven facts from aspirations.
- Match the language and promise to the traffic source.
- Treat objections as part of the argument, not a footer afterthought.

## Visual and motion direction

Use repository tokens and the approved project style. If no direction exists,
create variants rather than choosing a generic gradient-card template. The hero
visual must explain the product or transformation, not merely decorate empty
space.

Motion may reveal causality or product transformation. It must not delay access
to the offer, obscure copy, hijack scrolling, or run without a useful reduced-
motion state. A motion-heavy hero needs a stable final composition and a static
or low-cost fallback.

## Build and verify section by section

Implement hero, argument, proof, objections, and final action in reviewable
slices. Check each slice at the required mobile and desktop widths before adding
the next. Preserve semantic headings, keyboard flow, contrast, readable zoom,
image dimensions, and performance budgets.

Acceptance covers:

- the primary action works through the real public path
- copy and proof match approved source material
- mobile hierarchy and content order remain clear
- forms expose labels, errors, loading, success, and retry states
- metadata, canonical behavior, indexing choice, and structured data are correct
  when SEO applies
- motion, reduced motion, asset failure, and fallback are observed
- console, network, and performance checks are clean enough for the target

## Evidence

Report the conversion brief, assumptions, selected structure, proof sources,
primary action path, tested viewports, component stress results, motion results,
and unresolved claims or assets.

## Provenance

Adapted from Elaya's `landing-page-design` skill at commit
`1c1e97cb9878e236552c772092dda7adcdddbcb2` under the MIT License.
Copyright (c) 2026 Elaya.
