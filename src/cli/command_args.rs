use clap::Args;

#[derive(Args, Debug)]
pub(crate) struct ReplayCommand {
    /// Session ID, name, or path to session JSON file
    pub(crate) session: String,

    /// Replay related swarm sessions together in a synchronized multi-pane view
    #[arg(long)]
    pub(crate) swarm: bool,

    /// Export timeline as JSON instead of playing
    #[arg(long)]
    pub(crate) export: bool,

    /// Playback speed multiplier (default: 1.0)
    #[arg(long, default_value = "1.0")]
    pub(crate) speed: f64,

    /// Path to an edited timeline JSON file (overrides session timing)
    #[arg(long)]
    pub(crate) timeline: Option<String>,

    /// Auto-edit timeline: compress tool call wait times and gaps between prompts
    #[arg(long)]
    pub(crate) auto_edit: bool,

    /// Export as video file (auto-generates name if no path given)
    #[arg(long, default_missing_value = "auto", num_args = 0..=1)]
    pub(crate) video: Option<String>,

    /// Video width in columns (default: 120)
    #[arg(long, default_value = "120")]
    pub(crate) cols: u16,

    /// Video height in rows (default: 40)
    #[arg(long, default_value = "40")]
    pub(crate) rows: u16,

    /// Video frames per second (default: 60)
    #[arg(long, default_value = "60")]
    pub(crate) fps: u32,

    /// Force centered layout (overrides config)
    #[arg(long, conflicts_with = "no_centered")]
    pub(crate) centered: bool,

    /// Force left-aligned (non-centered) layout (overrides config)
    #[arg(long, conflicts_with = "centered")]
    pub(crate) no_centered: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ProviderDoctorCommand {
    /// OpenAI-compatible provider id to diagnose (e.g. cerebras, fpt, nvidia-nim)
    #[arg(id = "doctor_provider", value_name = "PROVIDER")]
    pub(crate) provider: String,

    /// How much to exercise: offline (no key/no spend), catalog (key, ~no spend),
    /// or full (key, spends balance: chat + streaming + tools).
    #[arg(long, value_name = "TIER", default_value = "catalog")]
    pub(crate) tier: String,

    /// Emit the report as JSON for scripting
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct AuthTestCommand {
    /// Run the provider login flow before validation (interactive/browser-based)
    #[arg(long)]
    pub(crate) login: bool,

    /// Test all currently configured supported auth providers instead of just --provider
    #[arg(long)]
    pub(crate) all_configured: bool,

    /// Skip the provider runtime smoke prompt
    #[arg(long)]
    pub(crate) no_smoke: bool,

    /// Skip the tool-enabled runtime smoke prompt (the same request path used during normal chat)
    #[arg(long)]
    pub(crate) no_tool_smoke: bool,

    /// Custom smoke prompt (default asks for AUTH_TEST_OK)
    #[arg(long)]
    pub(crate) prompt: Option<String>,

    /// Emit JSON report instead of human-readable output
    #[arg(long)]
    pub(crate) json: bool,

    /// Write the full auth-test report JSON to a file
    #[arg(long)]
    pub(crate) output: Option<String>,

    /// Show strict live provider/model E2E coverage instead of running auth tests
    #[arg(long, conflicts_with_all = ["login", "all_configured", "no_smoke", "no_tool_smoke", "prompt"])]
    pub(crate) coverage: bool,

    /// Fetch live model catalogs and verify context-window resolution for each model with metadata
    #[arg(long, conflicts_with_all = ["login", "no_smoke", "no_tool_smoke", "prompt", "coverage"])]
    pub(crate) context_audit: bool,

    /// Read coverage from this JSON file instead of the default live-test coverage ledger
    #[arg(long, requires = "coverage")]
    pub(crate) coverage_file: Option<String>,

    /// Maximum uncovered provider/model gaps to show in the text coverage report
    #[arg(long, requires = "coverage", default_value_t = 50)]
    pub(crate) coverage_limit: usize,
}
