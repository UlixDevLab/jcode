# Lab

Run a bounded experiment loop that declares, runs, freezes, gates, and
records. A run that skips a step is an anecdote, not an iteration.

Lite-adapted: stays within available tools. No Sonar, no container
runtime, no external scorer pipeline required. Use the harness the host
already provides.

## The loop

One iteration is: declare, run, freeze, gate, record.

1. **Declare** the hypothesis BEFORE running. Name the candidate set,
   the data by identity, the metric, the baseline, and the promotion
   contract. A hypothesis you can only state after seeing results is
   not a hypothesis.
2. **Run** the declared candidates on the declared data. Don't add a
   candidate mid-run because it looks promising; that belongs to the
   next iteration.
3. **Freeze** the winner before validation. Selection and validation
   must not touch the same data.
4. **Gate** against the promotion contract as written. A near miss is a
   miss.
5. **Record** the outcome whether it confirms, refutes, or is
   inconclusive. A refuted hypothesis is a result and must cost the same
   to record as a successful one.

## Before the first iteration

Confirm four boundaries and state them back before starting:

- **Money.** Whether any run can place a real order. Default to no.
  Backtest and demo only.
- **Budget.** Wall-clock, token, and iteration ceilings for this burst.
- **Stop condition.** What makes the loop stop rather than continue.
  "Out of ideas" is not one.
- **Data.** Which datasets are sealed; what may not be refit on them.

If any is unstated, ask once, then record the answer so the next burst
doesn't ask again.

## Scheduling

Don't build a bespoke daemon. Use the harness:

- Use `schedule` to wake this work at a chosen time, or after a wait,
  when the next step depends on elapsed time.
- Use background tasks for anything that outlives a turn. Give every one
  a timeout. An unbounded wait turns a loop into a stall.
- When a step is genuinely idle-until-a-time, schedule and end the turn.
  Don't spin.

Record on every wake: what ran, what it concluded, and the single next
concrete step. A fresh session must be able to resume from that alone.

## Evidence discipline

- An experiment identity is the content of its declaration. The same
  declaration must produce the same identity, so a repeat is detectable
  rather than silently rerun.
- Outcomes are append-only. Never edit or delete a recorded result to
  make a series look better.
- Report the metric declared, not the one that happened to look best.
- Separate in-sample, out-of-sample, and stress results explicitly.
  Collapsing them is the most common way a lab lies to itself.
- State costs, fees, slippage, and latency assumptions with every
  performance number.
- When a run is inconclusive, say inconclusive. Don't promote it and
  don't bury it.

## Avoiding the classic failure

The dangerous mode is a loop that runs forever and learns nothing: it
re-tests near-duplicates of an idea it already refuted, and each run
looks like progress.

Guard against it explicitly:

- Before declaring, search the ledger for the same hypothesis under a
  different name.
- If two consecutive iterations produce no new information, stop and
  report that. The finding is that the search space is exhausted — that
  is worth more than a third run.
- Prefer an iteration that can refute something over one that can only
  confirm.

## Reporting

At the end of a burst, report:

- iterations run, and what each one tested
- what was promoted, refuted, or left inconclusive, with the deciding numbers
- budget consumed against the ceiling
- the single next concrete step
- the stop reason: budget, stop condition, exhausted search space, or a blocker

A burst that ends without a stop reason is unfinished, not complete.
