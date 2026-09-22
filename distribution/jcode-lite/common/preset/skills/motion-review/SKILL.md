---
name: motion-review
description: >-
  Use when motion is central, changed, slow, or wrong. Review the real
  animation, access, and runtime.
metadata:
  visibility: exported
  provenance:
    adapted_from: https://github.com/emilkowalski/skills/tree/d23d7f88a2e21c9e4b1418c7abe420f5c1052ba7/skills/review-animations
    license: MIT
---

# Motion Review

Review motion as product behavior, not decoration. This skill is narrow. It does
not redesign unrelated layout, copy, or visual identity.

## Automatic trigger

Load this skill when motion is central to the brief, when an animation changes,
when a user says it feels wrong, or before signoff for a motion-heavy surface.
For ordinary static UI, do not load it merely because hover transitions exist.

Read the approved mock, storyboard, `.jcode/design/style-lock.md`, interaction
frequency, input model, target devices, and reduced-motion requirement before
reviewing.

## Review contract

Every motion decision must pass these checks:

1. **Purpose.** It explains spatial change, state, feedback, causality, or story.
   Remove motion whose only answer is "it looks impressive" on a frequent task.
2. **Frequency.** High-frequency and keyboard-driven actions should be instant or
   extremely restrained. Rare narrative moments may carry more choreography.
3. **Timing.** Interaction feedback starts immediately. Ordinary UI transitions
   are brief. Longer narrative sequences need an explicit story reason.
4. **Easing.** Enter and exit motion responds quickly rather than delaying the
   moment the user is watching. Avoid `ease-in` for ordinary interface entry.
5. **Physical origin.** Popovers, menus, panels, and assembled objects move from
   a believable source, target, or hinge.
6. **Interruptible behavior.** Repeated, scroll-driven, or gesture-driven motion
   must retarget from its current state rather than restart or fight input.
7. **Performance.** Prefer transform and opacity. Treat layout animation,
   excessive material work, large repaint regions, and per-frame allocation as
   findings unless measured safe.
8. **Reduced motion.** Preserve information while removing or simplifying
   movement. A static final state is often better than hiding the content.
9. **Input fit.** Hover motion requires a hover-capable pointer. Touch and
   keyboard paths receive equivalent feedback without pointer assumptions.
10. **Cohesion.** Motion matches the product's personality and the surrounding
    interaction language.

## Real observation

Review the animation in the real route at the required viewport and device
class. Exercise its actual trigger, interruption path, reverse path, resize,
visibility pause, reduced motion, and fallback. For scroll-linked or 3D work,
also inspect low-FPS behavior, context loss or asset failure, and the final
resting composition.

Use screenshots only for key states. Use a short recording or deterministic
progress controls when timing and continuity are the question. Source inspection
is supporting evidence, not a substitute for watching the motion.

## Findings

Return a prioritized table:

| Severity | Moment | Observed behavior | Required change | Why |
| --- | --- | --- | --- | --- |

Prefer fixes in this order:

1. delete unjustified motion
2. reduce distance, duration, or frequency
3. correct easing and physical origin
4. make the sequence interruptible
5. move work to GPU-safe properties or lower-cost rendering
6. add reduced-motion and input-specific behavior
7. polish secondary details only after the main story reads clearly

When the parent task includes implementation, rerun the exact failing moment
after each material change. Stop iterating when the approved story reads at the
target viewports and the measured runtime stays within the project's budget.

## Evidence

Report the route, trigger, viewport, reduced-motion result, interruption result,
performance observation or measurement, and the remaining unchecked devices.

## Provenance

Adapted from Emil Kowalski's `review-animations` skill at commit
`d23d7f88a2e21c9e4b1418c7abe420f5c1052ba7` under the MIT License.
Copyright (c) 2026 Emil Kowalski.
