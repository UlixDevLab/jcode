# Jcode Lite Free — start here

Jcode Lite Free is an AI coding assistant that runs in your terminal.

Unlike the paid Lite build, **Free comes with no AI provider included**. You
connect your own. What Free gives you is everything around the model: ready-made
workflows, preinstalled tools, skills, and specialist agents.

If you get stuck at any point, email **daniil.kovaliov@gmail.com**.

---

## What you need first

**1. Runtime prerequisites.**

On Apple Silicon macOS, Node.js and npm are included. You do not need to
install Node.js, Python or developer tools separately.

For the current Windows package, install **Node.js version 20 or newer** first.

Check by opening a terminal and typing:

```
node --version
```

If you see `v20` or higher you are fine. Otherwise install the "LTS" version
from <https://nodejs.org>, then close and reopen your terminal.

**2. An AI account of your own.** Any one of these works:

- **Anthropic Claude** — a Claude subscription, or an API key
- **OpenAI / ChatGPT** — a subscription, or an API key
- **GitHub Copilot** — an existing Copilot subscription
- **Google Gemini**, **OpenRouter**, **MiniMax**, and others

You need exactly one to start. You can add more later.

---

## Install

### macOS

1. Unzip the downloaded file (double-click it).
2. Open the folder it creates.
3. Double-click **`install.command`**.
4. If macOS says *"unidentified developer"*: right-click the file, choose
   **Open**, then **Open** again. Once only.

### Windows

1. Right-click the ZIP, choose **Extract All**, pick a folder.
2. Open that folder.
3. Double-click **`install.cmd`**.
4. If SmartScreen shows a blue warning: **More info** → **Run anyway**.

---

## Connect your AI account (one time)

Start it:

- **macOS:** double-click `jcode-free.command`
- **Windows:** double-click `jcode-free.cmd`

Then type:

```
/login
```

Pick your provider from the list and follow the prompts. For subscription
logins a browser window opens; sign in and come back. For an API key, paste it
when asked.

Then choose a model:

```
/model
```

That is the whole setup. It is remembered, so you only do it once.

---

## Run it

Start it the same way (`jcode-free.command` / `jcode-free.cmd`), then type what
you want in plain language:

> read the files in this folder and tell me what this project does

Run it from inside your project folder, or tell it the path.

---

## If something goes wrong

**"requires Node.js 20 or newer"**
Install the LTS version from <https://nodejs.org>, then reopen your terminal.

**"No provider configured" / it refuses to answer**
You have not connected an account yet, or the login expired. Type `/login` and
pick your provider again.

**"Rate limited" / "usage limit reached" / "quota"**
Your own account is out of capacity for now. Either wait for it to reset, or
type `/model` and switch to another provider you have connected.

**"Invalid API key" / "authentication failed"**
The key was mistyped, revoked, or expired. Run `/login` again and re-enter it.

**"zsh: killed" when you run it**
macOS blocked the app because it was downloaded from the internet. Open Terminal
and run this once, then try again:

```
xattr -dr com.apple.quarantine ~/Library/Application\ Support/LeGrin/JcodeLiteFree
```

Reinstalling with the current installer fixes this permanently.

**The window opens and closes immediately**
Open a terminal first, then drag `jcode-free` (macOS) or `jcode-free.ps1`
(Windows) into it and press Enter, so you can read the error.

**macOS: "unidentified developer"**
Right-click → **Open** → **Open**.

**Windows: SmartScreen warning**
**More info** → **Run anyway**.

**It answers, but poorly**
Give it more context: name the file or folder, paste the exact error text, and
say what you expected instead.

**Anything else**
Email **daniil.kovaliov@gmail.com** with what you did, what you expected, and
the exact message (screenshot is fine).

---

## Updating and removing

- **Update:** `update.command` (macOS) / `update.ps1` (Windows)
- **Roll back:** `rollback.command` / `rollback.ps1`
- **Remove the app but keep your work:** run `jcodef remove`, or double-click
  `remove.command` (macOS) / `remove.cmd` (Windows). This keeps sessions,
  memory, provider logins, and settings for a future reinstall.
- **Remove everything permanently:** run `jcodef remove --purge-data`, or run
  the platform removal script with `--purge-data`. You must confirm the purge.

Your isolated data is stored here, with sessions below `home/sessions`:

- macOS: `~/Library/Application Support/LeGrin/JcodeLiteFree/home`
- Windows: `%LOCALAPPDATA%\LeGrin\JcodeLiteFree\home`

Do not manually delete the whole Jcode Lite Free folder unless you intend to
delete that data too.
