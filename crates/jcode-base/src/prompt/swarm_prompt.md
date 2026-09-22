<!--
This file IS the swarm config. Swarms are complicated, dynamic systems, so
routing policy is passed to the models as a prompt rather than as options in
a standard config file. Edit freely: override globally at
~/.jcode/swarm-prompt.md or per-project at ./.jcode/swarm-prompt.md.
-->

Model routing guidance for spawned swarm agents. Pass `model` to choose a model
for newly spawned workers, including workers created by assignment or `run_plan`.
An explicit model overrides `agents.swarm_model`. When omitted, workers use that
configured default, or inherit the coordinator's model and route when unset.
Pass `model: "inherit"` to force coordinator inheritance even with a configured
default. Model selection does not change reused workers. Run `swarm list_models`
to check available models/routes. Route-prefixed values such as
`openai-api:gpt-6-astra` pin the authentication route as well as the model.
Pass `effort` when spawning or assigning swarm work:

- Implementation tasks: `effort: "low"`.
- Design, investigation, debugging, review, and verification: default effort.
- Context fetching / bulk reading / summarization: `effort: "none"`.
- Use `[agents] swarm_model` to set the default for future worker spawns, and
  the `model` parameter for task-specific choices.

- Never route Fable through swarm, normal model selection, default routing, or
  fallback. Within a swarm, `claude-fable-5` is available only through an
  explicit user-invoked `/fable` skill, which sends one bounded
  pre-gathered-context request through guarded Anthropic OAuth with no tools.
  Never pass `claude-fable-5` to `swarm`. (An operator may separately select
  Fable as a session's own main model by naming it exactly in
  `anthropic_subscription_guard_allowed_models`; that is a deliberate operator
  choice and does not make Fable a routing target for you.)
- Subscription-backed Claude routes use bare model IDs through Jcode's shared
  monitor. Parallel workers are allowed and record credential-free session,
  model, timing, and five-hour utilization telemetry. Do not bypass this with
  Claude CLI, uclaude/ucf, a side process, wrapper, or API-key alias.
- If monitoring reports suspicious movement or approval is pending, do not
  start another Claude call. Ask the operator, then use `jcode claude approve`
  only after approval. `jcode claude off` and `jcode claude on` control new
  calls globally. Continue safe MiniMax work while waiting.
- Meter failures and suspicious movement are warnings and approval pauses, not
  reasons to remove or rotate credentials.
- Never add `openai-api:` or `claude-api:` unless the operator explicitly
  accepts metered API billing.
- Keep verification independent from implementation and prefer a different
  provider or model family for the acceptance pass.

Do not split premium work into many calls. Prefer one bounded, evidence-rich
assignment with a clear definition of done. Always pass a short `label`. Keep
one writer for overlapping files. Workers must return STATE, EVIDENCE,
validation performed, and residual risk.

In normal and light-swarm mode, only the root session may spawn agents. Workers
complete their assignment directly and report back. Recursive spawning is
reserved for a root running in `swarm-deep` mode, where deeper decomposition is
allowed only when it materially improves coverage.
