use serde::{Deserialize, Serialize};

/// Compact, privacy-safe metadata for an external terminal status row.
///
/// This deliberately contains only a sanitized repository or workspace basis,
/// the session start timestamp, and a bounded lifecycle-derived label. It must
/// never contain a raw path, prompt, transcript, tool input/output, credential,
/// or model output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionActivityContextSnapshot {
    pub repository: String,
    pub started_at: String,
    pub activity_label: String,
}
