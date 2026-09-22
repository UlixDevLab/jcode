# Jcode Lite Free distribution

Jcode Lite Free is an isolated, credential-free Jcode package. It ships the
current optimized runtime, the accepted Lite skill set, Mage, and Knowledge OS,
but no provider profile, API key, OAuth token, or default model.

The first launch opens standard Jcode provider onboarding. Credentials added by
the recipient are stored only in the isolated `JcodeLiteFree/home` directory.
The global command is `jcodef`; normal `jcode`, private `jcodel`, and `~/.jcode`
are not changed.

The Apple Silicon macOS package bundles the same checksum-pinned Node runtime
as Lite. Install, launch, MCP configuration merge and update use that runtime,
so recipients do not need a separate Node.js, npm or Python installation.
Windows prerequisites remain documented separately until native Windows
packaging is validated.

## Session storage and removal

Free sets `JCODE_HOME` to its isolated `home` directory, so native sessions are
stored under `home/sessions` beside memory, provider logins, and configuration:

- macOS: `~/Library/Application Support/LeGrin/JcodeLiteFree/home`
- Windows: `%LOCALAPPDATA%\LeGrin\JcodeLiteFree\home`

`jcodef remove` and the platform removal scripts preserve `home` by default.
Use `jcodef remove --purge-data` or pass `--purge-data` to the platform removal
script only when sessions and all other isolated user data should be deleted.

## Managed skills and roles

Install and reinstall copy the packaged skills, roles, provider-neutral swarm
prompt, and Knowledge OS templates into the isolated Free home. Unchanged
package-managed files advance on upgrade. Recipient-created skills and edited
roles are preserved. Conflicting incoming versions are available under
`home/.asset-updates`, with a summary in `home/.managed-assets-report.json`.
Only unchanged files previously recorded as managed assets may be retired.

The installer does not remove macOS quarantine or replace publisher signatures.
Without Developer ID signing and notarization, macOS may require explicit user
confirmation. Ad-hoc signing is not a substitute for trusted distribution signing.

Build from an already verified native macOS or Windows x64 binary:

```bash
./build.sh --platform macos --binary <jcode-arm64> --output <macos-zip>
./build.sh --platform windows --binary <jcode.exe> --output <windows-zip>
```

For an offline macOS build, pass `--node-runtime <verified-node-tar.gz>`.
The builder checks it against `../jcode-lite/common/node-runtime.json` before
extraction. Without this option it downloads that exact official Node archive.

Build the public static source:

```bash
python3 site/build-site.py \
  --macos <macos-zip> \
  --windows <windows-zip> \
  --signing-key ~/.jcode/keys/jcode-lite-manifest-ed25519.pem \
  --output <site-root>
```

The public release prefix is `https://files.legrin-tech.net/jcode-lite-free/`.
The generated platform manifests are Ed25519-signed and carry explicit channel,
compatibility, SHA-256, and byte-size metadata. Free packages reuse the pinned
public trust root from Jcode Lite; the private signing key remains owner-only and
outside the repository. The updater rejects signature, channel, compatibility,
size, hash, and downgrade violations before installation.

Installed launchers automatically check the signed manifest at most once per 24
hours and print the exact update command when a newer compatible version exists.
Startup never installs or replaces files. Set `JCODE_LITE_FREE_UPDATE_CHECK=0`
to disable the check or `JCODE_LITE_FREE_UPDATE_CHECK_INTERVAL_HOURS` to change
its cadence.
