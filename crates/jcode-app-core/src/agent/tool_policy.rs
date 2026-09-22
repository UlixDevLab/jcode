use super::*;

const CONTROL_PLANE_TOOL_NAMES: &[&str] = &[
    "initiative",
    "schedule",
    "side_panel",
    "swarm",
    "todo",
    "structural_review",
];

/// Internal worker construction profiles. These are deliberately separate from
/// `SessionToolPolicyMode`: adding a persisted mode would alter the public
/// session schema, while this profile only constrains a newly-created worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerSpawnProfile {
    ContextScoutLuna,
    SpecScoutLuna,
}

impl WorkerSpawnProfile {
    fn allowed_tools(self) -> Option<HashSet<String>> {
        // Every entry is a single-purpose read, search, list, or context
        // retrieval tool. Do not use the `mcp` wildcard here: unknown MCP
        // tools could mutate state and must fail closed instead.
        match self {
            Self::ContextScoutLuna => Some(
                [
                    "agentgrep",
                    "conversation_search",
                    "jcode_docs",
                    "ls",
                    "read",
                    "session_search",
                    "webfetch",
                    "websearch",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            ),
            // The specification is passed in the initial prompt. Giving this
            // worker no tools makes "read only the spec" an enforced boundary,
            // not a broad instruction it can accidentally exceed.
            Self::SpecScoutLuna => Some(HashSet::new()),
        }
    }
}

impl Agent {
    /// Construct an explicitly configured interactive root session.
    pub(crate) fn new_interactive_root_with_initial_working_dir(
        provider: Arc<dyn Provider>,
        registry: Registry,
        working_dir: Option<&str>,
    ) -> Self {
        Self::new_with_initial_working_dir_and_tool_policy_mode(
            provider,
            registry,
            working_dir,
            crate::config::config().agents.root_tool_policy_mode,
        )
    }

    pub(crate) fn new_with_initial_working_dir_and_tool_policy_mode(
        provider: Arc<dyn Provider>,
        registry: Registry,
        working_dir: Option<&str>,
        tool_policy_mode: crate::config::SessionToolPolicyMode,
    ) -> Self {
        let mut agent =
            Self::new_with_initial_ownership(provider, registry, working_dir, None, true);
        agent.session.tool_policy_mode = tool_policy_mode;
        agent.sync_session_tool_policy();
        agent
    }

    pub(crate) fn new_initial_client_with_initial_working_dir(
        provider: Arc<dyn Provider>,
        registry: Registry,
        working_dir: Option<&str>,
        selfdev: bool,
    ) -> Self {
        let mut agent =
            Self::new_provisional_with_initial_working_dir(provider, registry, working_dir);
        if !selfdev {
            agent.session.tool_policy_mode = crate::config::config().agents.root_tool_policy_mode;
            agent.sync_session_tool_policy();
        }
        agent
    }

    pub(crate) fn new_worker_with_initial_working_dir(
        provider: Arc<dyn Provider>,
        registry: Registry,
        working_dir: Option<&str>,
    ) -> Self {
        Self::new_with_initial_working_dir_and_tool_policy_mode(
            provider,
            registry,
            working_dir,
            crate::config::SessionToolPolicyMode::Normal,
        )
    }

    pub(crate) fn new_context_scout_luna_worker_with_initial_working_dir(
        provider: Arc<dyn Provider>,
        registry: Registry,
        working_dir: Option<&str>,
    ) -> Self {
        let mut agent = Self::new_worker_with_initial_working_dir(provider, registry, working_dir);
        agent.apply_context_scout_luna_profile();
        agent
    }

    pub(crate) fn apply_context_scout_luna_profile(&mut self) {
        self.apply_worker_spawn_profile(WorkerSpawnProfile::ContextScoutLuna);
    }

    pub(crate) fn apply_spec_scout_luna_profile(&mut self) {
        self.apply_worker_spawn_profile(WorkerSpawnProfile::SpecScoutLuna);
    }

    fn apply_worker_spawn_profile(&mut self, spawn_profile: WorkerSpawnProfile) {
        if let Some(profile_allowed) = spawn_profile.allowed_tools() {
            // Intersect with configured restrictions so the profile cannot add
            // a tool an operator has already disabled. `Some` also guarantees
            // unknown tools are denied rather than inheriting an open surface.
            self.allowed_tools = Some(match self.allowed_tools.take() {
                Some(configured_allowed) => configured_allowed
                    .intersection(&profile_allowed)
                    .cloned()
                    .collect(),
                None => profile_allowed,
            });
            self.sync_session_tool_policy();
        }
    }

    pub(crate) fn session_tool_policy_mode(&self) -> crate::config::SessionToolPolicyMode {
        self.session.tool_policy_mode
    }

    pub(super) fn session_allowed_tools(&self) -> Option<HashSet<String>> {
        if self.session.tool_policy_mode == crate::config::SessionToolPolicyMode::Normal {
            return self.allowed_tools.clone();
        }
        let control_plane_allowed = CONTROL_PLANE_TOOL_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        match &self.allowed_tools {
            Some(configured_allowed) => Some(
                configured_allowed
                    .intersection(&control_plane_allowed)
                    .cloned()
                    .collect(),
            ),
            None => Some(control_plane_allowed),
        }
    }
}
