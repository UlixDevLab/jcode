//! Deterministic guardrails against no-progress tool-call loops.
//!
//! The guard has three independent trip-wires, all deliberately conservative:
//!
//! 1. **Exact-equivalent** — the original behaviour. Fires when the same tool
//!    plus its canonical input plus the completed output all match for
//!    `TERMINAL_REPEAT_COUNT` calls. Resets on any change.
//! 2. **Per-turn budget** — fires when the total number of tool calls in a
//!    turn crosses `TOTAL_BUDGET_TERMINAL`, even if every call is unique.
//!    `TOTAL_BUDGET_WARN` emits a model-visible hint well before the
//!    terminal so the agent can change strategy while still making progress.
//! 3. **Short-cycle** — fires when the last `CYCLE_HISTORY_LIMIT` calls
//!    collapse to a short repeating period (`2..=CYCLE_MAX_PERIOD`) for at
//!    least `CYCLE_WARN_REPEATS` full consecutive cycles. Signatures include
//!    the completed output, so changing outputs reset the cycle — by design,
//!    changing outputs are preserved as progress even when the call
//!    alternation is short.
//!
//! Priority when more than one signal applies in the same call:
//! exact-equivalent stop > cycle stop > budget stop > exact-equivalent warn
//! > cycle warn > budget warn. That ordering picks the most specific message
//! the model can act on first.
//!
//! The breaker is reconstructed at the start of every turn, so long-running
//! workflows, background wait, and resume behaviour are preserved naturally.

use serde_json::Value;

/// First completed exact-equivalent execution that receives a model-visible
/// warning. Was the original loop-breaker behaviour.
pub(crate) const WARNING_REPEAT_COUNT: u32 = 3;
/// Number of exact-equivalent completed executions that trips the terminal
/// guard. Was the original loop-breaker behaviour.
pub(crate) const TERMINAL_REPEAT_COUNT: u32 = 5;

/// Tool calls per turn before the model sees a "many tool calls" hint. Set
/// well above any single-task workflow but below runaway-storm territory:
/// 60 is comfortably above any plausible single-turn workflow while still
/// flagging the kind of 100+ rediscovery loops the guard exists to catch.
pub(crate) const TOTAL_BUDGET_WARN: u32 = 60;
/// Hard per-turn cap. Roughly 3× the warn line — keeps the warning actionable
/// (the model still has room to recover before the stop) and the terminal
/// unmistakable. Doubles the previous ad-hoc 100 so we don't false-positive
/// on long legitimate turns while still bounding the worst case.
pub(crate) const TOTAL_BUDGET_TERMINAL: u32 = 200;
/// Width of the rolling signature history used by the cycle detector. Twice
/// `CYCLE_MAX_PERIOD` plus headroom — enough to detect two consecutive short
/// cycles plus the new call in progress.
pub(crate) const CYCLE_HISTORY_LIMIT: usize = 16;
/// Largest period (in tool calls) that the detector considers. Caps the
/// detector at "short" alternations: longer patterns are noise-tolerant and
/// would otherwise trip on legitimate serial reads.
pub(crate) const CYCLE_MAX_PERIOD: usize = 8;
/// Consecutive identical cycles that emit a model-visible warning. 3 reps
/// means at least 6 tool calls for an A/B storm and 9 for an A/B/C storm —
/// well past the point where an observer would call it stuck.
pub(crate) const CYCLE_WARN_REPEATS: u32 = 3;
/// Consecutive identical cycles that trip the terminal stop. Twice the warn
/// threshold — leaves room for the warning to be acted on first.
pub(crate) const CYCLE_STOP_REPEATS: u32 = 6;

const EQUIVALENT_WARNING_MESSAGE: &str = "[loop guard warning] This tool call and result have repeated without progress. Change the tool input, use a different tool, or explain why the same call is still necessary.";
const EQUIVALENT_TERMINAL_MESSAGE: &str = "[loop guard stopped] This exact tool call has returned the same result five times. Further equivalent execution is blocked. Change the tool input or use a different tool.";
const BUDGET_WARNING_MESSAGE: &str = "[loop guard budget warning] This turn has already executed many tool calls without converging. Switch tools, summarise progress so far, or batch independent reads before continuing.";
const BUDGET_TERMINAL_MESSAGE: &str = "[loop guard budget stopped] This turn has exceeded the per-turn tool-call budget without making progress. Summarise the current state, change strategy, or ask the user before continuing.";
const CYCLE_WARNING_MESSAGE: &str = "[loop guard cycle warning] The same short alternation of tool calls has produced the same outputs repeatedly without progress. Change the call sequence, the inputs, or switch to a new tool.";
const CYCLE_TERMINAL_MESSAGE: &str = "[loop guard cycle stopped] The same short alternation of tool calls has run far past the budget without producing progress. Further equivalent alternations are blocked. Change the call sequence, the inputs, or switch to a new tool.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopGuardAction {
    Allow,
    Warn,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopGuardDecision {
    pub(crate) action: LoopGuardAction,
    pub(crate) message: Option<&'static str>,
}

impl LoopGuardDecision {
    fn allow() -> Self {
        Self {
            action: LoopGuardAction::Allow,
            message: None,
        }
    }

    fn warn() -> Self {
        Self {
            action: LoopGuardAction::Warn,
            message: Some(EQUIVALENT_WARNING_MESSAGE),
        }
    }

    fn stop() -> Self {
        Self {
            action: LoopGuardAction::Stop,
            message: Some(EQUIVALENT_TERMINAL_MESSAGE),
        }
    }

    fn warn_with(message: &'static str) -> Self {
        Self {
            action: LoopGuardAction::Warn,
            message: Some(message),
        }
    }

    fn stop_with(message: &'static str) -> Self {
        Self {
            action: LoopGuardAction::Stop,
            message: Some(message),
        }
    }

    pub(crate) fn annotate(self, mut content: String) -> String {
        if let Some(message) = self.message {
            content.push_str("\n\n");
            content.push_str(message);
        }
        content
    }
}

#[derive(Debug, Default)]
pub(crate) struct ToolLoopBreaker {
    last_canonical: Option<String>,
    last_output: Option<String>,
    equivalent_count: u32,
    total_completed: u32,
    recent_signatures: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CycleSignal {
    None,
    Warn,
    Stop,
}

impl ToolLoopBreaker {
    pub(crate) fn before_call(&self, tool_name: &str, input: &Value) -> LoopGuardDecision {
        let call = canonical_call(tool_name, input);
        let same_call =
            self.last_canonical.as_deref() == Some(call.as_str()) && self.last_output.is_some();

        if same_call && self.equivalent_count >= TERMINAL_REPEAT_COUNT {
            return LoopGuardDecision::stop();
        }
        if matches!(self.detect_cycle(), CycleSignal::Stop) {
            return LoopGuardDecision::stop_with(CYCLE_TERMINAL_MESSAGE);
        }
        if self.total_completed >= TOTAL_BUDGET_TERMINAL {
            return LoopGuardDecision::stop_with(BUDGET_TERMINAL_MESSAGE);
        }
        LoopGuardDecision::allow()
    }

    pub(crate) fn record_result(
        &mut self,
        tool_name: &str,
        input: &Value,
        output: &str,
    ) -> LoopGuardDecision {
        let call = canonical_call(tool_name, input);

        // 1. Equivalent-exact state.
        if self.last_canonical.as_deref() == Some(call.as_str())
            && self.last_output.as_deref() == Some(output)
        {
            self.equivalent_count = self.equivalent_count.saturating_add(1);
        } else {
            self.last_canonical = Some(call.clone());
            self.last_output = Some(output.to_string());
            self.equivalent_count = 1;
        }

        // 2. Per-turn budget counter.
        self.total_completed = self.total_completed.saturating_add(1);

        // 3. Push signature (canonical call + completed output) for cycle detection.
        let signature = format!("{}\x00{}", call, output);
        self.recent_signatures.push(signature);
        while self.recent_signatures.len() > CYCLE_HISTORY_LIMIT {
            self.recent_signatures.remove(0);
        }

        // 4. Combine signals. Priority: equivalent > cycle > budget.
        let cycle = self.detect_cycle();
        if self.equivalent_count >= TERMINAL_REPEAT_COUNT {
            return LoopGuardDecision::stop();
        }
        if matches!(cycle, CycleSignal::Stop) {
            return LoopGuardDecision::stop_with(CYCLE_TERMINAL_MESSAGE);
        }
        if self.total_completed >= TOTAL_BUDGET_TERMINAL {
            return LoopGuardDecision::stop_with(BUDGET_TERMINAL_MESSAGE);
        }
        if self.equivalent_count >= WARNING_REPEAT_COUNT {
            return LoopGuardDecision::warn();
        }
        if matches!(cycle, CycleSignal::Warn) {
            return LoopGuardDecision::warn_with(CYCLE_WARNING_MESSAGE);
        }
        if self.total_completed >= TOTAL_BUDGET_WARN {
            return LoopGuardDecision::warn_with(BUDGET_WARNING_MESSAGE);
        }
        LoopGuardDecision::allow()
    }

    /// Find the smallest repeating period covering the tail of the rolling
    /// signature history. Returns `Warn` after `CYCLE_WARN_REPEATS` consecutive
    /// identical cycles, `Stop` after `CYCLE_STOP_REPEATS`, `None` otherwise.
    ///
    /// Signatures include the completed output (the literal `\x00` separator
    /// prevents call/output collisions), so any change in output resets the
    /// cycle and the model can demonstrate progress by changing responses.
    fn detect_cycle(&self) -> CycleSignal {
        let n = self.recent_signatures.len();
        if n < 4 {
            return CycleSignal::None;
        }
        let max_p = CYCLE_MAX_PERIOD.min(n / 2);
        if max_p < 2 {
            return CycleSignal::None;
        }
        for p in 2..=max_p {
            let pattern: &[String] = &self.recent_signatures[n - p..];
            let mut reps: u32 = 0;
            let mut i = n;
            while i >= p {
                let candidate = &self.recent_signatures[i - p..i];
                if candidate == pattern {
                    reps = reps.saturating_add(1);
                    i -= p;
                    if reps >= CYCLE_STOP_REPEATS {
                        return CycleSignal::Stop;
                    }
                } else {
                    break;
                }
            }
            if reps >= CYCLE_WARN_REPEATS {
                return CycleSignal::Warn;
            }
        }
        CycleSignal::None
    }

    #[cfg(test)]
    fn equivalent_count(&self) -> u32 {
        self.equivalent_count
    }

    #[cfg(test)]
    fn total_completed(&self) -> u32 {
        self.total_completed
    }
}

/// Canonicalize a call so object key order and irrelevant JSON formatting do not
/// affect equivalence. Arrays retain their order because array order is input.
pub(crate) fn canonical_call(tool_name: &str, input: &Value) -> String {
    serde_json::json!({
        "name": tool_name,
        "input": canonical_json(input),
    })
    .to_string()
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| {
                        let value = canonical_json(&object[&key]);
                        (key, value)
                    })
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_repeat_warns_then_stops() {
        let mut breaker = ToolLoopBreaker::default();
        let input = json!({"b": 2, "a": 1});
        let output = "same result";

        assert_eq!(
            breaker.before_call("read", &input).action,
            LoopGuardAction::Allow
        );
        assert_eq!(
            breaker.record_result("read", &input, output).action,
            LoopGuardAction::Allow
        );
        assert_eq!(
            breaker.before_call("read", &json!({"a": 1, "b": 2})).action,
            LoopGuardAction::Allow
        );
        assert_eq!(
            breaker
                .record_result("read", &json!({"a": 1, "b": 2}), output)
                .action,
            LoopGuardAction::Allow
        );
        assert_eq!(
            breaker.record_result("read", &input, output).action,
            LoopGuardAction::Warn
        );
        assert_eq!(
            breaker.record_result("read", &input, output).action,
            LoopGuardAction::Warn
        );
        assert_eq!(
            breaker.record_result("read", &input, output).action,
            LoopGuardAction::Stop
        );
        assert_eq!(breaker.equivalent_count(), TERMINAL_REPEAT_COUNT);
        assert_eq!(
            breaker.before_call("read", &input).action,
            LoopGuardAction::Stop
        );
    }

    #[test]
    fn changed_input_resets() {
        let mut breaker = ToolLoopBreaker::default();
        let input = json!({"path": "a"});
        let _ = breaker.record_result("read", &input, "same");
        let _ = breaker.record_result("read", &json!({"path": "b"}), "same");
        assert_eq!(breaker.equivalent_count(), 1);
        assert_eq!(
            breaker.before_call("read", &input).action,
            LoopGuardAction::Allow
        );
    }

    #[test]
    fn changed_output_resets() {
        let mut breaker = ToolLoopBreaker::default();
        let input = json!({"path": "a"});
        let _ = breaker.record_result("read", &input, "one");
        let decision = breaker.record_result("read", &input, "two");
        assert_eq!(decision.action, LoopGuardAction::Allow);
        assert_eq!(breaker.equivalent_count(), 1);
        assert_eq!(
            breaker.before_call("read", &input).action,
            LoopGuardAction::Allow
        );
    }

    #[test]
    fn different_call_resets() {
        let mut breaker = ToolLoopBreaker::default();
        let input = json!({});
        let _ = breaker.record_result("read", &input, "same");
        let _ = breaker.record_result("write", &input, "same");
        assert_eq!(breaker.equivalent_count(), 1);
    }

    #[test]
    fn changed_output_does_not_receive_a_premature_warning() {
        let mut breaker = ToolLoopBreaker::default();
        let input = json!({"path": "status"});
        let _ = breaker.record_result("read", &input, "pending");
        let _ = breaker.record_result("read", &input, "pending");

        assert_eq!(
            breaker.before_call("read", &input).action,
            LoopGuardAction::Allow
        );
        assert_eq!(
            breaker.record_result("read", &input, "completed").action,
            LoopGuardAction::Allow
        );
    }

    #[test]
    fn warning_and_terminal_messages_are_actionable() {
        let warning = LoopGuardDecision::warn().message.unwrap();
        let terminal = LoopGuardDecision::stop().message.unwrap();
        assert!(warning.contains("Change the tool input"));
        assert!(terminal.contains("Further equivalent execution is blocked"));
    }

    // -------- New RED tests: budget + short-cycle detector --------

    #[test]
    fn changing_input_storm_trips_total_budget_warn() {
        let mut breaker = ToolLoopBreaker::default();
        // Calls 1..=TOTAL_BUDGET_WARN-1 should all be Allow even with unique inputs.
        for i in 0..TOTAL_BUDGET_WARN - 1 {
            let input = json!({"path": format!("file_{}", i)});
            let action = breaker.record_result("read", &input, "ok").action;
            assert_eq!(
                action,
                LoopGuardAction::Allow,
                "call {} should be Allow but was {:?}",
                i + 1,
                action
            );
        }
        // The crossing call emits the budget warning.
        let decision = breaker.record_result("read", &json!({"path": "trip"}), "ok");
        assert_eq!(decision.action, LoopGuardAction::Warn);
        assert!(
            decision.message.unwrap().contains("budget"),
            "warning message should mention budget: {:?}",
            decision.message
        );
        assert_eq!(breaker.total_completed(), TOTAL_BUDGET_WARN);
    }

    #[test]
    fn changing_input_storm_trips_total_budget_terminal() {
        let mut breaker = ToolLoopBreaker::default();
        for i in 0..TOTAL_BUDGET_TERMINAL {
            let input = json!({"path": format!("file_{}", i)});
            let _ = breaker.record_result("read", &input, "ok");
        }
        let decision = breaker.before_call("read", &json!({"path": "next"}));
        assert_eq!(decision.action, LoopGuardAction::Stop);
        assert!(
            decision.message.unwrap().contains("budget"),
            "stop message should mention budget: {:?}",
            decision.message
        );
    }

    #[test]
    fn alternating_cycle_with_stable_outputs_trips_cycle_warn() {
        let mut breaker = ToolLoopBreaker::default();
        let a_in = json!({"tool": "a"});
        let b_in = json!({"tool": "b"});
        let a_out = "a-result";
        let b_out = "b-result";

        // First 4 alternating calls (2 reps of period 2) all Allow.
        for _ in 0..2 {
            assert_eq!(
                breaker.record_result("a", &a_in, a_out).action,
                LoopGuardAction::Allow
            );
            assert_eq!(
                breaker.record_result("b", &b_in, b_out).action,
                LoopGuardAction::Allow
            );
        }
        // Call 5 (third rep's first call) still Allow.
        assert_eq!(
            breaker.record_result("a", &a_in, a_out).action,
            LoopGuardAction::Allow
        );
        // Call 6 (third rep's second call) trips the cycle warn (3 reps).
        let decision = breaker.record_result("b", &b_in, b_out);
        assert_eq!(decision.action, LoopGuardAction::Warn);
        assert!(
            decision.message.unwrap().contains("cycle"),
            "warning message should mention cycle: {:?}",
            decision.message
        );
    }

    #[test]
    fn alternating_cycle_with_stable_outputs_trips_cycle_terminal() {
        let mut breaker = ToolLoopBreaker::default();
        let a_in = json!({"tool": "a"});
        let b_in = json!({"tool": "b"});
        let a_out = "a-result";
        let b_out = "b-result";

        // 12 alternating calls (6 reps of period 2) -> cycle stop.
        for _ in 0..6 {
            let _ = breaker.record_result("a", &a_in, a_out);
            let _ = breaker.record_result("b", &b_in, b_out);
        }
        let decision = breaker.before_call("a", &a_in);
        assert_eq!(decision.action, LoopGuardAction::Stop);
        assert!(
            decision.message.unwrap().contains("cycle"),
            "stop message should mention cycle: {:?}",
            decision.message
        );
    }

    #[test]
    fn alternating_cycle_with_changing_outputs_does_not_trip() {
        let mut breaker = ToolLoopBreaker::default();
        let a_in = json!({"tool": "a"});
        let b_in = json!({"tool": "b"});

        // 12 alternating calls, each output distinct -> changing-output progress.
        for i in 0..12 {
            let tool = if i % 2 == 0 { "a" } else { "b" };
            let input = if i % 2 == 0 { &a_in } else { &b_in };
            let output = format!("output-{}", i);
            let decision = breaker.record_result(tool, input, &output);
            assert_ne!(
                decision.action,
                LoopGuardAction::Warn,
                "iteration {} should not warn when outputs change: {:?}",
                i,
                decision
            );
            assert_ne!(
                decision.action,
                LoopGuardAction::Stop,
                "iteration {} should not stop when outputs change: {:?}",
                i,
                decision
            );
        }
    }

    #[test]
    fn changing_output_resets_active_cycle_detection() {
        let mut breaker = ToolLoopBreaker::default();
        let a_in = json!({"tool": "a"});
        let b_in = json!({"tool": "b"});
        let a_out = "a-result";
        let b_out = "b-result";

        // 6 alternating stable-output calls (3 reps) -> warn already fired.
        for _ in 0..3 {
            let _ = breaker.record_result("a", &a_in, a_out);
            let _ = breaker.record_result("b", &b_in, b_out);
        }
        let warn_call = breaker.record_result("a", &a_in, a_out);
        assert_eq!(warn_call.action, LoopGuardAction::Warn);

        // Breaking the cycle with a distinct output should return to Allow.
        let reset = breaker.record_result("b", &b_in, "different-output");
        assert_eq!(reset.action, LoopGuardAction::Allow);
    }

    #[test]
    fn diverse_tool_workflow_does_not_trip_budget_or_cycle() {
        let mut breaker = ToolLoopBreaker::default();
        // Simulate a legitimate long-running workflow: many distinct calls.
        for i in 0..CYCLE_HISTORY_LIMIT {
            let tool = format!("tool_{}", i % 4);
            let input = json!({"i": i});
            let output = format!("out-{}", i);
            let decision = breaker.record_result(&tool, &input, &output);
            assert_ne!(
                decision.action,
                LoopGuardAction::Warn,
                "iter {} unexpected warn: {:?}",
                i,
                decision
            );
            assert_ne!(
                decision.action,
                LoopGuardAction::Stop,
                "iter {} unexpected stop: {:?}",
                i,
                decision
            );
        }
    }

    #[test]
    fn equivalent_stop_priority_over_cycle_when_both_apply() {
        // If exact-repeat-stop AND cycle-stop both apply, the more specific
        // exact-equivalent-stop message wins.
        let mut breaker = ToolLoopBreaker::default();
        let input = json!({"path": "/tmp/file"});
        let output = "same result";

        for _ in 0..TERMINAL_REPEAT_COUNT {
            let _ = breaker.record_result("read", &input, output);
        }
        let decision = breaker.before_call("read", &input);
        assert_eq!(decision.action, LoopGuardAction::Stop);
        assert!(
            decision.message.unwrap().contains("exact tool call"),
            "equivalent-terminal message should win: {:?}",
            decision.message
        );
    }

    #[test]
    fn three_cycle_with_stable_outputs_trips_cycle_warn() {
        // An A/B/C rotation should be caught just as reliably as A/B.
        let mut breaker = ToolLoopBreaker::default();
        let a_in = json!({"tool": "a"});
        let b_in = json!({"tool": "b"});
        let c_in = json!({"tool": "c"});
        let a_out = "a-result";
        let b_out = "b-result";
        let c_out = "c-result";

        // Two full A/B/C reps (6 calls) stay Allow.
        for _ in 0..2 {
            assert_eq!(
                breaker.record_result("a", &a_in, a_out).action,
                LoopGuardAction::Allow
            );
            assert_eq!(
                breaker.record_result("b", &b_in, b_out).action,
                LoopGuardAction::Allow
            );
            assert_eq!(
                breaker.record_result("c", &c_in, c_out).action,
                LoopGuardAction::Allow
            );
        }
        // Third rep: first call still Allow, third call (call 9) fires warn.
        assert_eq!(
            breaker.record_result("a", &a_in, a_out).action,
            LoopGuardAction::Allow
        );
        assert_eq!(
            breaker.record_result("b", &b_in, b_out).action,
            LoopGuardAction::Allow
        );
        let decision = breaker.record_result("c", &c_in, c_out);
        assert_eq!(decision.action, LoopGuardAction::Warn);
        assert!(decision.message.unwrap().contains("cycle"));
    }

    #[test]
    fn history_bounded_by_cycle_history_limit() {
        // After many calls the rolling history must stay bounded; we verify by
        // running far more calls than the limit and checking internal state
        // through the public surface (no panic, no overflow).
        let mut breaker = ToolLoopBreaker::default();
        for i in 0..(CYCLE_HISTORY_LIMIT * 4) {
            let tool = format!("t{}", i);
            let input = json!({"i": i});
            let output = format!("o{}", i);
            let _ = breaker.record_result(&tool, &input, &output);
        }
        // Should still be well below budget and cycle terminals.
        assert_eq!(
            breaker.before_call("any", &json!({})).action,
            LoopGuardAction::Allow
        );
    }
}
