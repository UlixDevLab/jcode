// sonar-file-size: {"waive":350,"owner":"jcode-maintainers","ticket":"Jcode-control-plane-policy","baseline_loc":1343,"max_loc":1343,"expires":"2026-12-01","removal_trigger":"module reaches 350 LOC","reason":"Policy extraction keeps inherited module at or below mission-base LOC."}
use super::*;
use crate::{terminal_eprintln as eprintln, terminal_println as println};
impl Agent {
    /// Run a single turn with the given user message
    pub async fn run_once(&mut self, user_message: &str) -> Result<()> {
        // Every Agent generation entry point owns this lease for its entire
        // turn. The read-side admission probe is released inside
        // `begin_generation`, but the returned lease remains live through
        // provider work, tool continuations, errors, and cancellation.
        let _generation_lease = crate::server::begin_generation()
            .await
            .map_err(|_| anyhow::anyhow!("server is reserved for idle recovery"))?;
        let input_id = self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: user_message.to_string(),
                cache_control: None,
            }],
        );
        if !user_message.trim().is_empty() {
            self.begin_model_usage_turn(&input_id);
        }
        self.session.save()?;
        if trace_enabled() {
            eprintln!("[trace] session_id {}", self.session.id);
        }
        // Non-streaming turns fire the same lifecycle hooks as streaming ones.
        // `jcode run` takes this path, so without these an automated run emits
        // post_tool events for a turn whose turn_start/turn_end never arrive,
        // leaving observers unable to close the turn they were told began.
        let turn_started_at = Instant::now();
        let start_message_index = self.message_count();
        self.fire_turn_start_hook("run");
        let result = self.run_turn(true).await;
        self.fire_turn_end_hook(
            &result
                .as_ref()
                .map(|_| ())
                .map_err(|error| anyhow::anyhow!("{error}")),
            turn_started_at,
            start_message_index,
        );
        result?;
        Ok(())
    }
    pub async fn run_once_capture(&mut self, user_message: &str) -> Result<String> {
        self.run_once_capture_with_display_role(user_message, None)
            .await
    }
    pub(crate) async fn run_once_capture_with_display_role(
        &mut self,
        user_message: &str,
        display_role: Option<crate::session::StoredDisplayRole>,
    ) -> Result<String> {
        // Keep the generation lease until this capture turn returns. Server
        // debug, swarm, spec-scout, and ambient paths all enter here.
        let _generation_lease = crate::server::begin_generation()
            .await
            .map_err(|_| anyhow::anyhow!("server is reserved for idle recovery"))?;
        let input_id = self.add_message_with_display_role(
            Role::User,
            vec![ContentBlock::Text {
                text: user_message.to_string(),
                cache_control: None,
            }],
            display_role,
        );
        if !user_message.trim().is_empty() {
            self.begin_model_usage_turn(&input_id);
        }
        self.session.save()?;
        if trace_enabled() {
            eprintln!("[trace] session_id {}", self.session.id);
        }
        let turn_started_at = Instant::now();
        let start_message_index = self.message_count();
        self.fire_turn_start_hook("run");
        let result = self.run_turn(false).await;
        self.fire_turn_end_hook(
            &result
                .as_ref()
                .map(|_| ())
                .map_err(|error| anyhow::anyhow!("{error}")),
            turn_started_at,
            start_message_index,
        );
        result
    }

    /// Run one conversation turn with streaming events via mpsc channel (per-client)
    pub async fn run_once_streaming_mpsc(
        &mut self,
        user_message: &str,
        images: Vec<(String, String)>,
        system_reminder: Option<String>,
        event_tx: mpsc::UnboundedSender<ServerEvent>,
    ) -> Result<()> {
        self.run_once_streaming_mpsc_with_display_role(
            user_message,
            images,
            system_reminder,
            event_tx,
            None,
        )
        .await
    }

    pub(crate) async fn run_once_streaming_mpsc_with_display_role(
        &mut self,
        user_message: &str,
        images: Vec<(String, String)>,
        system_reminder: Option<String>,
        event_tx: mpsc::UnboundedSender<ServerEvent>,
        display_role: Option<crate::session::StoredDisplayRole>,
    ) -> Result<()> {
        // Keep the generation lease until this streaming turn returns. The
        // wrapper above delegates here, so there is exactly one lease.
        let _generation_lease = crate::server::begin_generation()
            .await
            .map_err(|_| anyhow::anyhow!("server is reserved for idle recovery"))?;
        // Inject any pending notifications before the user message
        let alerts = self.take_alerts();
        if !alerts.is_empty() {
            let alert_text = format!(
                "[NOTIFICATION]\nYou received {} notification(s) from other agents working in this codebase:\n\n{}\n\nUse the communicate tool to coordinate with other agents (prefer dm; broadcast reaches only your spawned subtree).",
                alerts.len(),
                alerts.join("\n\n---\n\n")
            );
            self.add_message(
                Role::User,
                vec![ContentBlock::Text {
                    text: alert_text,
                    cache_control: None,
                }],
            );
        }

        self.current_turn_system_reminder =
            system_reminder.filter(|value| !value.trim().is_empty());

        self.append_user_context_message_with_display_role(user_message, images, display_role)?;
        crate::telemetry::record_turn();
        let turn_started_at = Instant::now();
        let start_message_index = self.message_count();
        self.fire_turn_start_hook("chat");
        let result = self.run_turn_streaming_mpsc(event_tx).await;
        self.current_turn_system_reminder = None;
        self.fire_turn_end_hook(&result, turn_started_at, start_message_index);
        result
    }

    /// Append and persist a user message without starting a model turn.
    pub(crate) fn append_user_context_message(
        &mut self,
        user_message: &str,
        images: Vec<(String, String)>,
    ) -> Result<()> {
        self.append_user_context_message_with_display_role(user_message, images, None)
    }

    fn append_user_context_message_with_display_role(
        &mut self,
        user_message: &str,
        images: Vec<(String, String)>,
        display_role: Option<crate::session::StoredDisplayRole>,
    ) -> Result<()> {
        let mut blocks: Vec<ContentBlock> = images
            .into_iter()
            .map(|(media_type, data)| ContentBlock::Image { media_type, data })
            .collect();
        blocks.push(ContentBlock::Text {
            text: user_message.to_string(),
            cache_control: None,
        });

        if blocks.len() > 1 {
            crate::logging::info(&format!(
                "Agent received message with {} image(s)",
                blocks.len() - 1
            ));
        }

        let starts_turn = blocks.len() > 1 || !user_message.trim().is_empty();
        let input_id = self.add_message_with_display_role(Role::User, blocks, display_role);
        if starts_turn {
            self.begin_model_usage_turn(&input_id);
        }
        self.session.save()
    }

    /// Fire the `turn_start` observer hook when a turn begins, before the model
    /// starts generating (and before the first `pre_tool`). This lets external
    /// integrations (terminal multiplexers, status bars) detect that the agent
    /// is actively working during the otherwise-invisible window between prompt
    /// submission and the first tool call. No-op (without building the payload)
    /// when the hook is not configured.
    fn fire_turn_start_hook(&self, source: &str) {
        if !crate::hooks::hook_configured("turn_start") {
            return;
        }
        let event = crate::hooks::HookEvent::new("turn_start")
            .session_id(self.session.id.clone())
            .field("MODEL", self.provider_model())
            .field("SOURCE", source.to_string());
        crate::hooks::dispatch_observer(self.with_lifecycle_hook_context(event, true));
    }

    /// Build the `turn_end` observer hook event with turn outcome metadata.
    /// Public for tests so the field contract can be asserted directly without
    /// spawning the bridge process. `fire_turn_end_hook` is the dispatch wrapper.
    pub(crate) fn turn_end_hook_event(
        &self,
        result: &Result<()>,
        started_at: Instant,
        start_message_index: usize,
    ) -> crate::hooks::HookEvent {
        let status = if result.is_ok() { "ok" } else { "error" };
        let mut event = crate::hooks::HookEvent::new("turn_end")
            .session_id(self.session.id.clone())
            .field("STATUS", status)
            .field("DURATION_MS", started_at.elapsed().as_millis().to_string())
            .field("MODEL", self.provider_model())
            .field("CONTEXT_MODE", self.rlm_context_mode_tag());
        event = self.with_lifecycle_hook_context(event, false);
        if let Some(text) = self.latest_assistant_text_after(start_message_index) {
            const LAST_TEXT_LIMIT: usize = 4000;
            let snippet: String = text.chars().take(LAST_TEXT_LIMIT).collect();
            event = event.field("LAST_ASSISTANT_TEXT", snippet);
        }
        if let Err(error) = result {
            const ERROR_LIMIT: usize = 1000;
            let message: String = error.to_string().chars().take(ERROR_LIMIT).collect();
            event = event.field("ERROR", message);
        }
        // Per-turn token usage for the Langfuse GENERATION bridge: feed exactly
        // the messages that produced this turn so prior-session usage is not
        // re-counted on every emission. Fields are suppressed entirely when no
        // message in the turn reported usage.
        let totals = self.session.token_usage_totals_since(start_message_index);
        let provider_name = self.provider.name();
        let context_window_tokens = self.provider.context_window() as u64;
        let reasoning_effort = self
            .provider
            .reasoning_effort()
            .unwrap_or_else(|| "none".to_string());
        let service_tier = self
            .provider
            .service_tier()
            .unwrap_or_else(|| "standard".to_string());
        let mut event = append_turn_usage_fields(
            event,
            provider_name,
            totals,
            context_window_tokens,
            &reasoning_effort,
            &service_tier,
        );
        if let Some(turn) = openai_pressure_turn(
            provider_name,
            self.provider_model(),
            reasoning_effort,
            service_tier,
            context_window_tokens,
            totals,
        ) {
            let estimate = crate::openai_subscription_pressure::estimate(&turn);
            event = event.field(
                "OPENAI_SUBSCRIPTION_PRESSURE_UNITS",
                format!("{:.6}", estimate.weighted_pressure_units),
            );
        }
        event
    }

    /// Fire the `turn_end` observer hook with turn outcome metadata.
    /// No-op (without building the payload) when the hook is not configured.
    fn fire_turn_end_hook(
        &self,
        result: &Result<()>,
        started_at: Instant,
        start_message_index: usize,
    ) {
        self.record_openai_subscription_pressure(start_message_index);
        if let Err(error) =
            crate::shadow_context_state::maybe_emit_for_completed_turn(&self.session)
        {
            crate::logging::warn(&format!("Shadow context-state emission failed: {error}"));
        }
        if !crate::hooks::hook_configured("turn_end") {
            return;
        }
        let event = self.turn_end_hook_event(result, started_at, start_message_index);
        crate::hooks::dispatch_observer(event);
    }

    /// The meter deliberately records regardless of observer-hook configuration.
    /// It is local context pressure, never a provider quota percentage.
    fn record_openai_subscription_pressure(&self, start_message_index: usize) {
        let totals = self.session.token_usage_totals_since(start_message_index);
        let provider_name = self.provider.name();
        if let Some(turn) = openai_pressure_turn(
            provider_name,
            self.provider_model(),
            self.provider
                .reasoning_effort()
                .unwrap_or_else(|| "none".to_string()),
            self.provider
                .service_tier()
                .unwrap_or_else(|| "standard".to_string()),
            self.provider.context_window() as u64,
            totals,
        ) {
            crate::openai_subscription_pressure::record_openai_turn(provider_name, &turn);
        }
    }

    /// Clear conversation history
    pub fn clear(&mut self) {
        let previous_session_id = self.session.id.clone();
        let preserve_canary = self.session.is_canary;
        let preserve_testing_build = self.session.testing_build.clone();
        let preserve_debug = self.session.is_debug;
        let preserve_working_dir = self.session.working_dir.clone();
        let preserve_tool_policy_mode = self.session.tool_policy_mode;

        self.session.mark_closed();
        self.finish_concurrency_tracking();
        self.persist_session_best_effort("pre-clear session close state");

        let mut new_session = Session::create(None, None);
        new_session.mark_active();
        new_session.model = Some(self.provider_model());
        new_session.provider_key = self.provider_key_for_new_session();
        new_session.is_canary = preserve_canary;
        new_session.testing_build = preserve_testing_build;
        new_session.is_debug = preserve_debug;
        new_session.working_dir = preserve_working_dir;
        new_session.tool_policy_mode = preserve_tool_policy_mode;
        new_session.ensure_initial_session_context_message();

        self.session = new_session;
        self.begin_concurrency_tracking();
        self._tool_policy_registration = crate::tool::register_session_tool_policy(
            &self.session.id,
            self.allowed_tools.clone(),
            self.disabled_tools.clone(),
        );
        self.refresh_agents_md_snapshot();
        self.reconcile_explicit_provider_pin_route();
        self.reset_runtime_state_for_session_change();
        self.sync_session_tool_policy();
        self.provider_session_id = None;
        self.seed_compaction_from_session();
    }

    /// Clear provider session so the next turn sends full context.
    pub fn reset_provider_session(&mut self) {
        self.provider_session_id = None;
        self.session.provider_session_id = None;
        self.persist_session_best_effort("provider session reset");
    }

    /// Rewind the conversation to a 1-based visible transcript message index.
    ///
    /// The index is interpreted against the same rendered transcript the TUI
    /// numbers in `/rewind` (user/assistant entries only, tool cards and
    /// system notices excluded). Mapping through raw stored messages instead
    /// would count tool-result messages the UI never numbers, sending
    /// `/rewind N` far earlier than the on-screen message N (issue #432).
    ///
    /// Provider-side resumable sessions are reset so the next request sends the
    /// truncated context from scratch instead of continuing from a stale upstream
    /// conversation.
    pub fn rewind_to_message(&mut self, message_index: usize) -> Result<usize, String> {
        let targets = self.session.rewind_target_stored_indices();
        let message_count = targets.len();
        if message_index == 0 || message_index > message_count {
            return Err(format!(
                "Invalid message number: {}. Valid range: 1-{}",
                message_index, message_count
            ));
        }
        let stored_len = targets[message_index - 1] + 1;

        let removed = message_count - message_index;
        self.rewind_undo_snapshot = Some(RewindUndoSnapshot {
            messages: self.session.messages.clone(),
            provider_session_id: self.provider_session_id.clone(),
            session_provider_session_id: self.session.provider_session_id.clone(),
            visible_message_count: message_count,
        });
        self.session.truncate_messages(stored_len);
        self.session.updated_at = chrono::Utc::now();
        self.provider_session_id = None;
        self.session.provider_session_id = None;
        self.cache_tracker.reset();
        self.locked_tools = None;
        self.reset_tool_output_tracking();
        self.persist_session_best_effort("conversation rewind");
        Ok(removed)
    }

    pub fn undo_rewind(&mut self) -> Result<usize, String> {
        let Some(snapshot) = self.rewind_undo_snapshot.take() else {
            return Err("No rewind to undo.".to_string());
        };

        let current_count = self.session.rewind_target_count();
        let restored = snapshot.visible_message_count.saturating_sub(current_count);
        self.session.replace_messages(snapshot.messages);
        self.provider_session_id = snapshot.provider_session_id;
        self.session.provider_session_id = snapshot.session_provider_session_id;
        self.session.updated_at = chrono::Utc::now();
        self.cache_tracker.reset();
        self.locked_tools = None;
        self.reset_tool_output_tracking();
        self.persist_session_best_effort("conversation rewind undo");
        Ok(restored)
    }

    /// Unlock the tool list so the next API request picks up any new tools.
    /// Called after MCP reload or when the user explicitly wants new tools.
    pub fn unlock_tools(&mut self) {
        if self.locked_tools.is_some() {
            logging::info("Tool list unlocked — next request will pick up current tools");
            self.locked_tools = None;
            self.cache_tracker.reset();
        }
        // Allow the late-MCP-registration recheck to fire once for the next
        // snapshot (e.g. after an explicit `mcp` reload).
        self.mcp_late_register_resolved = false;
    }

    /// Unlock tools if a tool execution may have changed the registry
    /// (e.g., mcp connect/disconnect/reload)
    pub(super) fn unlock_tools_if_needed(&mut self, tool_name: &str) {
        if tool_name == "mcp" {
            self.unlock_tools();
        }
    }

    pub fn is_canary(&self) -> bool {
        self.session.is_canary
    }

    pub fn is_debug(&self) -> bool {
        self.session.is_debug
    }

    pub fn set_canary(&mut self, build_hash: &str) {
        if !self.session.is_canary {
            // Self-dev changes the tool surface, including hiding bundled docs.
            self.unlock_tools();
        }
        self.session.set_canary(build_hash);
        if let Err(err) = self.session.save() {
            logging::error(&format!("Failed to persist canary session state: {}", err));
        }
    }

    /// Mark this session as a debug/test session
    /// Set a custom system prompt override (used by ambient mode).
    /// When set, this replaces the normal system prompt entirely.
    pub fn set_system_prompt(&mut self, prompt: &str) {
        self.system_prompt_override = Some(prompt.to_string());
    }

    pub fn set_debug(&mut self, is_debug: bool) {
        self.session.set_debug(is_debug);
        if let Err(err) = self.session.save() {
            logging::error(&format!("Failed to persist debug session state: {}", err));
        }
    }

    /// Enable or disable memory features for this session.
    pub fn set_memory_enabled(&mut self, enabled: bool) {
        self.memory_enabled = enabled;
        if !enabled {
            crate::memory::clear_pending_memory(&self.session.id);
        }
    }

    /// Mark this session as an inline swarm worker. When enabled, the streaming
    /// loop publishes a throttled output tail to the global bus so a
    /// coordinator can render a live inline gallery viewport for it.
    pub fn set_inline_output_tap(&mut self, enabled: bool) {
        self.inline_output_tap = enabled;
    }

    /// Install the server-derived structural-review capability for this live
    /// actor. The model cannot construct this value through a tool argument.
    pub(crate) fn set_structural_review_authority(
        &mut self,
        authority: Arc<dyn jcode_tool_core::StructuralReviewAuthority>,
    ) {
        self.structural_review_authority = Some(authority);
    }

    pub(crate) fn structural_review_authority(
        &self,
    ) -> Option<Arc<dyn jcode_tool_core::StructuralReviewAuthority>> {
        self.structural_review_authority.clone()
    }

    /// Whether this session streams an inline output tail to the bus.
    pub(crate) fn inline_output_tap(&self) -> bool {
        self.inline_output_tap
    }

    /// Return the session allow-list narrowed by an active skill's existing
    /// `allowed-tools` declaration. A skill without that declaration remains
    /// observational only, preserving the normal full tool surface. `all` and
    /// `*` are an explicit skill-level escape hatch and retain the session's
    /// configured surface.
    fn effective_allowed_tools(&self) -> Option<HashSet<String>> {
        let session_allowed_tools = self.session_allowed_tools();
        let Some(skill_name) = self.active_skill.as_deref() else {
            return session_allowed_tools;
        };
        let skills = self.current_skills_snapshot();
        let Some(skill) = skills.get(skill_name) else {
            return session_allowed_tools;
        };
        let Some(skill_allowed) = skill.allowed_tools.as_ref() else {
            return session_allowed_tools;
        };
        if skill_allowed.iter().any(|name| {
            let name = name.trim();
            name == "*" || name.eq_ignore_ascii_case("all")
        }) {
            return session_allowed_tools;
        }

        let mut skill_allowed: HashSet<String> = skill_allowed
            .iter()
            .map(|name| crate::tool::Registry::resolve_tool_name(name.trim()).to_string())
            .filter(|name| !name.is_empty())
            .collect();
        // `selfdev` is a canary control tool rather than a task capability.
        // Preserve it for the default session surface while keeping an explicit
        // session allow-list authoritative.
        if self.session.is_canary && session_allowed_tools.is_none() {
            skill_allowed.insert("selfdev".to_string());
        }
        match &session_allowed_tools {
            Some(session_allowed) => Some(
                session_allowed
                    .intersection(&skill_allowed)
                    .cloned()
                    .collect(),
            ),
            None => Some(skill_allowed),
        }
    }

    /// Keep direct registry execution aligned with the schemas sent to the
    /// model. This is refreshed whenever an active skill changes.
    pub(super) fn sync_session_tool_policy(&self) {
        self._tool_policy_registration
            .update(self.effective_allowed_tools(), self.disabled_tools.clone());
    }

    pub(super) fn set_active_skill(&mut self, skill_name: String) {
        if skill_name == "fable" {
            crate::tool::fable::authorize_explicit_invocation(&self.session.id);
        }
        if self.active_skill.as_deref() == Some(skill_name.as_str()) {
            // Repeating `/fable` is a new explicit request and grants a fresh
            // one-shot permit even though the active skill name is unchanged.
            if skill_name == "fable" {
                self.sync_session_tool_policy();
                self.unlock_tools();
            }
            return;
        }
        self.active_skill = Some(skill_name);
        self.sync_session_tool_policy();
        // An active skill changes the model-visible surface, so deliberately
        // replace the previously locked snapshot instead of mixing scopes.
        self.unlock_tools();
    }

    /// Publish the current rolling activity tail to the bus for the
    /// coordinator's inline gallery. No-op unless the inline tap is enabled.
    pub(crate) fn publish_inline_tail(&self) {
        if !self.inline_output_tap {
            return;
        }
        crate::bus::Bus::global().publish(crate::bus::BusEvent::SwarmOutputTail(
            crate::bus::SwarmOutputTail {
                session_id: self.session.id.clone(),
                tail: self.inline_tail.render(),
            },
        ));
    }

    /// Check whether memory features are enabled for this session.
    pub fn memory_enabled(&self) -> bool {
        self.memory_enabled
    }

    /// Set the stdin request channel for interactive stdin forwarding
    pub fn set_stdin_request_tx(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tool::StdinInputRequest>,
    ) {
        self.stdin_request_tx = Some(tx);
    }

    #[cfg(test)]
    pub(crate) fn stdin_request_tx_for_test(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<crate::tool::StdinInputRequest>> {
        self.stdin_request_tx.clone()
    }

    /// Prepare the static provider prefix while a client is idle. Unlike
    /// `tool_definitions`, this does not pin the tool snapshot or consume the
    /// one-shot late-MCP-discovery check before the first real turn.
    pub(crate) async fn prewarm_provider(&mut self) {
        if self.session.is_canary {
            self.registry.register_selfdev_tools().await;
        }
        let tools = match &self.locked_tools {
            Some(tools) => tools.clone(),
            None => self.build_filtered_tool_definitions().await,
        };
        let prompt = self.build_system_prompt_split(None);
        self.provider.prewarm(&tools, &prompt.static_part).await;
    }

    pub(super) async fn tool_definitions(&mut self) -> Vec<ToolDefinition> {
        if self.session.is_canary {
            self.registry.register_selfdev_tools().await;
        }

        // Account sign-in/out and verified entitlement changes must reach the
        // model even when the tool list is frozen (including deferred MCP).
        // Only update this definition when its guidance actually changes.
        if self
            .locked_tools
            .as_ref()
            .is_some_and(|tools| tools.iter().any(|tool| tool.name == "compile_remote"))
            && let Some(fresh) = self.registry.remote_compile_definition().await
            && let Some(locked) = self.locked_tools.as_mut()
            && let Some(previous) = locked.iter_mut().find(|tool| tool.name == "compile_remote")
            && (previous.description != fresh.description
                || previous.input_schema != fresh.input_schema)
        {
            *previous = fresh;
            self.cache_tracker.reset();
        }

        // Return locked tools if available (prevents cache invalidation from
        // tools arriving asynchronously after the first API request).
        //
        // Exception: MCP servers connect on a background task and register
        // `mcp__*` tools seconds after the session starts — typically *after*
        // the first turn has already locked the snapshot. We deliberately do
        // NOT block the first turn on MCP connection: servers can be slow or
        // hang, and we want the user to be able to talk to the agent the moment
        // the session spawns. The price is that the first locked snapshot is
        // missing MCP tools, and the only other unlock path fires when the model
        // calls the `mcp` management tool — which it cannot do without first
        // seeing MCP tools (#206).
        //
        // So, exactly once per locked snapshot, if MCP tools have since appeared
        // in the registry, we rebuild. This is a single intentional provider
        // prompt-cache miss (the turn MCP tools first appear). The
        // `mcp_late_register_resolved` flag makes this a one-shot check so we do
        // not rescan the registry on every subsequent turn.
        let locked_uses_fixed_mcp_surface = self.locked_tools.as_ref().is_some_and(|locked| {
            locked
                .iter()
                .any(|tool| matches!(tool.name.as_str(), "mcp_search" | "mcp_call"))
                && !locked.iter().any(|tool| tool.name.starts_with("mcp__"))
        });
        if (self.mcp_tools_mode == crate::config::McpToolsMode::Deferred
            || locked_uses_fixed_mcp_surface)
            && let Some(locked) = self.locked_tools.clone()
        {
            // Per-server tools may continue registering in the background, but
            // deferred mode's fixed surface cannot change as a result. Avoid an
            // unnecessary provider cache reset and registry scan.
            self.mcp_late_register_resolved = true;
            return locked;
        }
        if let Some(ref locked) = self.locked_tools {
            if self.mcp_late_register_resolved {
                return locked.clone();
            }
            if self.registry_has_new_mcp_tools(locked).await {
                logging::info(
                    "MCP tools registered after first turn locked the tool snapshot — \
                     rebuilding once to expose them. This is one intentional prompt-cache \
                     miss; we accept it so the agent is reachable immediately at spawn \
                     instead of blocking on MCP connection (#206).",
                );
                // Latch the one-shot guard and drop the stale snapshot directly.
                // We intentionally do NOT call `unlock_tools()` here, because that
                // re-arms the guard (it is the explicit-reload path) and would let
                // the recheck fire again on every later turn.
                self.mcp_late_register_resolved = true;
                self.locked_tools = None;
                self.cache_tracker.reset();
            } else {
                // No MCP tools have appeared. They may still be connecting, so
                // leave the guard unset and re-check on the next turn. Once they
                // appear (or never do, after the registry settles) we stop.
                return locked.clone();
            }
        }

        let tools = self.build_filtered_tool_definitions().await;

        // Lock the tool list to prevent cache invalidation when more tools
        // arrive asynchronously mid-session.
        logging::info(&format!(
            "Locking tool list at {} tools for cache stability",
            tools.len()
        ));
        self.locked_tools = Some(tools.clone());
        tools
    }

    /// Build the agent's tool definitions from the registry, applying the
    /// session's `allowed_tools`, `disabled_tools`, and self-dev filters.
    async fn build_filtered_tool_definitions(&self) -> Vec<ToolDefinition> {
        let allowed_tools = self.effective_allowed_tools();
        let mut tools = self.registry.definitions(allowed_tools.as_ref()).await;
        if !self.disabled_tools.is_empty() {
            tools.retain(|tool| {
                !self
                    .registry
                    .tool_is_disabled(&self.disabled_tools, &tool.name)
            });
        }
        if self.active_skill.as_deref() != Some("fable") {
            tools.retain(|tool| tool.name != "fable");
        }
        Self::apply_selfdev_tool_surface(
            &mut tools,
            self.session.is_canary,
            self.is_desktop_selfdev(),
        );
        self.apply_mcp_tool_exposure(&mut tools);
        tools
    }

    /// Replace per-server MCP definitions with the fixed search/call surface
    /// according to the configured mode. Auto mode estimates the actual
    /// serialized, already-filtered definitions the provider would receive.
    fn apply_mcp_tool_exposure(&self, tools: &mut Vec<ToolDefinition>) {
        let mcp_definitions: Vec<ToolDefinition> = tools
            .iter()
            .filter(|tool| tool.name.starts_with("mcp__"))
            .cloned()
            .collect();
        let estimated_tokens = ToolDefinition::aggregate_prompt_token_estimate(&mcp_definitions);
        let deferred = match self.mcp_tools_mode {
            crate::config::McpToolsMode::Auto => estimated_tokens > self.mcp_tools_token_threshold,
            crate::config::McpToolsMode::Eager => false,
            crate::config::McpToolsMode::Deferred => true,
        };

        if deferred {
            tools.retain(|tool| !tool.name.starts_with("mcp__"));
        } else {
            tools.retain(|tool| !matches!(tool.name.as_str(), "mcp_search" | "mcp_call"));
        }
    }

    /// Expose the `selfdev` tool only while running in self-development mode.
    /// Self-dev agents use the working tree rather than bundled `jcode_docs`,
    /// which can lag behind the source they are editing.
    ///
    /// The registry keeps the implementation available for self-dev sessions,
    /// but regular agents should not spend tool-list context on an internal
    /// development surface.
    fn apply_selfdev_tool_surface(
        tools: &mut Vec<ToolDefinition>,
        is_canary: bool,
        is_desktop: bool,
    ) {
        // Desktop development is a separate product mode, not a CLI canary.
        // Never advertise CLI build/reload or TUI debug sockets in that mode.
        if is_desktop {
            tools.retain(|tool| {
                !matches!(
                    tool.name.as_str(),
                    "selfdev" | "debug_socket" | "jcode_docs"
                )
            });
            return;
        }
        tools.retain(|tool| tool.name != "desktop_selfdev");
        if !is_canary {
            tools.retain(|tool| tool.name != "selfdev");
            return;
        }
        tools.retain(|tool| tool.name != "jcode_docs");
        for tool in tools.iter_mut() {
            if tool.name == "selfdev" {
                tool.description =
                    crate::tool::selfdev::SelfDevTool::description_for(true).to_string();
                tool.input_schema = crate::tool::selfdev::SelfDevTool::schema_for(true);
            }
        }
    }

    /// Returns true if the registry contains `mcp__*` tools (subject to the
    /// session's `allowed_tools` filter) that are not present in the currently
    /// locked snapshot. Used to detect the async MCP-registration race (#206).
    async fn registry_has_new_mcp_tools(&self, locked: &[ToolDefinition]) -> bool {
        let registry_names = self.registry.tool_names().await;
        let allowed_tools = self.effective_allowed_tools();
        let allowed = allowed_tools.as_ref();
        registry_names.iter().any(|name| {
            name.starts_with("mcp__")
                && allowed
                    .map(|set| self.registry.tool_is_allowed(set, name))
                    .unwrap_or(true)
                && !self.registry.tool_is_disabled(&self.disabled_tools, name)
                && !locked.iter().any(|t| &t.name == name)
        })
    }

    pub async fn tool_names(&self) -> Vec<String> {
        self.tool_definitions_for_debug()
            .await
            .into_iter()
            .map(|tool| tool.name)
            .collect()
    }

    /// Get full tool definitions for debug introspection (bypasses lock)
    pub async fn tool_definitions_for_debug(&self) -> Vec<crate::message::ToolDefinition> {
        if self.session.is_canary {
            self.registry.register_selfdev_tools().await;
        }
        self.build_filtered_tool_definitions().await
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<crate::tool::ToolOutput> {
        self.validate_tool_allowed(name)?;

        let call_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("debug-{}", d.as_millis()))
            .unwrap_or_else(|_| "debug".to_string());
        let ctx = ToolContext {
            session_id: self.session.id.clone(),
            message_id: self.session.id.clone(),
            tool_call_id: call_id,
            working_dir: self.working_dir().map(PathBuf::from),
            stdin_request_tx: self.stdin_request_tx.clone(),
            graceful_shutdown_signal: Some(self.graceful_shutdown.clone()),
            structural_review_authority: self.structural_review_authority.clone(),
            execution_mode: ToolExecutionMode::Direct,
        };
        self.registry.execute(name, input, ctx).await
    }

    pub fn add_manual_tool_use(
        &mut self,
        tool_call_id: String,
        tool_name: String,
        input: serde_json::Value,
    ) -> Result<String> {
        let message_id = self.add_message(
            Role::Assistant,
            vec![ContentBlock::ToolUse {
                id: tool_call_id,
                name: tool_name,
                input,
                thought_signature: None,
            }],
        );
        self.session.save()?;
        Ok(message_id)
    }

    pub fn add_manual_tool_result(
        &mut self,
        tool_call_id: String,
        output: crate::tool::ToolOutput,
        duration_ms: u64,
    ) -> Result<()> {
        let blocks = tool_output_to_content_blocks(tool_call_id, output);
        self.add_message_with_duration(Role::User, blocks, Some(duration_ms));
        self.session.save()?;
        Ok(())
    }

    pub fn add_manual_tool_error(
        &mut self,
        tool_call_id: String,
        error: String,
        duration_ms: u64,
    ) -> Result<()> {
        self.add_message_with_duration(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: tool_call_id,
                content: error,
                is_error: Some(true),
            }],
            Some(duration_ms),
        );
        self.session.save()?;
        Ok(())
    }

    pub(super) fn validate_tool_allowed(&self, name: &str) -> Result<()> {
        if name == "fable" && self.active_skill.as_deref() != Some("fable") {
            return Err(anyhow::anyhow!(
                "Tool 'fable' is available only during an explicit /fable invocation"
            ));
        }
        let is_desktop = self.is_desktop_selfdev();
        if is_desktop && matches!(name, "selfdev" | "debug_socket") {
            return Err(anyhow::anyhow!(
                "Tool '{}' targets Jcode CLI, not Desktop. Use 'desktop_selfdev' in Desktop self-development mode.",
                name
            ));
        }
        if !is_desktop && name == "desktop_selfdev" {
            return Err(anyhow::anyhow!(
                "Tool 'desktop_selfdev' is only available in a Jcode Desktop source checkout."
            ));
        }
        if (self.session.is_canary || is_desktop) && name == "jcode_docs" {
            return Err(anyhow::anyhow!(
                "Tool 'jcode_docs' is disabled in self-development mode. Read the working tree documentation instead."
            ));
        }
        if let Some(allowed) = self.effective_allowed_tools().as_ref()
            && !self.registry.tool_is_allowed(allowed, name)
        {
            return Err(anyhow::anyhow!("Tool '{}' is not allowed", name));
        }
        if self.registry.tool_is_disabled(&self.disabled_tools, name) {
            return Err(anyhow::anyhow!("Tool '{}' is disabled", name));
        }
        Ok(())
    }

    /// Restore a session by ID (loads from disk)
    pub fn restore_session(&mut self, session_id: &str) -> Result<SessionStatus> {
        self.restore_session_with_working_dir(session_id, None)
    }

    pub(crate) fn restore_session_with_working_dir(
        &mut self,
        session_id: &str,
        working_dir: Option<&str>,
    ) -> Result<SessionStatus> {
        let restore_start = Instant::now();
        let load_start = Instant::now();
        let mut session = Session::load(session_id)?;
        if let Some(working_dir) = working_dir {
            session.working_dir = Some(working_dir.to_string());
            session.refresh_initial_session_context_message();
        }
        let load_ms = load_start.elapsed().as_millis();
        logging::info(&format!(
            "Restoring session '{}' with {} messages, provider_session_id: {:?}, status: {}",
            session_id,
            session.messages.len(),
            session.provider_session_id,
            session.status.display()
        ));
        let previous_status = session.status.clone();

        let assign_start = Instant::now();
        // A failed load must leave the current Agent and its concurrency lease
        // alive. Close it only after the replacement is ready to install.
        self.mark_closed();
        // Restore provider_session_id for Claude CLI session resume
        self.provider_session_id = session.provider_session_id.clone();
        self.session = session;
        self.refresh_agents_md_snapshot();
        self._tool_policy_registration = crate::tool::register_session_tool_policy(
            &self.session.id,
            self.allowed_tools.clone(),
            self.disabled_tools.clone(),
        );
        let assign_ms = assign_start.elapsed().as_millis();

        let reset_start = Instant::now();
        self.reset_runtime_state_for_session_change();
        self.sync_session_tool_policy();
        let restored_soft_interrupts = self.restore_persisted_soft_interrupts();
        let reset_ms = reset_start.elapsed().as_millis();

        let model_start = Instant::now();
        if let Some(model) = self.session.model.clone() {
            let model_request =
                crate::provider::MultiProvider::model_switch_request_for_session_route(
                    &model,
                    self.session.provider_key.as_deref(),
                    self.session.route_api_method.as_deref(),
                );
            if let Err(e) =
                crate::provider::set_model_with_auth_refresh(self.provider.as_ref(), &model_request)
            {
                logging::error(&format!(
                    "Failed to restore session model '{}' via '{}': {}",
                    model, model_request, e
                ));
            } else {
                self.reconcile_explicit_provider_pin_route();
            }
        } else {
            self.session.model = Some(self.provider_model());
        }
        self.restore_reasoning_effort_from_session();
        let model_ms = model_start.elapsed().as_millis();

        let mark_active_start = Instant::now();
        self.session.mark_active();
        self.begin_concurrency_tracking();
        let mark_active_ms = mark_active_start.elapsed().as_millis();
        self.sync_memory_dedup_state_from_session();

        logging::info(&format!(
            "restore_session: loaded session {} with {} messages, calling seed_compaction",
            session_id,
            self.session.messages.len()
        ));
        let compaction_start = Instant::now();
        self.seed_compaction_from_session();
        let compaction_ms = compaction_start.elapsed().as_millis();

        let env_snapshot_start = Instant::now();
        self.log_env_snapshot("resume");
        let env_snapshot_ms = env_snapshot_start.elapsed().as_millis();
        self.fire_session_lifecycle_hook("session_start", "resume");

        let save_start = Instant::now();
        if let Err(err) = self.session.save() {
            logging::error(&format!(
                "Failed to persist resumed session state for {}: {}",
                session_id, err
            ));
        }
        let save_ms = save_start.elapsed().as_millis();

        logging::info(&format!(
            "[TIMING] restore_session: session={}, messages={}, restored_soft_interrupts={}, load={}ms, assign={}ms, reset={}ms, model={}ms, mark_active={}ms, compaction={}ms, env_snapshot={}ms, save={}ms, total={}ms",
            session_id,
            self.session.messages.len(),
            restored_soft_interrupts,
            load_ms,
            assign_ms,
            reset_ms,
            model_ms,
            mark_active_ms,
            compaction_ms,
            env_snapshot_ms,
            save_ms,
            restore_start.elapsed().as_millis(),
        ));
        logging::info(&format!(
            "Session restored: {} messages in session",
            self.session.messages.len()
        ));
        Ok(previous_status)
    }

    /// Get conversation history for sync
    pub fn get_history(&self) -> Vec<HistoryMessage> {
        crate::session::render_messages(&self.session)
            .into_iter()
            .map(|msg| HistoryMessage {
                response_stats: msg.response_stats,
                role: msg.role,
                content: msg.content,
                tool_calls: if msg.tool_calls.is_empty() {
                    None
                } else {
                    Some(msg.tool_calls)
                },
                tool_data: msg.tool_data,
            })
            .collect()
    }

    pub fn get_history_and_rendered_images(
        &self,
    ) -> (Vec<HistoryMessage>, Vec<crate::session::RenderedImage>) {
        let (messages, images) = crate::session::render_messages_and_images(&self.session);
        let history = messages
            .into_iter()
            .map(|msg| HistoryMessage {
                response_stats: msg.response_stats,
                role: msg.role,
                content: msg.content,
                tool_calls: if msg.tool_calls.is_empty() {
                    None
                } else {
                    Some(msg.tool_calls)
                },
                tool_data: msg.tool_data,
            })
            .collect();
        (history, images)
    }

    pub fn get_history_and_rendered_images_with_compacted_history(
        &self,
        compacted_history_visible: usize,
    ) -> (
        Vec<HistoryMessage>,
        Vec<crate::session::RenderedImage>,
        Option<crate::session::RenderedCompactedHistoryInfo>,
    ) {
        let (messages, images, compacted_info) =
            crate::session::render_messages_and_images_with_compacted_history(
                &self.session,
                compacted_history_visible,
            );
        let history = messages
            .into_iter()
            .map(|msg| HistoryMessage {
                response_stats: msg.response_stats,
                role: msg.role,
                content: msg.content,
                tool_calls: if msg.tool_calls.is_empty() {
                    None
                } else {
                    Some(msg.tool_calls)
                },
                tool_data: msg.tool_data,
            })
            .collect();
        (history, images, compacted_info)
    }

    pub fn get_tool_call_summaries(&self, limit: usize) -> Vec<crate::protocol::ToolCallSummary> {
        crate::session::summarize_tool_calls(&self.session, limit)
    }

    /// Start an interactive REPL
    pub async fn repl(&mut self) -> Result<()> {
        println!("J-Code - Coding Agent");
        println!("Type your message, or 'quit' to exit.");

        // Show available skills
        let skills = self.current_skills_snapshot();
        let skill_list = skills.list();
        if !skill_list.is_empty() {
            println!(
                "Available skills: {}",
                skill_list
                    .iter()
                    .map(|s| format!("/{}", s.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();

        loop {
            print!("> ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            if input == "quit" || input == "exit" {
                break;
            }

            if input == "clear" {
                self.clear();
                println!("Conversation cleared.");
                continue;
            }

            // Check for skill invocation. Resolve against the registry (not
            // the bare tokenizer) so a `SKILL.md` `name:` field containing
            // spaces, e.g. "My Custom Skill", can still be matched: the
            // bare parse always stops at the first whitespace.
            if let Some(invocation) = skills.resolve_invocation(input) {
                if let Some(skill) = skills.get(invocation.name) {
                    println!("Activating skill: {}", skill.name);
                    println!("{}\n", skill.description);
                    self.set_active_skill(invocation.name.to_string());
                    if let Some(prompt) = invocation.prompt {
                        if let Err(e) = self.run_once(prompt).await {
                            eprintln!("\nError: {}\n", e);
                        }
                        println!();
                    }
                    continue;
                } else {
                    println!("Unknown skill: /{}", invocation.name);
                    println!(
                        "Available: {}",
                        skills
                            .list()
                            .iter()
                            .map(|s| format!("/{}", s.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    continue;
                }
            }

            if let Err(e) = self.run_once(input).await {
                eprintln!("\nError: {}\n", e);
            }

            println!();
        }

        // Extract memories from session before exiting
        self.extract_session_memories().await;

        Ok(())
    }

    /// Extract memories from the session transcript
    /// Returns the number of memories extracted, or 0 if none/skipped
    pub async fn extract_session_memories(&self) -> usize {
        if !self.memory_enabled {
            return 0;
        }

        // Need at least 4 messages for meaningful extraction
        if self.session.messages.len() < 4 {
            return 0;
        }

        logging::info(&format!(
            "Extracting memories from {} messages",
            self.session.messages.len()
        ));

        // Build transcript
        let mut transcript = String::new();
        for msg in &self.session.messages {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            transcript.push_str(&format!("**{}:**\n", role));
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text, .. } => {
                        if text.trim_start().starts_with("<system-reminder>") {
                            continue;
                        }
                        transcript.push_str(text);
                        transcript.push('\n');
                    }
                    ContentBlock::ToolUse { name, .. } => {
                        transcript.push_str(&format!("[Used tool: {}]\n", name));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let preview = if content.len() > 200 {
                            format!("{}...", crate::util::truncate_str(content, 200))
                        } else {
                            content.clone()
                        };
                        transcript.push_str(&format!("[Result: {}]\n", preview));
                    }
                    ContentBlock::Reasoning { .. }
                    | ContentBlock::ReasoningTrace { .. }
                    | ContentBlock::AnthropicThinking { .. }
                    | ContentBlock::OpenAIReasoning { .. } => {}
                    ContentBlock::Image { .. } => {
                        transcript.push_str("[Image]\n");
                    }
                    ContentBlock::OpenAICompaction { .. } => {
                        transcript.push_str("[OpenAI native compaction]\n");
                    }
                }
            }
            transcript.push('\n');
        }

        if !crate::memory::memory_llm_judge_available() {
            logging::info("Memory extraction skipped: LLM judge unavailable");
            return 0;
        }

        // Extract using sidecar
        let sidecar = crate::sidecar::Sidecar::new();
        match sidecar.extract_memories(&transcript).await {
            Ok(extracted) if !extracted.is_empty() => {
                let manager = self
                    .session
                    .working_dir
                    .as_deref()
                    .map(|dir| crate::memory::MemoryManager::new().with_project_dir(dir))
                    .unwrap_or_default();
                let mut stored_count = 0;

                for memory in &extracted {
                    let category = crate::memory::MemoryCategory::from_extracted(&memory.category);

                    let trust = match memory.trust.as_str() {
                        "high" => crate::memory::TrustLevel::High,
                        "low" => crate::memory::TrustLevel::Low,
                        _ => crate::memory::TrustLevel::Medium,
                    };

                    let entry = crate::memory::MemoryEntry::new(category, &memory.content)
                        .with_source(&self.session.id)
                        .with_trust(trust);

                    if manager.remember_project(entry).is_ok() {
                        stored_count += 1;
                    }
                }

                if stored_count > 0 {
                    logging::info(&format!("Extracted {} memories from session", stored_count));
                }
                stored_count
            }
            Ok(_) => 0,
            Err(e) => {
                logging::info(&format!("Memory extraction skipped: {}", e));
                0
            }
        }
    }
}

fn append_turn_usage_fields(
    mut event: crate::hooks::HookEvent,
    provider_name: &str,
    totals: crate::protocol::TokenUsageTotals,
    context_window_tokens: u64,
    reasoning_effort: &str,
    service_tier: &str,
) -> crate::hooks::HookEvent {
    if totals.messages_with_token_usage == 0 {
        return event;
    }
    let effective_input = crate::compaction::effective_billed_input_tokens_from_usage(
        provider_name,
        totals.input_tokens,
        Some(totals.cache_read_input_tokens),
        Some(totals.cache_creation_input_tokens),
    );
    event = event
        .field("PROVIDER", provider_name.to_string())
        .field("CONTEXT_WINDOW_TOKENS", context_window_tokens.to_string())
        .field(
            "CONTEXT_UTILIZATION_PCT",
            format!(
                "{:.2}",
                effective_input as f64 * 100.0 / context_window_tokens.max(1) as f64
            ),
        )
        .field("REASONING_EFFORT", reasoning_effort.to_string())
        .field("SERVICE_TIER", service_tier.to_string())
        .field("INPUT_TOKENS", effective_input.to_string())
        .field("RAW_INPUT_TOKENS", totals.input_tokens.to_string())
        .field("OUTPUT_TOKENS", totals.output_tokens.to_string())
        .field(
            "CACHE_READ_INPUT_TOKENS",
            totals.cache_read_input_tokens.to_string(),
        )
        .field(
            "CACHE_CREATION_INPUT_TOKENS",
            totals.cache_creation_input_tokens.to_string(),
        )
        .field(
            "MESSAGES_WITH_TOKEN_USAGE",
            totals.messages_with_token_usage.to_string(),
        );
    event
}

fn openai_pressure_turn(
    provider_name: &str,
    model: String,
    reasoning_effort: String,
    service_tier: String,
    context_window_tokens: u64,
    totals: crate::protocol::TokenUsageTotals,
) -> Option<crate::openai_subscription_pressure::OpenAiPressureTurn> {
    if !provider_name.eq_ignore_ascii_case("openai")
        || totals.messages_with_token_usage == 0
        || context_window_tokens == 0
    {
        return None;
    }
    Some(crate::openai_subscription_pressure::OpenAiPressureTurn {
        model,
        reasoning_effort,
        service_tier,
        calls: totals.messages_with_token_usage as u64,
        effective_input_tokens: crate::compaction::effective_billed_input_tokens_from_usage(
            provider_name,
            totals.input_tokens,
            Some(totals.cache_read_input_tokens),
            Some(totals.cache_creation_input_tokens),
        ),
        raw_input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cache_read_input_tokens: totals.cache_read_input_tokens,
        cache_creation_input_tokens: totals.cache_creation_input_tokens,
        context_window_tokens,
    })
}

#[cfg(test)]
mod turn_usage_hook_tests {
    use super::{append_turn_usage_fields, openai_pressure_turn};
    use crate::protocol::TokenUsageTotals;

    fn field<'a>(event: &'a crate::hooks::HookEvent, key: &str) -> Option<&'a str> {
        event
            .fields
            .iter()
            .find_map(|(name, value)| (*name == key).then_some(value.as_str()))
    }

    #[test]
    fn turn_hook_normalizes_subset_and_split_cache_accounting() {
        let totals = TokenUsageTotals {
            cache_prompt_tokens: None,
            messages_with_token_usage: 1,
            input_tokens: 100,
            output_tokens: 25,
            cache_reported_input_tokens: 100,
            cache_read_input_tokens: 80,
            cache_creation_input_tokens: 0,
        };
        let openai = append_turn_usage_fields(
            crate::hooks::HookEvent::new("turn_end"),
            "openai",
            totals,
            200,
            "max",
            "priority",
        );
        assert_eq!(field(&openai, "INPUT_TOKENS"), Some("100"));
        assert_eq!(field(&openai, "RAW_INPUT_TOKENS"), Some("100"));
        assert_eq!(field(&openai, "PROVIDER"), Some("openai"));
        assert_eq!(field(&openai, "CONTEXT_WINDOW_TOKENS"), Some("200"));
        assert_eq!(field(&openai, "CONTEXT_UTILIZATION_PCT"), Some("50.00"));
        assert_eq!(field(&openai, "REASONING_EFFORT"), Some("max"));
        assert_eq!(field(&openai, "SERVICE_TIER"), Some("priority"));

        let anthropic = append_turn_usage_fields(
            crate::hooks::HookEvent::new("turn_end"),
            "anthropic",
            TokenUsageTotals {
                cache_creation_input_tokens: 2,
                cache_read_input_tokens: 200,
                input_tokens: 50,
                ..totals
            },
            200,
            "medium",
            "standard",
        );
        assert_eq!(field(&anthropic, "INPUT_TOKENS"), Some("252"));
        assert_eq!(field(&anthropic, "RAW_INPUT_TOKENS"), Some("50"));
        assert_eq!(field(&anthropic, "OUTPUT_TOKENS"), Some("25"));
    }

    #[test]
    fn only_openai_turns_produce_pressure_inputs() {
        let totals = TokenUsageTotals {
            messages_with_token_usage: 1,
            input_tokens: 100,
            output_tokens: 25,
            ..Default::default()
        };
        assert!(
            openai_pressure_turn(
                "openai",
                "gpt-5.6".to_string(),
                "max".to_string(),
                "priority".to_string(),
                200,
                totals,
            )
            .is_some()
        );
        assert!(
            openai_pressure_turn(
                "anthropic",
                "claude".to_string(),
                "high".to_string(),
                "standard".to_string(),
                200,
                totals,
            )
            .is_none()
        );
    }
}
