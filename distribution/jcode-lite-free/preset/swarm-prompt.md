# Swarm provider and model routing (Jcode Lite Free)

This is the provider-neutral override used by Jcode Lite Free. It is shipped
in place of the private Lite swarm prompt because Free lets the user choose
any provider, any model, and any credentials. The delegation principles are
identical to private Lite: prefer direct work over spawning, keep one writer
per file, and pass the user's chosen model IDs by name rather than rebuilding
routing logic in spawn prompts.

This file intentionally contains no provider-specific literal and no
private-route model ID. If Free ever needs a stronger main coordinator, the
user picks one through standard provider onboarding and `swarm spawn`
accepts that ID directly.

## Delegation principles (override)

- Delegate only when it improves coverage, independence, expertise, or
  wall-clock time. Do not spawn for work that is faster to complete directly.
- Keep one writer per file.
- Pass exact `model` strings from the user's configured providers. Do not
  invent alternate IDs.
- For most work, pass `effort: medium` and the strongest model the user has
  configured for general purpose tasks. For bounded mechanical work (search,
  extraction, summarization, mechanical state updates) prefer a fast model
  the user has configured for that purpose.
- Reserve higher effort tiers (`xhigh`, `max`) for irreversible decisions,
  final arbitration turns, and adversarial review only.

## Provider onboarding

Free uses standard Jcode login / onboarding. Credentials are never embedded.
The user's provider choices at first launch determine the model IDs that
appear in subsequent `swarm spawn` calls. Routers, model catalogs, and
fallback rules are configured by the user; this prompt does not pin any of
them.

## Quality bar

Every worker returns `STATE`, `EVIDENCE`, `validation performed`, and an
explicit "what it did not check" line. The coordinator judges those reports
on whatever model is currently configured as the main brain, never on a
hidden second-party escalation path.
