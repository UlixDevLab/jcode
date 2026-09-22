use clap::{Parser, Subcommand, ValueEnum};

use super::provider_init::ProviderChoice;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum TranscriptModeArg {
    Insert,
    Append,
    Replace,
    Send,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum GoogleAccessTierArg {
    Full,
    Readonly,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ProviderAuthArg {
    /// Send the API key as Authorization: Bearer <key> (OpenAI-compatible default)
    Bearer,
    /// Send the API key in an API-key header (defaults to api-key)
    ApiKey,
    /// Do not send authentication, useful for localhost model servers
    None,
}

#[derive(Parser, Debug)]
#[command(name = "jcode")]
#[command(version = jcode_build_meta::version())]
#[command(about = "J-Code: A coding agent using Claude Max or ChatGPT Pro subscriptions")]
pub(crate) struct Args {
    /// Initial provider to use (jcode, claude, openai, openai-api, openrouter, azure, opencode, opencode-go, zai, 302ai, baseten, conifer, cortecs, comtegra, deepseek, fpt, firmware, huggingface, moonshotai, nebius, scaleway, stackit, groq, mistral, perplexity, togetherai, deepinfra, xai, grok-build, nvidia-nim, lmstudio, ollama, chutes, cerebras, alibaba-coding-plan, openai-compatible, cursor, copilot, gemini, antigravity, google, or auto-detect). Interactive sessions can switch providers with /model.
    #[arg(short, long, default_value = "auto", global = true)]
    pub(crate) provider: ProviderChoice,

    /// Working directory for the local client process
    #[arg(short = 'C', long, global = true)]
    pub(crate) cwd: Option<String>,

    /// Working directory to send to a remote server when using --socket
    #[arg(long, global = true)]
    pub(crate) remote_working_dir: Option<String>,

    /// Run the UI locally and attach to the persistent Jcode server on this SSH host
    #[arg(long, global = true, conflicts_with = "socket", value_name = "HOST")]
    pub(crate) ssh: Option<String>,

    /// Remote Jcode executable name or literal path (requires --ssh)
    #[arg(long, global = true, requires = "ssh", value_name = "PATH")]
    pub(crate) ssh_binary: Option<String>,

    /// Remote daemon socket override, for isolated servers (requires --ssh)
    #[arg(long, global = true, requires = "ssh", value_name = "PATH")]
    pub(crate) ssh_server_socket: Option<String>,

    /// Skip the automatic update check
    #[arg(long, global = true)]
    pub(crate) no_update: bool,

    /// Auto-update when new version is available (default: true for release builds)
    #[arg(long, global = true, default_value = "true")]
    pub(crate) auto_update: bool,

    /// Log tool inputs/outputs and token usage to stderr
    #[arg(long, global = true)]
    pub(crate) trace: bool,

    /// Suppress non-error CLI/status output for scripting and wrappers
    #[arg(long, global = true)]
    pub(crate) quiet: bool,

    /// Resume a session by ID, or list sessions if no ID provided
    #[arg(long, global = true, num_args = 0..=1, default_missing_value = "")]
    pub(crate) resume: Option<String>,

    /// Internal: launched as a freshly spawned window, so skip heavy local resume bootstrap.
    #[arg(long, global = true, hide = true)]
    pub(crate) fresh_spawn: bool,

    /// Internal: canonical global hotkey that launched this process.
    #[arg(long, global = true, hide = true, value_name = "CHORD")]
    pub(crate) spawn_hotkey: Option<String>,

    /// Disable auto-detection of jcode repository and self-dev mode
    #[arg(long, global = true)]
    pub(crate) no_selfdev: bool,

    /// Start the onboarding simulator on launch (same as `/onboarding-sim`).
    /// Steps through every first-run onboarding screen with synthetic data;
    /// never touches real auth state.
    #[arg(long = "onboarding-sim")]
    pub(crate) onboarding_sim: bool,

    /// Launch the normal TUI, skip onboarding, then autoplay a safe simulation
    /// of receiving, downloading, installing, and restarting after an update.
    #[arg(long = "update-sim")]
    pub(crate) update_sim: bool,

    /// Custom socket path for server/client communication
    #[arg(long, global = true)]
    pub(crate) socket: Option<String>,

    /// Enable debug socket (broadcasts all TUI state changes)
    #[arg(long, global = true)]
    pub(crate) debug_socket: bool,

    /// Model to use (e.g., claude-opus-4-6, gpt-5.5)
    #[arg(short, long, global = true)]
    pub(crate) model: Option<String>,

    /// Named provider profile from [providers.<name>] in config.toml.
    /// Implies --provider openai-compatible for OpenAI-compatible profiles.
    #[arg(long, global = true)]
    pub(crate) provider_profile: Option<String>,

    /// Tool profile to expose to the model: full, minimal/lite, or none.
    #[arg(long, global = true)]
    pub(crate) tool_profile: Option<String>,

    /// Comma-separated explicit allow-list of tools to expose, e.g. bash,read,write,apply_patch. Use '*' or 'all' for the unrestricted full toolset.
    #[arg(long, global = true)]
    pub(crate) tools: Option<String>,

    /// Comma-separated list of tools to hide after applying the selected profile.
    #[arg(long, global = true)]
    pub(crate) disabled_tools: Option<String>,

    /// Hide all built-in tools unless --tools or [tools].enabled opts tools back in.
    #[arg(long, global = true)]
    pub(crate) disable_base_tools: bool,

    /// MCP tool exposure mode: auto, eager, or deferred.
    #[arg(long, global = true, value_parser = ["auto", "eager", "deferred"])]
    pub(crate) mcp_tools: Option<String>,

    /// Token estimate at which --mcp-tools=auto switches to deferred exposure.
    #[arg(long, global = true, value_name = "TOKENS")]
    pub(crate) mcp_tools_token_threshold: Option<usize>,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

pub(crate) use super::command::Command;
pub(crate) use super::command_args::{AuthTestCommand, ProviderDoctorCommand, ReplayCommand};
#[derive(Subcommand, Debug)]
pub(crate) enum ClaudeCommand {
    /// Show shared Claude subscription monitor state without making a provider call
    Status {
        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,
    },
    /// Allow new subscription-backed Claude calls globally
    On,
    /// Block new subscription-backed Claude calls globally
    Off,
    /// Acknowledge the current warning and start a new monitored baseline
    Approve,
}

#[derive(Subcommand, Debug)]
pub(crate) enum TelemetryCommand {
    /// Show the current telemetry state without creating an anonymous ID
    Status {
        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Enable anonymous usage telemetry
    Enable,
    /// Disable all telemetry persistently
    Disable,
}

#[derive(Subcommand, Debug)]
pub(crate) enum AccountCommand {
    /// Open browser-based device authorization and wait for plan activation
    Login {
        /// Do not open a browser automatically; print the public approval URL instead
        #[arg(long, alias = "headless")]
        no_browser: bool,
    },
    /// Show canonical account, plan, and usage status from /v1/me
    Status {
        /// Emit JSON instead of human-readable output
        #[arg(long)]
        json: bool,
    },
    /// Open the public Jcode account management page
    Manage,
    /// Revoke the current key when reachable, then securely clear local state
    Logout,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServerCommand {
    /// Internal native client protocol bridge over stdin/stdout (for SSH attach)
    #[cfg(unix)]
    #[command(hide = true)]
    Stdio,

    /// Start the background server if it is not already running.
    Start {
        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },

    /// Internal: hold a lightweight connection open until stdin closes.
    #[command(hide = true)]
    Keepalive,

    /// Pin the shared server channel to an installed version.
    ///
    /// Defaults to the active `current` version. This only selects the daemon's
    /// binary; run `jcode server reload` separately to apply it.
    Promote {
        /// Installed version to promote (defaults to the current channel)
        version: Option<String>,

        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },

    /// Gracefully reload the running background server onto the newest binary.
    ///
    /// This is the preferred way to pick up an upgrade: the daemon hands its
    /// live sessions off to a freshly exec'd server (the same path `/reload`
    /// uses), so headless/swarm work is preserved instead of being killed. If
    /// no server is running, this is a no-op. Use `server stop --force` only
    /// when you need to hard-retire a wedged daemon.
    Reload {
        /// Reload even if the running server is already on the newest binary.
        #[arg(long)]
        force: bool,

        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },

    /// Stop the running background server and clear its socket.
    ///
    /// Prefer `server reload` after an upgrade; it preserves live sessions.
    /// `stop` terminates the daemon (SIGTERM, escalating to SIGKILL), which
    /// drops any in-flight headless/swarm sessions, so it requires `--force`
    /// as a deliberate acknowledgement.
    Stop {
        /// Confirm that terminating the daemon (and dropping live sessions) is intended.
        #[arg(long)]
        force: bool,

        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CloudCommand {
    /// Upload, list, verify, and view cloud-synced sessions
    Sessions {
        #[command(subcommand)]
        action: CloudSessionsCommand,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CloudSessionsCommand {
    /// Configure Jade API defaults for cloud sessions on this machine
    Configure {
        /// Jade Session API base URL
        #[arg(long)]
        api_base: Option<String>,

        /// Jade Session API bearer token. Prefer --api-token-env to avoid shell history.
        #[arg(long, conflicts_with = "api_token_env")]
        api_token: Option<String>,

        /// Read the Jade Session API bearer token from this environment variable
        #[arg(long, conflicts_with = "api_token")]
        api_token_env: Option<String>,

        /// Optional Jade token id, e.g. dev-admin
        #[arg(long)]
        api_token_id: Option<String>,

        /// Default Jade user id for commands that do not pass --user-id
        #[arg(long)]
        user_id: Option<String>,

        /// Default private Jade session helper path
        #[arg(long)]
        helper: Option<String>,

        /// Remove the saved cloud sessions config
        #[arg(long)]
        clear: bool,
    },

    /// Show saved Jade API defaults for cloud sessions without printing secrets
    Status {
        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },

    /// Upload a specific local session JSON file to Jade cloud storage
    Upload {
        /// Path to a local Jcode session JSON file
        session_file: String,

        /// Upload without Jade's redaction pass
        #[arg(long)]
        raw: bool,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },

    /// Upload the newest local Jcode session to Jade cloud storage
    UploadLatest {
        /// Directory containing local Jcode session JSON files
        #[arg(long, default_value = "~/.jcode/sessions")]
        sessions_dir: String,

        /// Upload without Jade's redaction pass
        #[arg(long)]
        raw: bool,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },

    /// Sync new or changed local sessions to Jade cloud storage (idempotent; safe to schedule)
    Sync {
        /// Directory containing local Jcode session JSON files (default: ~/.jcode/sessions)
        #[arg(long)]
        sessions_dir: Option<String>,

        /// Only consider sessions modified within this many days (ignored with --all)
        #[arg(long)]
        since_days: Option<u64>,

        /// Sync all matching sessions regardless of age
        #[arg(long)]
        all: bool,

        /// Maximum number of sessions to upload in this run
        #[arg(long, default_value_t = 50)]
        max: usize,

        /// Skip this run if the last sync ran fewer than this many minutes ago (for cron/timers)
        #[arg(long)]
        min_interval_mins: Option<u64>,

        /// Upload without Jade's redaction pass
        #[arg(long)]
        raw: bool,

        /// Show what would be uploaded without uploading or recording state
        #[arg(long)]
        dry_run: bool,

        /// Re-upload sessions even if local sync state says they are unchanged
        #[arg(long)]
        force: bool,

        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },

    /// List cloud-uploaded sessions from the Jade index
    List {
        /// Maximum number of sessions to show
        #[arg(long, default_value_t = 25)]
        limit: usize,

        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },

    /// Verify that cloud metadata and the S3 session blob both exist
    Verify {
        /// Session ID to verify
        session_id: String,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },

    /// Render a local HTML dashboard of cloud-uploaded sessions from the Jade index
    Dashboard {
        /// Maximum number of sessions to include
        #[arg(long, default_value_t = 100)]
        limit: usize,

        /// Write the dashboard HTML to this path (default: a temp file)
        #[arg(long)]
        output: Option<String>,

        /// Open the generated dashboard in the default browser
        #[arg(long)]
        open: bool,

        /// Also download each session and link rows to a local per-session viewer
        #[arg(long)]
        with_view: bool,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },

    /// Download and view a cloud-uploaded session
    View {
        /// Session ID to view
        session_id: String,

        /// Output format
        #[arg(long, default_value = "summary")]
        format: CloudSessionViewFormat,

        /// Write HTML output to this path when --format html is used
        #[arg(long)]
        output: Option<String>,

        /// Open the generated HTML file when --format html is used
        #[arg(long)]
        open: bool,

        #[command(flatten)]
        jade: JadeCloudOptions,
    },
}

#[derive(Parser, Debug, Clone)]
pub(crate) struct JadeCloudOptions {
    /// Jade user id to pass to the dev helper
    #[arg(long, default_value = "dev")]
    pub(crate) user_id: String,

    /// AWS CLI profile used by the private dev Jade helper. If omitted, the helper decides.
    #[arg(long)]
    pub(crate) profile: Option<String>,

    /// AWS region used by the private dev Jade helper. If omitted, the helper decides.
    #[arg(long)]
    pub(crate) region: Option<String>,

    /// Path to the private Jade session helper. Defaults to $JCODE_JADE_SESSIONS_HELPER or ~/jade/scripts/jade_sessions.py.
    #[arg(long)]
    pub(crate) helper: Option<String>,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub(crate) enum CloudSessionViewFormat {
    Summary,
    Json,
    Html,
}

impl CloudSessionViewFormat {
    pub(crate) fn as_arg(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Json => "json",
            Self::Html => "html",
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum RestartCommand {
    /// Save a reboot snapshot of currently active jcode windows
    Save {
        /// Restore this reboot snapshot automatically the next time plain `jcode` starts
        #[arg(long)]
        auto_restore: bool,
    },
    /// Restore the most recently saved reboot snapshot
    Restore,
    /// Show the currently saved reboot snapshot
    Status,
    /// Remove the currently saved reboot snapshot
    Clear,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ModelCommand {
    /// List model names you can pass to -m/--model
    List {
        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,

        /// Show provider/selection summary before the list
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum SessionCommand {
    /// Rename a saved session's human-readable name/title
    Rename {
        /// Session ID or memorable short name, e.g. fox
        session: String,

        /// New session name/title
        #[arg(required_unless_present = "clear")]
        name: Option<String>,

        /// Clear the custom session name/title
        #[arg(long, conflicts_with = "name")]
        clear: bool,

        /// Emit JSON instead of human-readable output
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ProviderCommand {
    /// List provider IDs you can pass to -p/--provider
    List {
        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,
    },

    /// Show the currently requested and resolved provider selection
    Current {
        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,
    },

    /// Add a named OpenAI-compatible API provider profile
    Add {
        /// Profile name used with --provider-profile and config defaults, e.g. my-gateway
        name: String,

        /// OpenAI-compatible API base URL, e.g. https://llm.example.com/v1
        #[arg(long, alias = "api-base")]
        base_url: String,

        /// Default model id for this provider profile
        #[arg(short, long)]
        model: String,

        /// Optional model context window in tokens
        #[arg(long)]
        context_window: Option<usize>,

        /// Environment variable name that contains the API key
        #[arg(long, conflicts_with = "no_api_key")]
        api_key_env: Option<String>,

        /// API key value to store in jcode's private provider env file. Prefer --api-key-stdin for shell history safety.
        #[arg(long, conflicts_with_all = ["api_key_stdin", "no_api_key"])]
        api_key: Option<String>,

        /// Read the API key from stdin and store it in jcode's private provider env file
        #[arg(long, conflicts_with = "no_api_key")]
        api_key_stdin: bool,

        /// Configure the provider with no API key/authentication
        #[arg(long, conflicts_with_all = ["api_key", "api_key_stdin", "api_key_env"])]
        no_api_key: bool,

        /// Authentication style for the API key
        #[arg(long, value_enum)]
        auth: Option<ProviderAuthArg>,

        /// Header name when --auth api-key is used (default: api-key)
        #[arg(long)]
        auth_header: Option<String>,

        /// Private env file name under jcode's app config directory for stored API keys
        #[arg(long)]
        env_file: Option<String>,

        /// Make this profile the startup default provider/model
        #[arg(long, alias = "default")]
        set_default: bool,

        /// Replace an existing profile with the same name
        #[arg(long)]
        overwrite: bool,

        /// Allow provider-routing features for OpenRouter-style gateways
        #[arg(long)]
        provider_routing: bool,

        /// Fetch/list models from the provider's /models endpoint
        #[arg(long)]
        model_catalog: bool,

        /// Emit JSON instead of human-readable setup output
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum AuthCommand {
    /// Import one selected OAuth login from a trusted client (never overwrites a store)
    Import {
        /// Read the private credential envelope from stdin, never from command arguments
        #[arg(long, required = true)]
        stdin: bool,

        /// Emit a secret-free JSON acknowledgement
        #[arg(long)]
        json: bool,
    },
    /// Show configured authentication status for model/tool providers
    Status {
        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,
    },
    /// Diagnose provider auth issues and suggest next steps
    Doctor {
        /// Optional provider id or alias to focus diagnosis on one provider
        #[arg(id = "auth_provider", value_name = "PROVIDER")]
        provider: Option<String>,

        /// Run live post-login validation for configured providers during diagnosis
        #[arg(long)]
        validate: bool,

        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum AmbientCommand {
    /// Show ambient mode status
    Status,
    /// Show recent ambient activity log
    Log,
    /// Manually trigger an ambient cycle
    Trigger,
    /// Stop ambient mode
    Stop,
    /// Run an ambient cycle in a visible TUI (internal, spawned by the ambient runner)
    #[command(hide = true)]
    RunVisible,
}

#[derive(Subcommand, Debug)]
pub(crate) enum MemoryCommand {
    /// List all stored memories
    List {
        /// Filter by scope (project, global, all)
        #[arg(short, long, default_value = "all")]
        scope: String,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
    },

    /// Search memories by query
    Search {
        /// Search query
        query: String,

        /// Use Jev relevance decisions instead of local keyword search (requires Jev access)
        #[arg(short, long)]
        semantic: bool,
    },

    /// Export memories to a JSON file
    Export {
        /// Output file path
        output: String,

        /// Export scope (project, global, all)
        #[arg(short, long, default_value = "all")]
        scope: String,
    },

    /// Import memories from a JSON file
    Import {
        /// Input file path
        input: String,

        /// Import scope (project, global)
        #[arg(short, long, default_value = "project")]
        scope: String,

        /// Overwrite existing memories with same ID
        #[arg(long)]
        overwrite: bool,
    },

    /// Show memory statistics
    Stats,

    /// Clear test memory storage (used by debug sessions)
    ClearTest,
}

#[cfg(test)]
#[path = "args/help_tests.rs"]
mod help_tests;
#[cfg(test)]
mod tests;
