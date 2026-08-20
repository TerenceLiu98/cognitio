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

Verified on 2026-08-20:

- Svelte checks, formatting, 6 Vitest tests, desktop/mobile Chromium Playwright workflows, and the static production build pass.
- Rust formatting, strict Clippy, and 39 Rust tests pass offline, including mock MinerU HTTP and local bare Git publication recovery.
- Diagnostics tests confirm tokens, credential-bearing remote URLs, workspace paths, and detailed logs are excluded from the default archive.
- Task tests cover backend-owned actions, exact `Cognitio-Job` trailer lookup, unrelated commits, verified parse manifests, streamed MinerU responses, and bounded ZIP extraction.
- The hidden parser CLI rejects a missing PDF with a nonzero exit and a direct error.
- `Cognitio.app` builds as arm64, has a valid property list, and passes strict ad-hoc code-signature verification.
- The bundle declares `LSUIElement=true`; its launched process reports `background only=true` and has no Dock presence.

This stabilization run deliberately did not call real GitHub, MinerU, or paid Agent services. Real-agent PDF quality, Precision parsing, remote push, and hosted Quartz rendering remain credentialed manual release checks.

## Manual Smoke Test

1. Run `gh auth login`, verify `gh auth status`, and authenticate one supported agent.
2. Start `pnpm tauri dev`, choose Flash mode, an empty temporary workspace, and a disposable GitHub repository.
3. Complete preflight and initialization, then add a small PDF to `inbox/`.
4. Confirm one job reaches Published, the PDF moves to `done/`, the Wiki is clean, and the remote contains exactly one commit with the matching job trailer.
5. Repeat with Precision, then verify Retry reuses the matching manifest and Reparse calls the currently selected mode.
6. Open the hosted site and check the deployment badge, paper page, concept links, citations, search, and graph navigation.
7. Repeat the small-paper run with Codex, Claude Code, and OpenCode before tagging a release.

## Known Limitations

- The preview is not Developer ID signed, notarized, auto-updated, or supported on Intel, Windows, or Linux.
- GitHub authentication and repository administration permissions remain user-operated; the app enables Pages workflow mode through the authenticated `gh` session.
- Quartz is bundled at the pinned commit, so workspace initialization does not download the template or run Node locally.
- There is no numeric coverage threshold. Browser coverage targets stabilization workflows rather than the native Tauri menu bar shell.
