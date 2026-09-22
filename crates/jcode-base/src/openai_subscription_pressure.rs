//! Local, deliberately approximate OpenAI subscription-pressure tracking.
//!
//! This is a comparative context-normalized index, not a provider quota meter.
//! The file retains only the current UTC day at
//! `~/.jcode/openai_subscription_pressure.json`, so its storage is bounded.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

const SCHEMA_VERSION: u8 = 1;

/// One completed turn's normalized token usage and runtime settings.
#[derive(Debug, Clone)]
pub struct OpenAiPressureTurn {
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub calls: u64,
    pub effective_input_tokens: u64,
    pub raw_input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub context_window_tokens: u64,
}

/// The deterministic, local estimate derived from one completed turn.
#[derive(Debug, Clone, Copy, Default)]
pub struct PressureEstimate {
    pub input_context_window_equivalents: f64,
    pub output_context_window_equivalents: f64,
    pub weighted_pressure_units: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PressureBreakdown {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default)]
    pub service_tier: String,
    #[serde(default)]
    pub calls: u64,
    #[serde(default)]
    pub input_context_window_equivalents: f64,
    #[serde(default)]
    pub output_context_window_equivalents: f64,
    #[serde(default)]
    pub weighted_pressure_units: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyOpenAiSubscriptionPressure {
    /// UTC `YYYY-MM-DD` for this bounded daily bucket.
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub calls: u64,
    #[serde(default)]
    pub effective_input_tokens: u64,
    #[serde(default)]
    pub raw_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub input_context_window_equivalents: f64,
    #[serde(default)]
    pub output_context_window_equivalents: f64,
    #[serde(default)]
    pub weighted_pressure_units: f64,
    #[serde(default)]
    pub high_max_or_priority_pressure_units: f64,
    #[serde(default)]
    pub breakdown: HashMap<String, PressureBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAiSubscriptionPressureStore {
    #[serde(default)]
    pub schema_version: u8,
    #[serde(default)]
    pub day: DailyOpenAiSubscriptionPressure,
}

static LEDGER: Mutex<()> = Mutex::new(());

fn meter_path() -> PathBuf {
    crate::storage::jcode_dir()
        .unwrap_or_else(|_| PathBuf::from(".").join(".jcode"))
        .join("openai_subscription_pressure.json")
}

fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

fn reasoning_weight(effort: &str) -> f64 {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" => 0.35,
        "low" => 0.55,
        "medium" => 0.8,
        "high" => 1.0,
        "xhigh" => 1.2,
        "max" => 1.4,
        _ => 0.35,
    }
}

fn tier_multiplier(tier: &str) -> f64 {
    if tier.trim().eq_ignore_ascii_case("priority") {
        1.2
    } else {
        1.0
    }
}

/// Computes `(effective_input / context_window * effort + output / context_window) * tier`.
/// `none` and absent/unknown effort use the conservative `0.35` multiplier.
pub fn estimate(turn: &OpenAiPressureTurn) -> PressureEstimate {
    if turn.context_window_tokens == 0 {
        return PressureEstimate::default();
    }
    let context = turn.context_window_tokens as f64;
    let input = turn.effective_input_tokens as f64 / context;
    let output = turn.output_tokens as f64 / context;
    PressureEstimate {
        input_context_window_equivalents: input,
        output_context_window_equivalents: output,
        weighted_pressure_units: (input * reasoning_weight(&turn.reasoning_effort) + output)
            * tier_multiplier(&turn.service_tier),
    }
}

fn is_openai(provider_name: &str) -> bool {
    provider_name.trim().eq_ignore_ascii_case("openai")
}

fn roll_to_day(day: &mut DailyOpenAiSubscriptionPressure, date: &str) {
    if day.date != date {
        *day = DailyOpenAiSubscriptionPressure {
            date: date.to_string(),
            ..Default::default()
        };
    }
}

fn breakdown_key(turn: &OpenAiPressureTurn) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        turn.model, turn.reasoning_effort, turn.service_tier
    )
}

fn high_max_or_priority(turn: &OpenAiPressureTurn) -> bool {
    matches!(
        turn.reasoning_effort.trim().to_ascii_lowercase().as_str(),
        "high" | "xhigh" | "max"
    ) || turn.service_tier.trim().eq_ignore_ascii_case("priority")
}

fn apply_turn(
    day: &mut DailyOpenAiSubscriptionPressure,
    turn: &OpenAiPressureTurn,
    estimate: PressureEstimate,
) {
    day.calls += turn.calls;
    day.effective_input_tokens += turn.effective_input_tokens;
    day.raw_input_tokens += turn.raw_input_tokens;
    day.output_tokens += turn.output_tokens;
    day.cache_read_input_tokens += turn.cache_read_input_tokens;
    day.cache_creation_input_tokens += turn.cache_creation_input_tokens;
    day.input_context_window_equivalents += estimate.input_context_window_equivalents;
    day.output_context_window_equivalents += estimate.output_context_window_equivalents;
    day.weighted_pressure_units += estimate.weighted_pressure_units;
    if high_max_or_priority(turn) {
        day.high_max_or_priority_pressure_units += estimate.weighted_pressure_units;
    }
    let entry = day
        .breakdown
        .entry(breakdown_key(turn))
        .or_insert_with(|| PressureBreakdown {
            model: turn.model.clone(),
            reasoning_effort: turn.reasoning_effort.clone(),
            service_tier: turn.service_tier.clone(),
            ..Default::default()
        });
    entry.calls += turn.calls;
    entry.input_context_window_equivalents += estimate.input_context_window_equivalents;
    entry.output_context_window_equivalents += estimate.output_context_window_equivalents;
    entry.weighted_pressure_units += estimate.weighted_pressure_units;
}

fn apply_openai_turn(
    day: &mut DailyOpenAiSubscriptionPressure,
    provider_name: &str,
    turn: &OpenAiPressureTurn,
) -> bool {
    if !is_openai(provider_name) || turn.calls == 0 || turn.context_window_tokens == 0 {
        return false;
    }
    apply_turn(day, turn, estimate(turn));
    true
}

/// Records an OpenAI turn against a freshly-read store before atomically writing it.
/// Calls for every other provider are intentionally ignored.
pub fn record_openai_turn(provider_name: &str, turn: &OpenAiPressureTurn) {
    let date = today();
    let _guard = LEDGER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut store: OpenAiSubscriptionPressureStore =
        crate::storage::read_json(&meter_path()).unwrap_or_default();
    store.schema_version = SCHEMA_VERSION;
    roll_to_day(&mut store.day, &date);
    if apply_openai_turn(&mut store.day, provider_name, turn) {
        let _ = crate::storage::write_json(&meter_path(), &store);
    }
}

/// Freshly reads the current UTC day's local estimate, returning zero after rollover.
pub fn today_summary() -> DailyOpenAiSubscriptionPressure {
    let mut store: OpenAiSubscriptionPressureStore =
        crate::storage::read_json(&meter_path()).unwrap_or_default();
    roll_to_day(&mut store.day, &today());
    store.day
}

pub fn local_estimate_extra_info() -> Vec<(String, String)> {
    let day = today_summary();
    let context_equivalents =
        day.input_context_window_equivalents + day.output_context_window_equivalents;
    let share = if day.weighted_pressure_units > 0.0 {
        day.high_max_or_priority_pressure_units / day.weighted_pressure_units * 100.0
    } else {
        0.0
    };
    vec![
        (
            "Local estimate: today's calls".to_string(),
            format!("{} (not provider quota)", day.calls),
        ),
        (
            "Local estimate: context equivalents".to_string(),
            format!("{context_equivalents:.3} (not provider quota)"),
        ),
        (
            "Local estimate: weighted pressure units".to_string(),
            format!("{:.3} (not provider quota)", day.weighted_pressure_units),
        ),
        (
            "Local estimate: high/max + priority share".to_string(),
            format!("{share:.1}% (not provider quota)"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(effort: &str, tier: &str) -> OpenAiPressureTurn {
        OpenAiPressureTurn {
            model: "gpt-5.6".to_string(),
            reasoning_effort: effort.to_string(),
            service_tier: tier.to_string(),
            calls: 1,
            effective_input_tokens: 50_000,
            raw_input_tokens: 50_000,
            output_tokens: 5_000,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 20,
            context_window_tokens: 100_000,
        }
    }

    #[test]
    fn max_priority_weighs_more_than_high_standard() {
        assert!(
            estimate(&turn("max", "priority")).weighted_pressure_units
                > estimate(&turn("high", "standard")).weighted_pressure_units
        );
    }

    #[test]
    fn rollover_discards_prior_day_totals() {
        let mut day = DailyOpenAiSubscriptionPressure {
            date: "2026-08-13".to_string(),
            calls: 3,
            ..Default::default()
        };
        roll_to_day(&mut day, "2026-08-14");
        assert_eq!(day.date, "2026-08-14");
        assert_eq!(day.calls, 0);
    }

    #[test]
    fn schema_defaults_preserve_old_or_partial_files() {
        let store: OpenAiSubscriptionPressureStore = serde_json::from_str("{}").unwrap();
        assert_eq!(store.schema_version, 0);
        assert_eq!(store.day.calls, 0);
    }

    #[test]
    fn non_openai_turn_is_not_applied() {
        let mut day = DailyOpenAiSubscriptionPressure::default();
        assert!(!apply_openai_turn(
            &mut day,
            "anthropic",
            &turn("high", "standard")
        ));
        assert_eq!(day.calls, 0);
    }

    #[test]
    fn report_lines_are_explicitly_local_estimates() {
        assert!(
            local_estimate_extra_info()
                .iter()
                .all(|(key, value)| key.starts_with("Local estimate:")
                    && value.contains("not provider quota"))
        );
    }
}
