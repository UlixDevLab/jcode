# Comparing a mock to an implementation

## When

You have an approved mock page and an implementation of the same screen, and
you need to decide whether the implementation matches. Not "do the tests pass",
but "does it look right".

## What

Capture both sides under one contract, then compare with
`~/.jcode/scripts/visual-parity-compare.mjs`.

The capture contract is the part people skip, and it is the part that decides
whether the numbers mean anything:

- same root locator on both sides, not a mock bezel against a whole app page
- one CSS viewport per pair, `deviceScaleFactor: 1`, `scale: 'css'`
- `await document.fonts.ready`, animations disabled, caret hidden
- deterministic fixtures: frozen time, seeded data, intercepted APIs
- assert the expected state before screenshotting, and let that assertion fail
  rather than capturing the wrong state

Then:

```bash
node ~/.jcode/scripts/visual-parity-compare.mjs \
  --reference mock.png --actual app.png \
  --out diff.png --json report.json
```

Exit codes: 0 PASS, 2 REVIEW, 1 BLOCK, 3 broken capture contract.

The per-pixel threshold is `0.015`, not pixelmatch's `0.15` default. At `0.15`
the effective `maxDelta` is 792, so a light grey card on white (delta 619) and a
subtle border (delta 202) both scored as identical: bold drift was caught while
low-contrast surfaces drifted invisibly.

## The other half: is it built from the design system?

Pixel parity cannot answer this, and passing it says nothing about the answer. A
page can match its mock exactly while hardcoding every colour it uses. It looks
correct today and breaks the day a token changes.

```bash
python3 ~/.jcode/scripts/design-lint.py page.html --tokens tokens.css
```

Exit 0 clean, 1 error-severity violations, 2 the lint could not run.

Point it at the **page**, not the stylesheet. It resolves what the page actually
loads: `<link>`, `@import` chains, inline `<style>`, and `style=""` attributes.
Every one of those was a hole at some point, and each hole produced a *clean
report on drifted code* rather than an error. Both `no-styles-found` and
`broken-stylesheet-ref` are therefore hard errors: "no rules ran" must never be
reported as "no violations found".

`--max-hex 0` is not aspirational. All six approved ULIX mocks measure exactly 0
hardcoded colours; real shipped stylesheets in the same repo measure 227 and 3.
Note that two of those compliant mocks contain 43 and 51 raw hex values by grep,
all of them `:root{--accent:#1877F2}` token definitions. A definition is not
drift, and a linter that fails the source of truth gets disabled within a day.

Both scripts live in a full `~/.jcode` checkout. The Lite distribution ships
this entry but not `~/.jcode/scripts/`, so on Lite treat the two commands as
descriptions of the method rather than paths you can run.

## Why this beats the obvious alternative

The obvious alternative is a single percentage: resize both images, compute RGB
RMSE or mean absolute difference, and threshold it. Three things go wrong.

**A single global number hides a local disaster.** A page can be 98% identical
and still have one completely wrong modal. Gating on the density of the worst
64x64 window alongside the total catches that; a page average never will.

**Raw pixel distance punishes harmless text rendering.** Font hinting and
subpixel placement differ between runs and machines. Comparing at 50% scale and
excluding detected anti-aliased pixels removes most of that, while layout,
spacing, density, and color survive the downscale intact.

**Resizing to force a comparison hides the real bug.** In the Tenderium
fidelity set, two mobile pairs were 780x1688 against 390x844: a
deviceScaleFactor mismatch in the capture, not a visual difference. A tool that
resizes silently reports a plausible number for a meaningless comparison. Treat
a dimension mismatch as a hard error.

Also worth knowing: a failing gate is not automatically a wrong threshold. That
same fidelity set scored 0/12 under the old gate, and the failures were real
divergence in card geometry, mobile composition, and content density. Loosening
the threshold would have hidden twelve genuine problems.

## How you know it worked

- Identical input gives exactly `0.000%` and exits 0.
- The diff PNG shows red where content genuinely differs and yellow where
  anti-aliasing was forgiven, so a human can check the tool's judgment.
- The ranking is stable against an independent method. On the Tenderium set
  this ordering matched a separately recorded RMSE/SSIM ranking: `tenders`
  worst at 17.1% total and 99.2% worst-window, `register` best and the only
  non-BLOCK at 0.97%.
- On a synthetic one-pixel anti-aliasing fringe, the default reports 0 diff
  pixels and `--include-aa` reports 396, which proves the AA path is actually
  doing work rather than being decorative.
