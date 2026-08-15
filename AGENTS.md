# Repository Guidelines

## Project Structure & Module Organization

`PRD.md` is the product and architecture baseline, while `PLAN.md` defines the implementation sequence. Keep product decisions in the PRD and record delivery changes in the plan.

The Tauri 2 backend lives in `src-tauri/src/`; the SvelteKit/TypeScript UI lives in `src/`, with static assets in `static/`. Colocate frontend tests as `*.test.ts`; place Rust integration tests under `src-tauri/tests/`. The generated wiki is a separate runtime workspace and must retain `content/papers/`, `content/concepts/`, and `assets/papers/`.

## Build, Test, and Development Commands

Install dependencies with `pnpm install`. The main checks are:

```sh
pnpm tauri dev          # run the desktop app in development
pnpm lint               # run Svelte/TypeScript and formatting checks
pnpm test               # run Vitest once
pnpm build              # build the static frontend
cargo test --manifest-path src-tauri/Cargo.toml
```

Before opening a PR, also run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and `git diff --check`.

## Coding Style & Naming Conventions

Use `rustfmt` conventions for Rust and two-space indentation for Svelte/TypeScript. Prefer `snake_case` for Rust modules and functions, `PascalCase` for Rust types and Svelte components, and `camelCase` for TypeScript variables. Keep orchestration and OS-facing behavior in Rust; keep the frontend presentational. Format Markdown with short paragraphs, fenced examples, and descriptive headings.

## Testing Guidelines

Add tests with every behavioral change. Place Rust unit tests near their modules and integration tests under `src-tauri/tests/`; name frontend tests `*.test.ts`. Prioritize folder lifecycle transitions, stable-file detection, agent adapters, cancellation, failure recovery, and command argument safety. Any new test framework or coverage threshold must be declared in the same change that introduces it.

## Commit & Pull Request Guidelines

History currently contains only `first commit`, so no established commit convention can be inferred. Use concise, imperative subjects such as `Add Codex runner adapter`, and keep commits focused. Pull requests should explain the user-visible outcome, note PRD decisions affected, list verification performed, and link relevant issues. Include screenshots for menubar or settings changes and never commit PDFs, credentials, `.env` files, or generated local job data.
