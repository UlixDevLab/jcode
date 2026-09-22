---
name: howtos
description: >-
  Curated, project-agnostic techniques worth reusing: how to do a
  specific kind of engineering task well, drawn from what actually
  worked. Load when a task matches an entry, not for
metadata:
  visibility: exported
---

# How-tos

A growing library of techniques that earned their place by working. Each entry
is a technique, not a rule: it says what to do, why it beats the obvious
alternative, and how you know it worked.

Add an entry when a technique proves itself twice, or once with strong
evidence. Delete an entry when it stops being true. A how-to that nobody can
point to a success for is clutter.

## Structure

Each entry lives in `entries/<slug>.md` and answers four things:

1. **When** it applies, concretely enough to recognize the situation.
2. **What** to do, in enough detail to follow without rediscovering it.
3. **Why** this beats the obvious alternative. This is the part that makes it
   worth keeping.
4. **How you know** it worked: the observable check.

Keep an entry under 80 lines. If it needs more, it is probably a role or a
project document rather than a technique.

## Reading

Read the index below, then open only the entries a task actually triggers.
Loading everything defeats the point: the value is in having the right
technique available, not in carrying all of them in context.

## Index

Entries are added over time. The index is the catalog; the bodies are lazy.

- `entries/mock-to-implementation-parity.md` - compare an approved mock against
  the implemented UI without a single global percentage lying to you.

Entries are added only when a technique has proven itself. An index padded with
plausible-sounding entries nobody validated is worse than a short one.

## Adding an entry

Prefer capturing a technique at the moment it proves itself, while the evidence
is still concrete. Record:

- the situation that triggered it
- what was tried first, if something failed before this worked
- the specific commands, parameters, or sequence
- the observable result that confirmed it

Cite real evidence: a file, a command output, a session, a measurement. A
how-to whose justification is "this is best practice" has no place here, because
the model already knows generic best practice and does not need context spent
on it. The library exists for the things that are true *here* and are not
obvious.
