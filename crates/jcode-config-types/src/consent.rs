use serde::{Deserialize, Serialize};

/// Controls whether the fixed, narrow navigation class can proceed without a
/// native approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsentNavigationPolicy {
    /// Allow only the built-in typed navigation operations. All other protected
    /// operations still require a native approval prompt.
    #[default]
    Allow,
    /// Prompt for every protected navigation operation as well.
    Prompt,
}

impl ConsentNavigationPolicy {
    pub fn allows_navigation(self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Native consent prompt policy.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsentConfig {
    /// The narrow built-in default-allow preset for local app/file and HTTP(S)
    /// navigation. Set to `"prompt"` to require native approval for navigation.
    pub navigation: ConsentNavigationPolicy,
}
