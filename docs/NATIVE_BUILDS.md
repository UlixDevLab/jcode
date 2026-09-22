# Native release builds (maintainers)

## Policy

GitHub is source and release storage, **not a build service** for this distribution. Repository Actions are disabled, and the former hosted Windows workflow was removed. Pushes, pull requests, tags and documentation edits must not start build jobs. Do not add self-hosted runners with push/PR triggers either: the approved path is an explicit operator-run build on the maintainer's Apple Silicon Mac or home Windows x64 PC.

Recipients do not need any of these tools. They download a release ZIP and use its installer. See the main [beginner guide](../README.md).

## Pin the inputs

Use a clean checkout at a recorded commit. Keep `Cargo.lock`, MCP lockfiles, `.gitattributes`, update trust and renderer reference hashes unchanged. Record the full source commit, the binary's `version --json`, its SHA-256, the final ZIP SHA-256 and validation logs. By default, the build recipe requires the binary revision to match its checkout. For packaging-only changes, `--binary-source-commit COMMIT` permits reusing an executable from that ancestor only after the clean-checkout verifier confirms that no runtime, Cargo or build inputs changed. The package records binary and packaging commits separately. Never relabel an older executable as a new build.

Keep source checkouts separate from shared caches. Reuse a configured Cargo target directory and package caches, with **one active writer per Cargo target**. Do not delete an existing checkout or cache to start a new build. Packaging-only retries should reuse the matching executable. Do not mutate an already published archive.

## macOS arm64

Build natively on Apple Silicon using the repository's coordinated Cargo path when available:

```bash
scripts/dev_cargo.sh build --profile release-lto --locked --no-default-features --features pdf,embeddings --bin jcode
```

Assemble from that same clean source checkout:

```bash
distribution/jcode-lite-free/build.sh --platform macos \
  --binary /absolute/path/to/target/release-lto/jcode \
  --output /absolute/path/to/JcodeLiteFree-macos-arm64-VERSION.zip
python3 distribution/jcode-lite-free/tests/verify-package.py /absolute/path/to/JcodeLiteFree-macos-arm64-VERSION.zip
```

The Mac package builder includes the pinned official Node runtime and ad-hoc signs native binaries. Ad-hoc signing is not Apple Developer ID notarization. Do not claim Gatekeeper acceptance without testing an actual downloaded artifact on the relevant recipient system.

## Windows x64

Run on the approved home Windows machine, not a Mac cross-compiler. Builder prerequisites: Rust stable, Git for Windows including Git Bash, Python 3.11+, Node/npm for assembling the package, and Visual Studio C++ Build Tools **including the Windows SDK**. Having `link.exe` alone is insufficient: missing `kernel32.lib` means the SDK or its environment is unavailable.

Open the existing x64 Visual Studio developer environment, then:

```powershell
cargo build --profile release-lto --locked --no-default-features --features pdf,embeddings --bin jcode
# Check the exit code before continuing.
& "$env:CARGO_TARGET_DIR/release-lto/jcode.exe" --no-update version --json
```

If `CARGO_TARGET_DIR` is not set, the executable is in the checkout's `target/release-lto/`. Set it explicitly when reusing the builder's existing shared cache. Do not run two builds against it concurrently.

In Git Bash, from that same source checkout (verify that `python3 --version` resolves to the real installed Python, not the Microsoft Store alias):

```bash
set -euo pipefail
artifact_dir="$(cd .. && pwd)/release-artifacts"
mkdir -p "$artifact_dir"
distribution/jcode-lite-free/build.sh --platform windows \
  --binary /absolute/path/to/target/release-lto/jcode.exe \
  --output "$artifact_dir/JcodeLiteFree-windows-x64-VERSION.zip"
python distribution/jcode-lite-free/support/bundle-windows-artifact.py \
  "$artifact_dir/JcodeLiteFree-windows-x64-VERSION.zip"
python distribution/jcode-lite-free/tests/verify-package.py \
  "$artifact_dir/JcodeLiteFree-windows-x64-VERSION.zip"
```

Keep output outside the source checkout so the packaging-only reuse check remains clean. When operating remotely, upload a script and run it with PowerShell `-File` or Bash rather than passing a long nested command string. This avoids Windows command-length limits and loss of argument quoting.

The second step is essential: it adds the checksum-pinned official Windows Node distribution so recipients do not need Node installed separately. The builder has a Python ZIP fallback and Python checksum reporting, so Unix `zip` and `shasum` are not required. Preserve exact renderer bytes on checkout; `test_renderer_checkout.py` verifies this against independent pinned values.

Remote operation must use the existing host configuration, strict SSH host checking, finite connection/command timeouts and retained logs. Do not publish private host credentials or add a generic public remote-admin script. Do not restart the PC, install an entire new toolchain or change its security settings as a shortcut. A missing SDK component should be diagnosed and installed only through the authorized official installer path.

## Acceptance before publishing

1. Run the public package verifier on the exact final ZIP. Confirm no private router key, PIN, recipient state or personal configuration is included.
2. On the native platform, extract the final artifact into a fresh isolated test directory. Point installer HOME/USERPROFILE/LOCALAPPDATA at that case, disable recipient PATH changes and automatic interactive launch through the installer test flags.
3. Execute the **real installer**, then the installed launcher, bundled Node and npm. Verify installed executable SHA against the archive member. This is not replaceable by a stub binary.
4. Create a session marker, custom skill and edited managed role; reinstall sequentially and verify all survive. Confirm the ordinary Jcode home was not created or modified. Windows server handoff parity with macOS is not currently promised: close relevant Windows sessions before updating.
5. For macOS lifecycle changes, also exercise old-daemon retirement, same-session resume, same-version replacement, failed-start rollback and survival of an unrelated daemon.
6. Upload only accepted artifacts to GitHub Releases. Keep existing Mac ZIP bytes and release tag unchanged when adding a later Windows artifact. State each platform's actual runtime/source identity in the release notes.
7. Download the hosted artifact and compare its SHA-256 to the accepted original. Website updates additionally require a signed manifest with the actual archive's identity, the pinned verifier, bounded backup/rollback and preservation of unrelated assets. No service restart is needed for an asset-only update.

A successful package check is not proof of model entitlement, paid inference, Jev performance, notarization or every recipient's organization policy. Report those boundaries explicitly.
