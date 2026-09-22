# Vendored MCP packages for Jcode Lite / Jcode Lite Free

This directory holds the shared, deterministic MCP vendor used by both
Jcode Lite (Stables) and Jcode Lite Free (provider-neutral). It is
populated by `npm ci --omit=dev --ignore-scripts --no-audit --no-fund`
from `package.json` and `package-lock.json`. The build script enforces
this step and refuses to produce an archive without a populated
`node_modules`.

## Pinned packages

| Server    | Package                    | Version  | License    |
| --------- | -------------------------- | -------- | ---------- |
| playwright | `@playwright/mcp`         | 0.0.79   | Apache-2.0 |
| context7   | `@upstash/context7-mcp`   | 4.0.2    | MIT        |
| omnisearch | `mcp-omnisearch`          | 0.0.28   | MIT        |
| reddit     | `reddit-mcp-buddy`        | 1.1.14   | MIT        |

The vendored binary is invoked through Node wrappers under `bin/` so the
package stays hermetic and the verifier can enforce:

* **playwright** runs with `--headless --isolated`; no browser cache is
  written; if no system Chrome or Edge is installed, the wrapper exits
  with an actionable message rather than downloading a browser at runtime.
* **context7 / omnisearch** honor optional user-supplied API keys
  (`CONTEXT7_API_KEY`, `OMNISEARCH_API_KEY`) without embedding any
  credentials in the package. Update and rollback flows preserve any
  user-supplied values.
* **reddit** ships in anonymous mode (public RSS for subreddit browsing)
  by default. Search, comment threads, and user analysis require the
  user to set `REDDIT_CLIENT_ID` and `REDDIT_CLIENT_SECRET`. Site copy
  must describe the anonymous limitations honestly; this package owns
  distribution but not the production site directory, so coordinate the
  site copy update with the site owner.

## mcp.json merge policy

On install, the recipient's isolated `mcp.json` is preserved. Missing
bundled server entries are merged in idempotently. Any user-supplied
server entries or env values are never overwritten, and the package
never embeds API keys.

## Updating a pinned version

1. Bump the version in `manifest.json` and `package.json`.
2. Run `npm install --package-lock-only` here to refresh
   `package-lock.json` with real integrity hashes.
3. Run `npm ci --omit=dev --ignore-scripts --no-audit --no-fund` to
   populate `node_modules/`.
4. Re-run the verifier: it rejects archives whose lockfile integrity
   hashes do not match the installed `node_modules` tree.
