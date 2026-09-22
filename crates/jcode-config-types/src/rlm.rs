use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RlmMode {
    #[default]
    Off,
    Observe,
    Pilot,
}

impl RlmMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Pilot => "pilot",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RlmManifestRetention {
    #[default]
    SessionScopedRedacted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RlmConfig {
    pub mode: RlmMode,
    pub max_depth: u8,
    pub max_children: u16,
    pub max_provider_calls: u32,
    pub max_tool_rounds_without_evidence: u32,
    pub manifest_retention: RlmManifestRetention,
}

impl Default for RlmConfig {
    fn default() -> Self {
        Self {
            mode: RlmMode::Off,
            max_depth: 1,
            max_children: 0,
            max_provider_calls: 12,
            max_tool_rounds_without_evidence: 3,
            manifest_retention: RlmManifestRetention::SessionScopedRedacted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_bounded() {
        let config = RlmConfig::default();
        assert_eq!(config.mode, RlmMode::Off);
        assert_eq!(config.max_depth, 1);
        assert_eq!(config.max_children, 0);
        assert_eq!(config.max_provider_calls, 12);
    }
}
