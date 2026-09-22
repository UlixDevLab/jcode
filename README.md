# Jcode Lite Free

A credential-free distribution of [Jcode](https://github.com/1jehuang/jcode), maintained by UlixDevLab. It combines the upstream terminal agent with a curated setup for coding, research, design and management.

## Download and install

Download an artifact for your platform from [Releases](https://github.com/UlixDevLab/jcode/releases). A source ZIP is **not** an installer. Only assets explicitly attached to a published release are release deliverables.

- **Apple Silicon macOS:** extract `JcodeLiteFree-macos-arm64-…zip`, then double-click `install.command`. Node and the MCP packages are bundled. The installer creates `jcodef` and leaves ordinary `jcode`, its settings and its sessions alone.
- **Windows x64:** use the Windows asset when available and double-click `install.cmd`. Platform availability and prerequisites are stated in each release. Do not use an ARM macOS archive on Windows or an Intel Mac.
- Connect your own supported subscription/provider through the normal Jcode onboarding. Public-source builds do not embed Google OAuth application credentials: Gemini/Antigravity OAuth requires the existing client-ID/client-secret environment overrides; other provider login flows are unchanged. Choose the main model yourself. No subscription, API key, private router access or download PIN is included.

The macOS builds are ad-hoc signed, **not Developer ID notarized**. macOS can require its normal Privacy & Security / Open Anyway confirmation after an Internet download. Windows may show SmartScreen or organization-policy warnings. We do not disable these protections or promise zero prompts.

## What this distribution adds

- 35 curated skills for research, first-principles analysis, decisions, design, management and implementation, plus four packaged agent roles.
- Bundled Playwright, Context7, Omnisearch and Reddit MCP packages. Playwright uses an isolated headless session and requires an installed supported browser. Services may require recipient-owned credentials for their optional capabilities.
- Provider-neutral task-based delegation. Pick models in Jcode's agent configuration. Use fast configured models for bounded work and stronger ones where intellectual difficulty or independent review warrants it, not merely because a task has many tokens.
- Native consent integration and preservation of useful local fixes. This is a guardrail, **not an operating-system sandbox** for arbitrary shell/MCP effects.
- Isolated home/runtime, preserved personal skills and rules, package provenance, signed-manifest update verification and rollback-aware macOS installation.
- Upstream v0.86/Jev implementation rather than a separate reimplementation. Live Jev service entitlement and performance depend on the user's setup. Unit-test results are not a production speed benchmark.

On the macOS lifecycle release, run `/update_lite` inside an idle session or `jcodef update` in a terminal. The upstream `/update` remains separate. Updates use the configured signed distribution channel. A child process cannot replace aliases already loaded in a parent shell, so open a new terminal when prompted.

## Sources and provenance

`SOURCE-PROVENANCE.json` pins the upstream version and the integrated source revision used for this export. This repository contains the runtime and Free distribution inputs, not the maintainer's private Lite credentials, personal configuration, mission/session history or private Git history. Some shared packaging helpers retain their historical `distribution/jcode-lite/common` location; the private edition itself is not distributed here.

Build the CLI with Rust stable and `cargo build --release --locked --no-default-features --features pdf,embeddings --bin jcode`. The Free package recipe is `distribution/jcode-lite-free/build.sh`. Native Windows builds run in GitHub Actions, not through cross-compilation on the maintainer's Mac.

Upstream and local work remain attributed in their source copyright notices and `LICENSE`. Contributions are welcome. This is an independent distribution, not an official upstream Jcode release.
