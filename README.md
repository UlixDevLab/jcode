# Jcode Lite Free: an AI assistant for everyday work

**Describe your task in plain language. Jcode can help you understand it, make a plan, and work with files and tools on your computer.**

This **UlixDevLab** distribution is not just for programmers. It is designed to be useful to managers, designers, researchers, business owners, and people who are just getting started with AI.

**[Download the ready-to-install release →](https://github.com/UlixDevLab/jcode/releases/tag/lite-free-v0.3.0-beta.1)** · [Report a problem](https://github.com/UlixDevLab/jcode/issues) · [Original Jcode](https://github.com/1jehuang/jcode) · [Website: Ukrainian / English](https://files.legrin-tech.net/jcode-lite-free/)

> This is a beta release, so you may encounter bugs. Start with a copy of a non-critical folder. The software is free, but **you connect your own AI account or provider**. You do not need to send your keys or passwords to the distribution's author.

## What is Jcode, in plain language?

A regular AI chat mostly replies with messages. **Jcode is an agent**: in addition to chatting, it can read and edit files, use connected tools, research questions, and carry out tasks with multiple steps.

Jcode runs in a terminal: a text-based window rather than a conventional website. **You do not need to know programming commands to talk to it.** You can write, for example: “This folder contains project materials. Prepare a short overview and a list of open questions.”

The AI model is the assistant's “brain.” Jcode provides its workspace, context, and available tools. You choose the model, so quality, speed, usage limits, and cost also depend on your provider.

## How is Lite Free different from the original Jcode?

We are not creating a separate AI or claiming upstream features as our own. The foundation is **upstream Jcode v0.86**, with compatible local improvements and a ready-to-use collection of working tools.

| What you get | Where it comes from and why it matters |
| --- | --- |
| Conversations with models, file operations, sessions, and subagents | Features of Jcode itself. A subagent is a separate assistant working on part of a task. |
| **35 selected skills** | Ready-made approaches to research, design, analysis, decisions, and management. You do not have to assemble the collection yourself. |
| **4 prepared agent roles** | Working instructions for specialized parts of a task. These are not four separate paid accounts. |
| **Bundled MCP tools** | Connections for browser automation, documentation, and search. The tool packages are included in the distribution. |
| **Guidance for distributing work between models** | Simple, clearly bounded subtasks can go to faster models, while difficult decisions and independent checks can use stronger models. Only models you have configured are available. |
| **Memory and working context** | Prepared memory settings and Knowledge OS tools for organizing knowledge. Jcode can use context from earlier work instead of starting from scratch every time. |
| **An isolated installation that preserves your settings** | Lite Free has its own data folder. Updates account for your custom skills and edited rules. This distribution is not intended to replace an ordinary Jcode installation. |
| **Verified updates and local fixes** | Package provenance and checksums are checked. On macOS, client and server updates are coordinated, with session resume support. |

**Free does not include** the author's private router, an Astra subscription, someone else's API keys, the private friends-only PIN, or the author's personal history. Private Lite for friends is a separate distribution. Here, you get a free collection of approaches and tools and connect your own accounts.

### Will it make AI usage cheaper?

The distribution provides guidance for economical delegation, but it does not guarantee a particular percentage of savings. If you connect only one model, other models do not become available automatically. More subagents can also mean more requests.

You choose the main model. A stronger model should be used because of **reasoning difficulty, the consequences of a decision, or the need for verification**, not simply because a text is long. Monitor limits and spending in your provider account.

## What you need

1. **A supported computer.** An Apple Silicon Mac, such as M1/M2/M3/M4 or a newer compatible model, or a Windows x64 PC. The release lists the available packages. The ARM Mac package does not work on Intel Macs or Windows.
2. **Internet access and your own AI access.** For example, a supported account login or an API key. Subscription terms vary: paying for a chat product does not automatically grant access through every API.
3. **A little time for the first login.** Once an account is connected, you should not need to log in every time.

Node.js and MCP packages are included in the complete published installers. You do not need to install Rust, Git, or a programming environment to use them. Browser tasks require a supported browser, such as Chrome or Edge. The browser itself is not included in the ZIP. Some search services require your own additional keys.

## How to install

### 1. Download the installer, not the source code

Open the **[release page](https://github.com/UlixDevLab/jcode/releases/tag/lite-free-v0.3.0-beta.1)** and expand **Assets**.

- Apple Silicon Mac: [download the 0.3.0-beta.1 installer](https://github.com/UlixDevLab/jcode/releases/download/lite-free-v0.3.0-beta.1/JcodeLiteFree-macos-arm64-0.3.0-beta.1.zip).
- Windows x64: [download the 0.3.0-beta.1 installer](https://github.com/UlixDevLab/jcode/releases/download/lite-free-v0.3.0-beta.1/JcodeLiteFree-windows-x64-0.3.0-beta.1.zip).
- **Do not choose `Source code (zip)` or `Source code (tar.gz)`.** Those contain developer source code, not a ready-to-run application.

### 2. Extract the archive and open the installer

**Mac:** double-click the ZIP, open the extracted folder, then double-click **`install.command`**.

**Windows:** right-click the ZIP, choose **Extract All**, open the extracted folder, then double-click **`install.cmd`**. Do not run it from inside the ZIP.

Read the messages in the installer window. After installation, you can start the application by entering **`jcodef`** in a new terminal window. The package also includes a launcher: `jcode-free.command` on Mac or `jcode-free.cmd` on Windows.

### Why does the operating system show a warning?

The Mac build currently has an ad-hoc technical signature, but **it is not Apple Developer ID notarized**. The Windows build also lacks a trusted commercial code-signing signature. macOS Gatekeeper or Windows SmartScreen may therefore ask for confirmation or block execution according to your computer's policies.

Check that you downloaded the file from our release. On your own Mac, use Apple's standard **System Settings → Privacy & Security → Open Anyway** path if it is available for that file. On a work computer, contact your administrator. **Do not disable system protection, change the system-wide PowerShell policy, or bypass workplace restrictions to install this software.**

An HTTPS certificate protects the download connection; it does not replace application signing. We cannot promise “no permission prompts on every computer.”

### 3. Connect a model

After starting Jcode, follow its initial setup or enter:

```text
/login
```

Choose a provider and complete the offered login process. If you use an API key, enter it only in the appropriate field, not in a public message or screenshot.

Then choose a model:

```text
/model
```

One provider is enough to get started. You can connect others later. **Astra is available only if your particular account and provider support and authorize it.**

A technical limitation of builds made from the public source: Google Gemini/Antigravity OAuth application credentials are not embedded. Those login methods require your own client ID/secret configuration through the supported environment variables. Beginners may find another supported login method easier. This limitation does not apply to every provider.

## First tasks you can copy

**Understand your materials**
> The “Project” folder contains documents. Do not change anything yet. Explain what this project is about, what has already been decided, and what information is missing.

**For a manager**
> Turn these meeting notes into a list of decisions, tasks, owners, and open questions. Do not invent people or deadlines that are not in the notes.

**For a designer**
> Review this page description. Suggest three different structures, explain their trade-offs, and check whether the main action is clear to a visitor. Do not edit files yet.

**For research**
> Compare these three products for my use case. Separate verified facts with sources from assumptions. If information is missing, say so.

**For a difficult decision**
> Help me evaluate this idea from the customer's, financial, and risk perspectives. Clarify the goal first. Finish with no more than three next actions.

**For learning**
> This is my first time using an AI agent. Explain the next step in plain language and do not install anything without explaining it first.

A useful request includes **the goal, materials, constraints, and desired result**. For example: “Use these files to prepare a one-page summary in English. Do not use external services or modify the originals.”

## What are skills, roles, and MCP?

A **skill** is a prepared approach to a type of work. Included skills cover research, first-principles reasoning, decision boards, risk review, finance, frontend design, landing pages, and operational diagnosis. Examples include `/research`, `/first-principles`, `/decision-board`, `/skeptic`, `/cfo`, `/frontend-design`, `/landing-page`, and `/ops-diagnosis`. Start typing `/` to discover available commands. For example:

```text
/decision-board Should we launch this service? Here is the context...
```

An **agent role** is a set of instructions for an assistant handling one part of a task. Delegation is not always useful. A short question may only need a direct answer.

**MCP** is a way to connect external tools to AI. This distribution includes packages for:

- **Playwright:** working with web pages through a browser.
- **Context7:** finding current technical documentation.
- **Omnisearch:** connecting search services.
- **Reddit:** working with Reddit material and discussions.

A bundled tool does not mean free access to every service. Some features need a browser, a network connection, permissions, or your own keys. Do not send other people's personal data to external services without an appropriate basis.

## Memory, privacy, and permissions

Sessions, settings, and memory are stored in a separate Lite Free folder. Your personal data is not included in the downloaded ZIP or published in this repository:

- Mac: `~/Library/Application Support/LeGrin/JcodeLiteFree/home`
- Windows: `%LOCALAPPDATA%\LeGrin\JcodeLiteFree\home`

**Local storage does not mean fully offline operation.** The model receives request context to produce an answer, and tools may contact external services. Jev and other remote features have their own access and data-processing terms. Do not add secrets to a task unnecessarily.

Jcode has permission mechanisms for protected operations, but it is **not an operating-system sandbox that makes every dangerous action technically impossible**. Carefully review commands, message sending, file deletion, and configuration changes. AI can make mistakes. Keep backups of important data.

## How to update

**On Mac:** when your session is idle and no important task is running, enter **`/update_lite`**. Alternatively, open a terminal and enter **`jcodef update`**. This is our distribution's command, not the ordinary upstream `/update`.

The updater checks the metadata signature and package checksum, updates the client and its separate server, preserves your data, and attempts to resume your session. If it reports an error, keep the exact message. If the launch path changes, open a new terminal: a child process cannot change aliases already loaded in its parent shell.

**On Windows:** close your Jcode Lite Free work sessions, download the new Windows ZIP from the release, and run `install.cmd`. The package also includes `update.ps1` for the verified update channel. We do not yet claim macOS-equivalent automatic server replacement and session resume on Windows.

Custom skills and edited rules are intended to survive updates. Still, back up your `home` folder before an important update. Do not delete it just to perform a “clean reinstall.”

## Troubleshooting

| What you see | What to do |
| --- | --- |
| The ZIP contains only source code and no installer | You downloaded Source code. Return to Assets on the release page. |
| “No provider configured” | Complete `/login`, then choose `/model`. |
| A model is missing or “not allowed” | Check access with your particular provider. Installing the application does not grant model access. |
| “Rate limit” or “quota” | Check the provider's limits or balance. Do not enable an unfamiliar paid fallback route. |
| The window closes immediately | Open a terminal yourself and run `jcodef` so you can read the error. |
| `jcodef` is not found | Open a new terminal window after installation. Try the launcher included in the package. |
| A browser task will not start | Check that a supported Chrome/Edge browser is installed and read the tool's exact error. |
| OS protection or workplace policy blocks startup | Use the standard permitted OS process or contact your administrator. Do not disable protection. |

When [reporting a problem](https://github.com/UlixDevLab/jcode/issues/new), include your platform, distribution version, what you did, what you expected, and the exact error. **Remove keys, tokens, passwords, private documents, and personal data from screenshots and logs.**

## Jev and performance

This distribution uses the Jev implementation from upstream Jcode, not an independently rebuilt alternative. Having the code does not guarantee that your provider offers the necessary service or subscription. We do not publish unverified “X times faster” claims: local tests are not a substitute for measuring real tasks with an available service.

## For readers who want the source code

This is an independent distribution, not an official release by Jcode's authors. Upstream attribution and licenses are preserved in [LICENSE](LICENSE) and the source files. The initial export's provenance is recorded in [SOURCE-PROVENANCE.json](SOURCE-PROVENANCE.json). Each binary package's exact version is recorded in its metadata and release notes.

**Builds run manually only on the owner's Mac and home Windows PC. GitHub Actions are disabled in this repository. Commits and pull requests do not trigger builds.** GitHub is used for source code, discussion, and downloadable releases. Technical details: [native builds](docs/NATIVE_BUILDS.md).
