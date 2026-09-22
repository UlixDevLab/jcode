use serde::{Deserialize, Serialize};

/// Per-session tool surface. `ControlPlaneOnly` is intentionally opt-in and
/// restricts a coordinator to orchestration and reporting tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionToolPolicyMode {
    /// Ordinary sessions retain the configured normal tool surface.
    #[default]
    Normal,
    /// Limit a coordinator session to the native control-plane allow-list.
    ControlPlaneOnly,
}

impl SessionToolPolicyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::ControlPlaneOnly => "control-plane-only",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "control-plane-only" | "control_plane_only" | "controlplaneonly" => {
                Some(Self::ControlPlaneOnly)
            }
            _ => None,
        }
    }
}
