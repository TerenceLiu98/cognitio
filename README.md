# Cognitio

Cognitio is a local-first macOS menu bar utility that turns academic PDFs into a linked Quartz knowledge base. It watches a workspace inbox, delegates paper analysis to Codex, Claude Code, or OpenCode, and verifies that each result is committed and pushed before archiving the source PDF.

The current release is an unsigned developer preview for macOS 13+ on Apple Silicon. Read `PRD.md` for product scope, `PLAN.md` for architecture and milestones, and `docs/DEVELOPER_PREVIEW.md` for compatibility and acceptance status.

## Prerequisites

- Node.js 22+, pnpm 10.33.2, Rust stable, and Xcode command-line tools
- Git with global `user.name` and `user.email`
- An authenticated supported agent CLI
- GitHub CLI authentication when using an `owner/repository` target
- A MinerU token for Precision mode; Flash mode does not require one

## Development

```sh
pnpm install
pnpm tauri dev
```

Run the complete local quality gate:

```sh
pnpm lint
pnpm test
pnpm test:e2e
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
git diff --check
```

## Build The App

```sh
pnpm tauri build --debug --bundles app
```

The ad-hoc-signed local bundle is written to `src-tauri/target/debug/bundle/macos/Cognitio.app`. It is intentionally not notarized. A production distribution requires an Apple Developer signing identity and notarization credentials.

After setup, Cognitio launches without a Dock icon or primary window. Use its menu bar icon to view processing status, pause or resume watching, open workspace folders, and reach Settings or Logs. Closing the settings window hides it without stopping the background worker; use Quit Cognitio in the menu to stop the app.

## Runtime Flow

On first launch, select an empty workspace, choose a site title, agent, and MinerU mode, enter a GitHub repository, then run preflight. Initialization creates `inbox/`, `processing/`, `done/`, `failed/`, and `wiki/`, installs the versioned `$llmwiki` skill, pins Quartz to a reviewed commit, and configures the Git remote. GitHub Actions installs Quartz dependencies and builds the site after each push.

Drop PDFs into `<workspace>/inbox/`. Files are queued after three stable observations. Cognitio sends each PDF to MinerU, stores a checksummed Markdown result and parse manifest in the task directory, and gives only that local Markdown to the selected agent. The app processes one job at a time and exposes Retry, explicit reparse, Cancel, local logs, and a redacted diagnostics archive.

Each job captures its Agent, model, MinerU mode, and archival behavior when it enters the queue. A normal Retry preserves those choices and reuses Markdown only when the PDF hash, mode, parser profile, size, and checksum match. Reparse uses the currently selected MinerU mode. Git publication is accepted only for one clean commit carrying the matching `Cognitio-Job` trailer and changing approved Wiki paths.

The task succeeds after the verified commit is present upstream and the source PDF is archived. GitHub Pages deployment is monitored separately, so a workflow failure is shown on the task without rerunning paper analysis. The GitHub repository is fixed after workspace initialization; use a new workspace to change repositories.

The bundled parser is also available as a hidden CLI:

```sh
Cognitio.app/Contents/MacOS/llmwiki parse \
  --input paper.pdf --output parsed --mode flash --json
```

Proxy variables (`http_proxy`, `https_proxy`, and `all_proxy`, including uppercase variants) are inherited by agent processes when the app is launched from that environment.
