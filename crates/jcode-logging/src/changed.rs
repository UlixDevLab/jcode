use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Last logged outcome per caller-provided key.
///
/// The TUI resolves the account binding on every inline picker rebuild, so an
/// unconditional log line there produced ~500 identical lines per second
/// (6M lines, 980 MB in one day on 2026-09-06). Callers use this shared helper
/// to log only when an outcome for their key changes.
static LAST_LOGGED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// True when `outcome` differs from the last one recorded for `key`, recording
/// it as a side effect so the next identical call returns false.
fn outcome_changed(key: &str, outcome: &str) -> bool {
    let store = LAST_LOGGED.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut guard) = store.lock() else {
        return true;
    };
    match guard.get(key) {
        Some(previous) if previous == outcome => false,
        _ => {
            guard.insert(key.to_string(), outcome.to_string());
            true
        }
    }
}

/// Log an info message only when it changes for `key`.
pub fn info_if_changed(key: &str, message: String) {
    if outcome_changed(key, &message) {
        super::info(&message);
    }
}

/// Log a warning only when it changes for `key`.
pub fn warn_if_changed(key: &str, message: String) {
    if outcome_changed(key, &message) {
        super::warn(&message);
    }
}
