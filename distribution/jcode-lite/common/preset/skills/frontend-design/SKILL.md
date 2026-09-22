---
name: frontend-design
description: >-
  ALWAYS use for new or redesigned web UI. Inspect the repo and deliver an
  approved mock.
metadata:
  visibility: exported
---

# Frontend Design

Create distinctive, production-grade interfaces by making the existing product
language more intentional. Distinctive does not mean louder, stranger, or less
usable. It means the hierarchy, interaction, and surface physics feel authored
for this product.

## Load the engineering contract first

Before proposing or changing frontend code, read `~/.jcode/roles/frontend.md`
first. Then read the nearest project instructions, relevant schema/task maps,
approved design-language documents, and the affected implementation surface.
Follow the role's classification, mock approval, atomic order, tests-first,
verification, and deploy gates.

Repository rules and approved design language outrank aesthetic recipes. Do not
replace an established product direction because a different style is more
fashionable or dramatic.

## Establish the actual design system

Identify and record:

- the framework, platform, viewport, and input model
- the current component layers and reusable primitives
- the single token source and every active theme
- the approved mock, storyboard, or visual comparison state
- `.jcode/design/style-lock.md` when present
- accessibility and reduced-motion requirements
- real lint, test, build, browser, and screenshot commands

Reuse the existing component vocabulary before adding primitives. Extend the
single token source when a missing semantic value is proven. Never create a
parallel token namespace for color, elevation, motion, spacing, or typography.

## Select narrow workflows during planning

Choose automatically from the session facts. Load only skills that materially
change the plan:

- Load `/design-variants` when multiple credible visual directions remain and
  no approved reference or project style lock resolves them.
- Load `/component-stress` before signoff for a reusable component whose content,
  state, width, theme, or motion can vary.
- Load `/motion-review` when motion is prominent, changed, performance-sensitive,
  or reported as feeling wrong.
- Load `/landing-page` for a marketing, campaign, portfolio, launch, waitlist, or
  comparison page.
- Load `/scroll-craft` when scrolling itself is the primary narrative input,
  such as scrollytelling, scrubbed media, a continuous world, or pinned acts.
  Do not load it for ordinary landing-page entrance animation.

Do not ask the user to select skills. The session decides from the task and
records the selection in the UI brief. Do not load all narrow skills by default.
An explicit user invocation always wins.

## Reference-study mode

When the user supplies a screenshot, URL, recording, or existing page as a
reference, enter `reference-study` during planning:

1. capture or ingest the full relevant surface, not only a hero crop
2. extract observable structure, spacing rhythm, typography roles, palette,
   component archetypes, motion beats, and responsive behavior
3. separate transferable design logic from branding, copyrighted assets, copy,
   and product-specific pixels
4. record evidence and uncertainty instead of summarizing a vague "vibe"
5. convert the transferable findings into variants, a mock, or a style-lock
   proposal for approval

A reference is evidence, not permission to clone it.

## Project style memory

Use `.jcode/design/style-lock.md` as project-scoped design memory. Create or
change it only after explicit approval of a mock or reusable design direction.
It records:

- approved references and approval artifact paths
- semantic color, type, spacing, radius, surface, icon, and motion decisions
- density, accessibility, responsive, and reduced-motion rules
- deliberate anti-patterns and known exceptions

The style lock never imports generated values from the implementation it judges.
Repository tokens, approved references, and accessibility requirements outrank
it. Do not create a global personal taste profile in this workflow.

## Choose the correct visual context

Distinguish a productivity application from a marketing or editorial surface.
A productivity application prioritizes stable action placement, information
density, touch and keyboard clarity, and calm repeated use. A marketing or
editorial surface may justify more asymmetry, overlap, reveal choreography, and
expressive typography. Do not transfer those patterns between contexts by
default.

Accessibility, reduced motion, density, and platform constraints always outrank
visual novelty. Preserve semantic HTML, focus-visible treatment, target sizes,
contrast, keyboard flow, readable zoom, and motion preferences.

## Visual craft review

Use visual craft review when requested, when the approved mock calls for it, or
when the affected surface is visibly under-resolved. Review:

- hierarchy and one clear primary action
- semantic typography and deliberate wrapping
- meaningful color and contrast
- token-bound surface physics
- composition appropriate to the product context
- motion that explains state or story
- coherent default, hover, focus, active, disabled, loading, empty, and error
  states
- approved icons and interaction details

Reject arbitrary style libraries, gradients, custom cursors, texture, or motion
that compete with product meaning.

## Delivery workflow

1. Classify the affected surface using the frontend role's mode.
2. Inventory reusable tokens, components, references, and project style memory.
3. Select the matching narrow skills automatically and record why.
4. Resolve open direction with `/design-variants` when needed.
5. Build or update the token-bound mock in the real target viewport.
6. Get explicit mock approval and record it in the project's approval artifact.
7. Update `.jcode/design/style-lock.md` only when the approval establishes a
   reusable direction.
8. Add a failing test for each changed public flow or state.
9. Implement in the project's component order and keep side effects outside
   render components.
10. Run project checks plus `/component-stress` and `/motion-review` when their
    triggers apply.
11. Compare the real implementation against the approved reference.
12. Update verification, destination/version, and deploy records.

## Visual evidence contract

Use deterministic named states with seeded data, fixed viewports, reduced
motion, and continuous animations disabled during static capture. Screenshot
capture is evidence, not pixel-regression coverage unless assertions run.
Report functional acceptance separately from perceptual review.

The result is complete only when the approved direction, implementation, public
interaction states, project style lock, accessibility behavior, and verification
record agree.
