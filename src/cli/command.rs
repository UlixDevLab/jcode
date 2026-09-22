use clap::Subcommand;

use super::args::*;
use super::command_args::{AuthTestCommand, ProviderDoctorCommand, ReplayCommand};
use super::provider_init::ProviderChoice;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Start the agent server (background daemon)
    Serve {
        /// Internal: mark this server as temporary so it can self-clean when its owner exits.
        #[arg(long, hide = true)]
        temporary_server: bool,

        /// Internal: owning process pid for a temporary server.
        #[arg(long, hide = true)]
        owner_pid: Option<u32>,

        /// Internal: idle shutdown timeout in seconds for a temporary server.
        #[arg(long, hide = true)]
        temp_idle_timeout_secs: Option<u64>,

        /// Stable display name for this server in connected clients and session pickers.
        ///
        /// Useful for long-lived remote runtimes, e.g. `fabian`, `john`, or
        /// `mount-cloud-fabian`. Unsafe characters are normalized before use.
        #[arg(long)]
        server_name: Option<String>,
    },

    /// Run as an Agent Client Protocol (ACP) adapter backed by the Jcode daemon
    Acp,

    /// Manage the background server daemon (e.g. `jcode server stop`).
    Server {
        #[command(subcommand)]
        action: ServerCommand,
    },

    /// Connect to a running server
    Connect,

    /// Run a single message and exit
    Run {
        /// Emit a machine-readable JSON result instead of streaming text
        #[arg(long, conflicts_with = "ndjson")]
        json: bool,

        /// Emit newline-delimited JSON events while the response streams
        #[arg(long, conflicts_with = "json")]
        ndjson: bool,

        /// The message to send
        message: String,
    },

    /// Login to a provider via OAuth, API key, or local credentials
    Login {
        /// Provider to log in to. Equivalent to --provider for this command, e.g. `jcode login google`.
        // Distinct clap id: the global `--provider` flag also has id "provider";
        // sharing the id makes clap drop the flag inside `login` (so
        // `jcode login --provider x` errors) and propagate the global default
        // into this positional.
        #[arg(value_enum, id = "login_provider", value_name = "PROVIDER")]
        provider: Option<ProviderChoice>,

        /// Account label for multi-account support (stored labels are auto-numbered)
        #[arg(long, short = 'a')]
        account: Option<String>,

        /// Do not open a local browser. Show a login QR for another device (useful over SSH).
        #[arg(long, alias = "headless")]
        no_browser: bool,

        /// Print a script-friendly auth URL and persist temporary login state for later completion.
        #[arg(long, conflicts_with_all = ["callback_url", "auth_code"])]
        print_auth_url: bool,

        /// Complete a previously printed auth flow using a full callback URL or query string.
        #[arg(long, conflicts_with = "auth_code")]
        callback_url: Option<String>,

        /// Complete a previously printed auth flow using a provider-issued authorization code.
        #[arg(long, conflicts_with = "callback_url")]
        auth_code: Option<String>,

        /// Emit machine-readable JSON for script-friendly login flows.
        #[arg(long)]
        json: bool,

        /// Resume a pending scriptable login flow that does not require callback/code input.
        #[arg(long, conflicts_with_all = ["print_auth_url", "callback_url", "auth_code"])]
        complete: bool,

        /// Isolate temporary login state using 1-64 ASCII letters, digits, underscores or hyphens.
        #[arg(long, value_parser = super::login::parse_login_flow_id)]
        flow_id: Option<String>,

        /// Cancel only this provider's pending flow. Does not remove saved credentials.
        #[arg(long, requires = "flow_id", conflicts_with_all = ["print_auth_url", "callback_url", "auth_code", "complete", "account", "api_base", "api_key", "api_key_env"])]
        cancel: bool,

        /// Save credentials without running the post-login live provider validation.
        /// Useful for offline setup, CI, or when entering credentials before network access is available.
        #[arg(long)]
        no_validate: bool,

        /// Gmail/Google access tier for non-interactive flows. Defaults to full.
        #[arg(long, value_enum)]
        google_access_tier: Option<GoogleAccessTierArg>,

        /// OpenAI-compatible API base URL. Used with --provider openai-compatible/custom profiles.
        #[arg(long)]
        api_base: Option<String>,

        /// OpenAI-compatible API key. If omitted, jcode prompts securely when needed.
        #[arg(long)]
        api_key: Option<String>,

        /// Environment variable name to store/use for an OpenAI-compatible API key.
        #[arg(long)]
        api_key_env: Option<String>,
    },

    /// Log in to and manage your Jcode account
    Account {
        #[command(subcommand)]
        action: AccountCommand,
    },

    /// Run in simple REPL mode (no TUI)
    Repl,

    /// Update jcode to the latest version
    Update,

    /// Show build/version information in human or JSON form
    Version {
        /// Emit JSON instead of plain text
        #[arg(long)]
        json: bool,
    },

    /// Show usage limits for connected providers
    Usage {
        #[arg(long)]
        json: bool,
    },

    /// Report actual consent-gate prompts and decisions from the last seven days
    GateTelemetry,

    /// Inspect or control subscription-backed Claude calls across all sessions
    Claude {
        #[command(subcommand)]
        action: ClaudeCommand,
    },
    /// Inspect or change anonymous telemetry settings
    #[command(subcommand)]
    Telemetry(TelemetryCommand),

    /// Self-development mode: run as a canary session on the shared server
    #[command(alias = "selfdev")]
    SelfDev {
        /// Build and test a new canary version before launching
        #[arg(long)]
        build: bool,
    },

    /// Debug socket CLI - interact with running jcode server
    Debug {
        /// Debug command to run (list, start, sessions, create_session, message, tool, state, history, etc.)
        #[arg(default_value = "help")]
        command: String,

        /// Optional argument for the command
        #[arg(default_value = "")]
        arg: String,

        /// Target a specific session by ID
        #[arg(short = 'S', long)]
        session: Option<String>,

        /// Connect to specific server socket path
        #[arg(short = 's', long)]
        socket: Option<String>,

        /// Wait for response to complete (for message command)
        #[arg(short, long)]
        wait: bool,
    },

    /// Authentication status and validation helpers
    #[command(subcommand)]
    Auth(AuthCommand),

    /// Provider discovery and selection helpers
    #[command(subcommand)]
    Provider(ProviderCommand),

    /// Memory management commands
    #[command(subcommand)]
    Memory(MemoryCommand),

    /// Session management commands
    #[command(subcommand)]
    Session(SessionCommand),

    /// Ambient mode management
    #[command(subcommand)]
    Ambient(AmbientCommand),

    /// Optional Jcode Cloud/Jade integration commands
    #[command(subcommand)]
    Cloud(CloudCommand),

    /// Generate a pairing code for iOS/web client
    Pair {
        /// List paired devices instead of generating a code
        #[arg(long)]
        list: bool,

        /// Revoke a paired device by name or ID
        #[arg(long)]
        revoke: Option<String>,
    },

    /// Review and respond to pending ambient permission requests
    Permissions,

    /// Inject externally transcribed text into the active Jcode TUI
    Transcript {
        /// Transcript text. If omitted, reads from stdin.
        text: Option<String>,

        /// How to apply the transcript inside Jcode
        #[arg(long, value_enum, default_value = "send")]
        mode: TranscriptModeArg,

        /// Target a specific live session instead of the active TUI
        #[arg(short = 'S', long)]
        session: Option<String>,
    },

    /// Run configured dictation: send to last-focused jcode client or type raw text
    Dictate {
        /// Type the transcript into the focused app instead of sending to jcode
        #[arg(long)]
        r#type: bool,
    },

    /// Set up the platform global hotkey to launch jcode
    SetupHotkey {
        /// Internal: run as the macOS hotkey listener process.
        #[arg(long, hide = true)]
        listen_macos_hotkey: bool,

        /// Internal: show a rate-limited shortcut reminder from a CLI SessionStart hook.
        #[arg(long, hide = true, value_name = "CLI")]
        notify_cli_launch: Option<String>,

        /// Internal: run as the Windows hotkey listener process.
        #[arg(long, hide = true)]
        listen_windows_hotkey: bool,

        /// Remove the installed platform global hotkey listener.
        #[arg(long)]
        uninstall: bool,
    },

    /// Install a launcher so jcode appears in your app launcher
    SetupLauncher,

    /// Browser automation setup and status
    Browser {
        /// Action (setup, status)
        #[arg(default_value = "setup")]
        action: String,
    },

    /// Replay a saved session in the TUI
    Replay(ReplayCommand),

    /// Model management commands
    #[command(subcommand)]
    Model(ModelCommand),

    /// Show live verification coverage. With no provider/model, prints the full coverage summary.
    #[command(name = "provider-test-coverage", alias = "model-status")]
    ProviderTestCoverage {
        /// Provider to look up. Omit provider and model to print the full coverage summary.
        #[arg(value_name = "PROVIDER")]
        provider_query: Option<String>,

        /// Model to look up. Defaults to the global --model value only when PROVIDER is supplied.
        #[arg(value_name = "MODEL")]
        model_query: Option<String>,

        /// Read coverage from this JSON file instead of the default live-test coverage ledger
        #[arg(long)]
        coverage_file: Option<String>,

        /// Maximum provider/model pairs to list in the full summary (0 = show all)
        #[arg(long, default_value_t = 0)]
        coverage_limit: usize,
    },

    /// Diagnose why a provider/model or the model picker is broken by walking the
    /// strict end-to-end checkpoints (catalog, picker, model-switch, chat, streaming, tools).
    #[command(name = "provider-doctor", alias = "provider-strict-e2e")]
    ProviderDoctor(ProviderDoctorCommand),

    /// Test authentication end-to-end: login (optional), credential probe, refresh, and provider smoke
    AuthTest(AuthTestCommand),

    /// Save or restore the current set of open jcode windows across a system reboot
    Restart {
        #[command(subcommand)]
        action: RestartCommand,
    },

    /// Show a live macOS menu bar indicator with running/streaming session counts
    #[command(alias = "menu-bar", alias = "statusbar")]
    Menubar {
        /// Print the current counts once as text and exit (no menu bar item)
        #[arg(long)]
        once: bool,

        /// Emit the current counts as JSON and exit
        #[arg(long, conflicts_with = "once")]
        json: bool,
    },

    /// Serve the stable harness API on a Unix socket, for SDK clients.
    ///
    /// This is the endpoint the TypeScript SDK (`@1jehuang/jcode-sdk`) connects to. It
    /// ships in the released binary on purpose: the API is only "generally
    /// available" if reaching it does not require a Rust toolchain and a
    /// source checkout.
    #[cfg(unix)]
    #[command(name = "api-bridge", alias = "api")]
    ApiBridge {
        /// Path of the API socket to listen on (default: $XDG_RUNTIME_DIR/jcode-api.sock)
        ///
        /// Named `--api-socket` rather than `--socket` because the global
        /// `--socket` already selects the *internal daemon* socket, and clap
        /// binds the global first: a subcommand `--socket` silently pointed
        /// both ends of the bridge at the same path.
        #[arg(long = "api-socket")]
        api_socket: Option<String>,

        /// Serve one API connection on stdin/stdout (for SSH SDK clients).
        /// Starts the shared daemon if needed; does not create an API socket.
        #[arg(long, conflicts_with = "api_socket")]
        stdio: bool,
    },
}
