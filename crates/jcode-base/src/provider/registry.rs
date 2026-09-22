use super::*;

/// Centralizes runtime lookup and lifecycle decisions for provider slots that
/// can have more than one concrete runtime behind a single public provider.
///
/// Today the main case is `ActiveProvider::OpenRouter`: real OpenRouter and
/// direct OpenAI-compatible profiles share the OpenAI-compatible wire protocol,
/// but they are distinct runtime identities and must not overwrite each other.
pub(super) struct ProviderRegistry<'a> {
    provider: &'a MultiProvider,
}

impl<'a> ProviderRegistry<'a> {
    pub(super) fn new(provider: &'a MultiProvider) -> Self {
        Self { provider }
    }

    pub(super) fn real_openrouter(&self) -> Option<Arc<dyn Provider>> {
        self.provider
            .openrouter
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn compatible_profile(&self, profile_id: &str) -> Option<Arc<dyn Provider>> {
        self.provider
            .openai_compatible_profiles
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(profile_id)
            .cloned()
    }

    pub(super) fn install_compatible_profile(
        &self,
        profile_id: impl Into<String>,
        runtime: Arc<dyn Provider>,
    ) {
        self.provider
            .openai_compatible_profiles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(profile_id.into(), runtime);
    }

    pub(super) fn active_compatible_profile_id(&self) -> Option<String> {
        self.provider
            .active_openai_compatible_profile
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn set_active_compatible_profile(&self, profile_id: impl Into<String>) {
        *self
            .provider
            .active_openai_compatible_profile
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(profile_id.into());
    }

    pub(super) fn clear_active_compatible_profile(&self) {
        *self
            .provider
            .active_openai_compatible_profile
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    pub(super) fn active_compatible_profile(&self) -> Option<Arc<dyn Provider>> {
        let profile_id = self.active_compatible_profile_id()?;
        self.compatible_profile(&profile_id)
    }

    /// Runtime that should execute requests for the public OpenRouter slot.
    pub(super) fn active_openrouter_execution(&self) -> Option<Arc<dyn Provider>> {
        self.active_compatible_profile()
            .or_else(|| self.real_openrouter())
            // Last resort for the *failover* slot: an OpenAI-compatible profile
            // the operator configured but has not selected this session (e.g.
            // MiniMax while Claude/OpenAI are the working routes). Without this,
            // the OpenRouter slot reports "not configured" whenever no
            // OPENROUTER_API_KEY exists and no profile happens to be active, so
            // a paid MiniMax subscription could never catch a failover even
            // though it is fully set up. Deterministic by profile id so the
            // choice does not vary between runs.
            .or_else(|| self.first_configured_compatible_profile())
    }

    /// The configured OpenAI-compatible profile with the lowest id, if any.
    /// Used only as a failover fallback; explicit selection always wins.
    fn first_configured_compatible_profile(&self) -> Option<Arc<dyn Provider>> {
        let profiles = self
            .provider
            .openai_compatible_profiles
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut ids: Vec<&String> = profiles.keys().collect();
        ids.sort();
        ids.first().and_then(|id| profiles.get(*id).cloned())
    }
}
