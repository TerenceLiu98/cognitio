# LLMWiki

LLMWiki is a local-first macOS utility that turns academic PDFs into a linked Quartz knowledge base. It watches a workspace inbox, delegates paper analysis to Codex, Claude Code, or OpenCode, and verifies that each result is committed and pushed before archiving the source PDF.

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

The ad-hoc-signed local bundle is written to `src-tauri/target/debug/bundle/macos/LLMWiki.app`. It is intentionally not notarized. A production distribution requires an Apple Developer signing identity and notarization credentials.

## Runtime Flow

On first launch, select an empty workspace, choose an agent and MinerU mode, enter a GitHub repository, then run preflight. Initialization creates `inbox/`, `processing/`, `done/`, `failed/`, and `wiki/`, installs the versioned `$llmwiki` skill, pins Quartz to a reviewed commit, builds it locally, and configures the Git remote.

Drop PDFs into `<workspace>/inbox/`. Files are queued after three stable observations. The app processes one job at a time and exposes Retry, Cancel, local logs, and a redacted diagnostics archive.

The bundled parser is also available as a hidden CLI:

```sh
LLMWiki.app/Contents/MacOS/llmwiki parse \
  --input paper.pdf --output parsed --mode flash --json
```

Proxy variables (`http_proxy`, `https_proxy`, and `all_proxy`, including uppercase variants) are inherited by agent processes when the app is launched from that environment.
