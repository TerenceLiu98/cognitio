# Developer Preview

## Compatibility

| Component   | Supported contract | Locally verified                           |
| ----------- | ------------------ | ------------------------------------------ |
| macOS       | 13+, Apple Silicon | arm64 bundle on macOS 27.0                 |
| Node.js     | 22+                | 24.5.0                                     |
| pnpm        | 10.33.2            | 10.33.2                                    |
| Rust        | Stable             | 1.95.0                                     |
| Codex CLI   | Major 0            | 0.146.0                                    |
| Claude Code | Major 2            | 2.1.209                                    |
| OpenCode    | Major 1            | 1.18.18                                    |
| Quartz      | Pinned commit      | `9cf87ff1c248a8ca551093214b0fec3b31415009` |

Unknown agent major versions fail closed so CLI changes cannot silently weaken permissions or alter output parsing.

## Acceptance Record

Verified on 2026-08-15:

- Svelte checks, formatting, 5 Vitest tests, and the static production build pass.
- Rust formatting, strict Clippy, and 20 unit tests pass offline.
- Diagnostics tests confirm bearer tokens and credential-bearing remote URLs are redacted.
- The hidden parser CLI rejects a missing PDF with a nonzero exit and a direct error.
- `LLMWiki.app` builds as arm64, has a valid property list, and passes strict ad-hoc code-signature verification.
- The bundle declares `LSUIElement=true`; its launched process reports `background only=true` and has no Dock presence.

Real GitHub publication was not run because the configured `gh` token is invalid. Precision parsing was not run because no MinerU token is available. Consequently, real-agent PDF quality, remote push, and hosted Quartz rendering remain manual release checks after credentials are restored.

## Manual Smoke Test

1. Run `gh auth login`, verify `gh auth status`, and authenticate one supported agent.
2. Start `pnpm tauri dev`, choose Flash mode, an empty temporary workspace, and a disposable GitHub repository.
3. Complete preflight and initialization, then add a small PDF to `inbox/`.
4. Confirm one job reaches Published, the PDF moves to `done/`, the Wiki is clean, and the remote branch contains the new commit.
5. Open the hosted site and check the paper page, concept links, citations, search, and graph navigation.

## Known Limitations

- The preview is not Developer ID signed, notarized, auto-updated, or supported on Intel, Windows, or Linux.
- Cloudflare Pages connection and GitHub authentication remain user-operated.
- There is no numeric coverage threshold or browser-level Playwright suite yet; current UI coverage is component-level.
