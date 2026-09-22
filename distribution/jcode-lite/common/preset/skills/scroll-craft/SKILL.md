---
name: scroll-craft
description: >-
  Use for scrollytelling where scroll is the timeline, with real browser
  verification.
metadata:
  visibility: exported
---

# Scroll Craft

Scroll is the timeline.

Build a page where scrolling advances a designed experience rather than merely
revealing stacked sections. This is a specialized child of `/frontend-design`
and `/landing-page`. It does not replace either workflow, and it is not the
default for ordinary marketing pages with a few entrance animations.

## Automatic trigger

Load this skill when the brief asks for scrollytelling, scroll-scrubbed media, a
continuous 3D world, pinned narrative acts, an interactive product journey, or a
page that should feel like an experience rather than a document.

Do not load it for ordinary product UI, documentation, articles, or landing
pages where scrolling only moves the viewport. Load `/motion-review` as well
when the experience is implemented or changed.

## Establish the evidence first

Read the repository, approved references, brand rules, real assets,
`.jcode/design/style-lock.md`, target devices, performance budget, reduced-motion
requirements, and the public conversion path. Existing project language and an
approved storyboard outrank this skill.

Recover these facts before asking questions:

- the audience, offer, one primary action, and traffic context
- the intended journey in the visitor's words
- the desired energy and emotion by stage
- the one moment the visitor should remember
- whether the world is continuous or uses deliberate chapters and cuts
- the assets already owned and any generation budget
- the strongest platform, accessibility, and bandwidth constraints

Ask only for missing facts that would materially change the experience. Record
bounded assumptions instead of forcing an interview when the repository already
answers them.

## Choose one page grammar

Select one organizing grammar before act planning. Do not blend incompatible
grammars merely to collect effects:

1. **Filmic one-shot:** one linear argument with continuous handoffs.
2. **Chaptered editorial:** hard chapter boundaries and dense reading.
3. **Live surface:** the real product surface changes as the argument.
4. **Continuous world:** one fixed spatial world with no section seams.
5. **Typographic poster:** type carries the imagery and rhythm.
6. **Gallery or catalog:** the visitor walks a collection of real objects.
7. **Split stage:** two persistent regions evolve in dialogue.
8. **Rhythmic cutlist:** short, deliberate scenes with authored hard cuts.

Document why the chosen grammar fits and which constraints rule out the closest
alternatives. The grammar determines navigation, hero behavior, act structure,
and the close.

## Write the feeling curve before the scroll score

For every act, write one line containing the intended feeling and the visible
cause. Two adjacent acts with the same feeling indicate filler or an unresolved
transition.

Choose one engineered peak. It receives the clearest story beat, the largest
useful scroll span, the strongest asset, and enough calm before it to register.
A page with several equal peaks has no peak.

Then write a scroll score:

| Act | Visitor shift | Device | Visible state | Scroll span | Reduced state |
| --- | --- | --- | --- | --- | --- |

Every act must change what the visitor understands or feels. Every unit of
scroll must advance a visible state or be labeled as an intentional hold.

## Invent one signature move

Create one interaction that belongs to this page and explains its subject. A
recolored spotlight, generic parallax, or another parameter change is not a
signature move.

Before building, compare the plan with
`.jcode/design/scroll-fingerprints.md` when it exists. Record grammar,
navigation, hero device, act shape, close, and signature move. Change the plan
when it repeats the project history too closely. Add the new fingerprint only
after the experience is accepted.

## Build on the project floor

Use semantic HTML, the project token source, real copy, real assets, real focus
order, and one stable public conversion path. The page remains usable when
scripts, motion, generated assets, or high-bandwidth media fail.

Keep the scroll mechanism separate from page-specific composition. A reusable
engine may publish normalized progress and device events, but it must not build
the page from a universal configuration object. Bespoke behavior stays in the
page or its project component and follows the project's architecture.

For continuous work prefer transform, opacity, and measured media seeking. Avoid
layout mutation per frame, uncontrolled allocation, `transition: all`, and
motion that restarts instead of retargeting from its current state.

Reduced motion preserves the argument, reading order, final states, and access
to content. It removes or simplifies spatial travel rather than deleting the
experience.

## Verify the entire timeline

Serve the real project. File URLs and copied harnesses are not acceptance.
Sample within every act at deterministic progress positions on desktop, mobile,
and reduced motion. Wait for media and canvas state to settle before capture.

Produce a contact sheet and machine-readable findings for:

- **dead scroll:** adjacent samples with no meaningful visible change
- **never-peaking cues:** required copy or controls never become fully readable
- **frozen media:** scroll advances while the media state remains stuck
- **composited contrast:** text measured against the rendered frame underneath
- console errors, failed assets, blocked media, and fallback activation
- focus order, primary action, resize, interruption, reverse scrolling, and the
  final resting composition

Custom canvases and fixed stages must expose a compact rendered-state signature.
Do not publish raw progress as proof when the composition has not changed.

Read the contact sheet after the checks. Automation can show that the timeline
advances, but it cannot decide whether the composition is good, the motion is
smooth, the peak feels memorable, or the acts form a coherent argument.

A headless mobile viewport does not prove real-phone decoding, autoplay,
low-power behavior, or touch physics. Report those as unchecked until a real
device is exercised.

## Acceptance

The result is accepted only when:

- the chosen grammar remains coherent from hero through close
- the intended feeling curve matches a cold observed pass
- one peak is visibly dominant and the close resolves instead of trailing off
- the signature move is specific, useful, and accessible
- every sampled scroll state works at required viewports
- reduced motion and asset failure preserve meaning and conversion
- the public action works through the real route
- remaining device and perceptual uncertainty is reported plainly

Report the grammar, alternatives rejected, feeling curve, peak, signature move,
scroll score, route, contact sheet, findings repaired, public action result,
and unchecked real devices.

## Provenance

Adapted from Nate Herk's Scroll Craft at commit
`e95798551874854cef6dd3996ec7de1364a82bbd` under the MIT License.
Copyright (c) 2026 Nate Herk.
