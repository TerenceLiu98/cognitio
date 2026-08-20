# Cognitio — Product Requirements Document

**Version:** v0.1 Draft
**Date:** 2026-08-15
**Status:** Product / Architecture Baseline
**Product Type:** Local-first menubar utility for academic knowledge compilation

---

## 1. Product Summary

Cognitio is a lightweight local menubar utility that continuously converts academic papers into a linked, searchable Markdown knowledge base.

The user drops a PDF into a watched folder. Cognitio detects the file and uses MinerU to convert it to local Markdown before invoking the user's existing coding agent (Codex, Claude Code, or OpenCode) through its official non-interactive CLI interface. The agent activates the Cognitio skill, reads and analyzes the prepared Markdown, searches the existing local knowledge base, creates or updates linked Markdown pages, commits the changes to Git, and pushes them to GitHub.

The GitHub repository is the canonical knowledge store. Quartz 5 renders the repository into an interconnected academic knowledge website with wikilinks, backlinks, search, graph navigation, LaTeX, citations, and previews. GitHub Actions builds and deploys the site to GitHub Pages from the same repository.

The core product principle is:

> **Cognitio should be a thin automation layer around tools users already have, not a new AI platform with its own infrastructure.**

The architectural model is:

```text
Menubar = orchestration
Agent CLI = reasoning runtime
Cognitio Skill = application logic
MinerU = PDF parser
Markdown = knowledge format
Git = database + version control
GitHub = remote sync
Quartz = knowledge UI
GitHub Actions + Pages = build and hosting
```

---

## 2. Problem Statement

Researchers increasingly use Codex, Claude Code, OpenCode, and similar agents to read and reason about papers, but the resulting knowledge is usually ephemeral:

- insights remain inside chat sessions;
- paper summaries are isolated documents;
- concepts are duplicated across notes;
- links between papers and ideas are not maintained;
- research knowledge is difficult to search or browse later;
- existing RAG products often require uploading documents to a separate cloud service;
- knowledge bases frequently depend on proprietary databases and application-specific formats.

At the same time, academic researchers already have:

- local PDF collections;
- coding-agent subscriptions or API access;
- GitHub accounts;
- Markdown workflows;
- static-site hosting.

Cognitio should connect those existing tools into one automatic workflow.

---

## 3. Product Vision

A researcher should be able to create and maintain a personal academic knowledge base by doing one thing:

```text
Drop papers into a folder.
```

Everything else happens automatically:

```text
paper.pdf
   ↓
local agent
   ↓
structured academic understanding
   ↓
linked Markdown knowledge
   ↓
GitHub
   ↓
Quartz
   ↓
personal academic wiki
```

Over time:

```text
10 papers
→ useful notes

100 papers
→ linked literature collection

1,000 papers
→ personal academic knowledge network
```

Cognitio should remain local-first: the PDF and LLM reasoning process do not need to run on an Cognitio-operated cloud service.

---

## 4. Goals

### 4.1 Primary Goals

Cognitio v0.1 must:

1. Watch one or more user-selected folders for new PDFs.
2. Invoke a supported local coding agent automatically.
3. Support Codex, Claude Code, and OpenCode through subprocess adapters.
4. Allow the user to select an agent and optionally select a model.
5. Use a single Cognitio agent skill for the complete ingest workflow.
6. Use MinerU for PDF-to-Markdown conversion.
7. Generate consistent academic paper pages in Markdown.
8. Reuse and link existing concepts rather than creating unnecessary duplicates.
9. Store the knowledge base in a local Git repository.
10. Push the repository to GitHub.
11. Render the repository using Quartz 5.
12. Deploy the resulting site automatically using GitHub Actions and GitHub Pages.
13. Support wikilinks, backlinks, full-text search, local graph exploration, LaTeX, citations, and hover previews through Quartz.
14. Provide a minimal menubar/tray interface for status and settings.

### 4.2 Secondary Goals

The product should:

- preserve user ownership of Markdown content;
- allow users to edit generated Markdown manually;
- allow the user to replace Codex with Claude Code or OpenCode;
- allow the user to choose the agent's default model or override it;
- remain useful even if Cognitio is no longer installed;
- avoid proprietary knowledge formats;
- keep the system simple enough that a knowledgeable user can understand the entire workflow.

---

## 5. Non-Goals for v0.1

The following are explicitly **not** required for v0.1:

- hosted LLM inference;
- Cognitio-operated PDF parsing service;
- R2 as the canonical knowledge database;
- D1 metadata database;
- Vectorize;
- embeddings;
- semantic search infrastructure;
- rerankers;
- hosted RAG;
- remote academic search API;
- remote MCP knowledge server;
- ACP as the primary agent integration protocol;
- Codex App Server integration;
- Claude Agent SDK integration;
- OpenCode HTTP server integration;
- custom graph database;
- claim/evidence relational database;
- multi-user concurrent editing;
- team workspaces;
- desktop dashboard;
- web application for managing the knowledge base.

These capabilities may be considered only after the basic local workflow proves useful.

---

## 6. Target Users

Primary users are researchers, graduate students, engineers, and technical practitioners who:

- read academic PDFs frequently;
- use Codex, Claude Code, OpenCode, or similar coding agents;
- are comfortable with GitHub;
- want a long-lived personal academic knowledge base;
- prefer local-first workflows;
- do not want to manually maintain backlinks and knowledge pages.

Example workflow:

```text
~/Papers/Inbox
       ↓
Cognitio
       ↓
https://research.example.com
```

---

## 7. Core User Experience

### 7.1 First-Time Setup

On first launch:

1. User selects a watch folder.
2. Cognitio detects supported local agents.
3. User selects Auto, Codex, Claude Code, or OpenCode.
4. User optionally selects a model.
5. User enters MinerU API credentials if needed.
6. User selects or creates a GitHub repository.
7. Cognitio installs the Cognitio agent skill.
8. Cognitio initializes or clones the knowledge repository.
9. Cognitio enables GitHub Pages in Actions mode using the authenticated GitHub CLI.
10. Cognitio begins watching the configured folder.

Initialization runs as a cancellable background operation. The setup window displays stable phases and progress, closing the window does not stop the task, and an interrupted launch returns to a retryable setup state. No workspace is considered configured until its completion marker and Wiki Git repository both exist.

After setup, the user should not need a primary application window.

### 7.2 Normal Workflow

```text
User downloads paper.pdf
        ↓
Moves paper.pdf into watched folder
        ↓
Cognitio detects stable new file
        ↓
Cognitio selects configured AgentRunner
        ↓
Agent starts in local Wiki repository
        ↓
$llmwiki skill is explicitly invoked
        ↓
MinerU converts PDF → Markdown
        ↓
Agent reads generated Markdown
        ↓
Agent searches existing local Wiki
        ↓
Agent writes/updates Markdown pages
        ↓
Agent validates changes
        ↓
git commit
        ↓
git push
        ↓
GitHub Actions detects the push
        ↓
Quartz build
        ↓
Wiki website updated
```

---

## 8. Product Surface

Cognitio should behave like a system utility, not a normal desktop app.

Example menubar:

```text
Cognitio

● Watching ~/Papers/Inbox

Processing
  Mamba.pdf
  └─ Updating Wiki with Codex

────────────────────

Open Inbox
Open Wiki
Open Repository

Pause Watching

Settings...
View Logs

Quit
```

---

## 9. Settings

### General

```text
Watch Folder
~/Papers/Inbox
[Choose...]

Launch at Login
[✓]

After Processing
( ) Keep PDF
(●) Move to Done
( ) Delete PDF
```

### Agent

```text
Agent
(●) Auto
( ) Codex
( ) Claude Code
( ) OpenCode

Model
(●) Agent Default
( ) <supported model override>
```

Default must be **Agent Default**. Cognitio must not hard-code a model.

### MinerU

```text
Parser
MinerU

Mode
(●) Precision
( ) Flash

API Key
••••••••••••••
[Test]
```

Secrets should be stored in the OS credential store where practical and must never be written into `SKILL.md`.

### Repository

```text
Wiki Repository
~/Cognitio/wiki

Git Remote
github.com/user/my-llmwiki

Website
https://research.example.com
```

---

## 10. Local Architecture

Recommended stack:

```text
Tauri 2
Rust
Tokio
Svelte + TypeScript
```

The Rust side owns:

- filesystem watching;
- subprocess spawning;
- cancellation;
- process environment;
- filesystem paths;
- configuration;
- job status.

The frontend is limited to:

- settings;
- status;
- recent jobs;
- logs.

---

## 11. Folder Workflow

Recommended local directories:

```text
~/Cognitio/
├── inbox/
├── processing/
├── done/
├── failed/
└── wiki/
```

Lifecycle:

```text
inbox/paper.pdf
       ↓
processing/paper.pdf
       ↓
done/paper.pdf
```

On failure:

```text
failed/paper.pdf
failed/paper.log
```

The filesystem is sufficient as the v0.1 job-state model. SQLite is not required initially.

---

## 12. Agent Invocation Strategy

### Decision

Cognitio v0.1 uses:

> **Direct subprocess execution of each agent's official non-interactive CLI.**

Do not use `bash -c` as the primary invocation mechanism.

Instead:

```text
spawn(
  executable,
  argv[],
  cwd,
  env
)
```

### Codex

Conceptual invocation:

```bash
codex exec   --json   --sandbox workspace-write   [--model MODEL]   "<task>"
```

### Claude Code

Conceptual invocation:

```bash
claude   -p   --output-format stream-json   [--model MODEL]   "<task>"
```

### OpenCode

Conceptual invocation:

```bash
opencode run   --format json   [-m provider/model]   "<task>"
```

Exact flags must be version-tested during implementation.

---

## 13. AgentRunner Abstraction

Define one minimal abstraction:

```text
AgentRunner

detect()
version()
authenticated()
run()
cancel()
```

Conceptual invocation:

```text
run(
  cwd,
  prompt,
  model?,
  env
)
```

Implementations:

```text
CodexRunner
ClaudeRunner
OpenCodeRunner
```

This is the only provider-specific abstraction required for v0.1.

---

## 14. Agent Model Selection

The user may choose:

```text
Agent Default
```

or an explicit model.

If no model is selected, Cognitio passes no model override.

If selected:

```text
Codex
→ --model ...

Claude
→ --model ...

OpenCode
→ -m provider/model
```

v0.1 principle:

> **one paper = one agent job = one selected model**

Task-specific multi-model routing is out of scope.

---

## 15. Structured Agent Events

Where supported, request JSON or JSONL output.

These events are used for:

- progress;
- logging;
- debugging;
- cancellation.

They are not the main transport for knowledge artifacts.

The primary result of a job is:

```text
changes to the local Git repository
```

Normalized events may include:

```text
Started
Thinking
ToolStarted
ToolCompleted
FileChanged
Message
Completed
Failed
```

---

## 16. ACP Decision

ACP is not part of the v0.1 critical path.

Reasons:

- non-interactive CLI is the current common denominator across supported agents;
- ACP support is not equally native across all providers;
- ACP would make the menubar utility a more complex Agent Host.

ACP may be introduced later if Cognitio needs:

- interactive approval UI;
- live conversations;
- session management;
- resume;
- rich tool events;
- standardized multi-agent transport.

The `AgentRunner` abstraction must permit a future `AcpRunner`.

---

## 17. Cognitio Skill

The system installs one primary skill:

```text
$llmwiki
```

Recommended structure:

```text
llmwiki/
├── SKILL.md
└── references/
    ├── paper-format.md
    ├── concept-format.md
    └── writing-guide.md
```

Do not split the v0.1 workflow into separate MinerU, analysis, Wiki, and publishing skills.

---

## 18. Skill Responsibilities

The skill should instruct the agent to:

1. Accept the source PDF metadata and Cognitio-provided Markdown path.
2. Refuse to invoke MinerU, upload the PDF, or run another parser.
3. Read the provided Markdown fully.
4. Identify research question, motivation, contributions, method, experiments, limitations, and important concepts.
5. Search the existing Wiki before creating concepts.
6. Reuse canonical concepts where possible.
7. Create a Paper page.
8. Create or update Concept pages only when useful.
9. Add wikilinks.
10. Avoid unsupported claims.
11. Validate the result.
12. Inspect `git diff`.
13. Commit changes.
14. Push changes.

---

## 19. MinerU Integration

MinerU is an App-owned parsing adapter rather than an Agent capability.

Its role is:

```text
PDF
 ↓
MinerU
 ↓
Markdown
```

The rest of the Cognitio workflow begins from Markdown.

The app provides a diagnostic CLI:

```bash
llmwiki parse paper.pdf
```

The background worker calls the same Rust adapter before starting the Agent and reuses a non-empty task-local `parsed/full.md` on Retry. The Skill must not call MinerU directly. v0.1 does not introduce MinerU MCP or another parsing service.

---

## 20. MinerU Credentials

Preferred v0.1 flow:

```text
Settings
 ↓
OS Keychain
 ↓
App-owned MinerU adapter
 ↓
MinerU integration
```

The key must never be:

- committed to Git;
- written to Markdown;
- embedded in `SKILL.md`.

A more elaborate secret broker is out of scope.

---

## 21. Canonical Knowledge Store

The canonical knowledge store is:

> **A Git repository containing Markdown.**

v0.1 does not require:

- R2;
- D1;
- graph database;
- vector database.

Git provides:

- version history;
- diff;
- rollback;
- synchronization;
- conflict detection;
- review.

---

## 22. Repository Structure

Recommended:

```text
my-llmwiki/
├── content/
│   ├── papers/
│   │   ├── mamba.md
│   │   └── attention-is-all-you-need.md
│   │
│   └── concepts/
│       ├── transformer.md
│       ├── state-space-model.md
│       └── selective-scan.md
│
├── assets/
│   └── papers/
│
├── references.bib
│
├── quartz.config.ts
├── quartz.layout.ts
└── package.json
```

Knowledge content should remain renderer-independent whenever possible.

---

## 23. Knowledge Model

v0.1 has two required page types:

```text
Paper
Concept
```

Author, Dataset, Method, Benchmark, and Model pages are future extensions.

---

## 24. Paper Markdown Format

```markdown
---
title: "Mamba: Linear-Time Sequence Modeling with Selective State Spaces"
type: paper
authors:
  - Albert Gu
  - Tri Dao
year: 2023
arxiv: "2312.00752"
tags:
  - state-space-model
  - sequence-modeling
---

# Mamba

## TL;DR

...

## Research Question

...

## Motivation

...

## Contributions

...

## Method

...

## Experiments

...

## Limitations

...

## Related Concepts

- [[State Space Models]]
- [[Selective Scan]]

## Related Papers

- [[S4]]
```

---

## 25. Concept Markdown Format

```markdown
---
title: State Space Models
type: concept
aliases:
  - SSM
tags:
  - sequence-modeling
---

# State Space Models

## Overview

...

## Key Ideas

...

## Important Papers

...

## Related Concepts

- [[Selective Scan]]
- [[Linear Attention]]
```

Concept pages may evolve gradually. The agent does not need to update every related concept on every ingest.

---

## 26. Linking Model

The agent maintains **forward links** only.

Example:

```markdown
Mamba builds on [[State Space Models]].
```

Quartz derives backlinks automatically.

This prevents unnecessary multi-file writes.

---

## 27. Knowledge Resolution

Before creating a Concept page, the agent searches the local repository.

Example:

```bash
rg -i "test.?time|inference.?time" content/concepts
git grep "State Space"
```

Because the agent's working directory is the Wiki repo, the filesystem itself is the v0.1 Knowledge API.

Remote MCP is not required.

---

## 28. Git Workflow

```text
agent writes files
      ↓
git diff
      ↓
validation
      ↓
git add
      ↓
git commit
      ↓
git push
```

The menubar app does not need to parse or rewrite the generated knowledge.

A successful job is primarily determined by:

- agent exit status;
- Git state;
- push success.

---

## 29. Website Renderer

### Decision

v0.1 uses:

> **Quartz 5**

instead of Hugo.

Quartz directly provides:

- wikilinks;
- backlinks;
- full-text search;
- graph views;
- explorer;
- breadcrumbs;
- table of contents;
- tags;
- popover previews;
- LaTeX;
- citations;
- frontmatter support.

Using Hugo would require rebuilding much of this knowledge-navigation layer.

---

## 30. Website Information Architecture

### Homepage

```text
Cognitio

Search...

Recent Papers

Mamba-2
DeepSeek-R1
Scaling LLM Test-Time Compute

Concepts

State Space Models
Reasoning
Mixture of Experts
Test-Time Compute

Explore Graph
```

### Paper Page

```text
Title
Metadata
TL;DR
Research Question
Motivation
Contributions
Method
Experiments
Limitations
Related Concepts
Related Papers
Backlinks
Local Graph
```

### Concept Page

```text
Definition
Aliases
Key Ideas
Important Papers
Related Concepts
Backlinks
Local Graph
```

---

## 31. Search

v0.1 uses:

```text
Local agent:
filesystem / rg / git grep

Website:
Quartz built-in search
```

No embeddings, vector search, reranker, or remote search API are required.

---

## 32. Deployment

Recommended:

```text
Local Git Repo
     ↓
git push
     ↓
GitHub
     ↓
GitHub Actions
     ↓
Quartz build and Pages artifact deployment
     ↓
Published Cognitio
```

The local app uses the existing authenticated `gh` session to enable Pages and does not store a separate GitHub token.

---

## 33. Authentication

Reuse existing agent authentication wherever possible.

- Codex authentication belongs to Codex CLI.
- Claude authentication belongs to Claude Code.
- OpenCode provider authentication belongs to OpenCode.

Cognitio should not ask users to duplicate OpenAI or Anthropic keys merely to invoke authenticated CLIs.

MinerU credentials are separate.

---

## 34. Error Handling

A job may fail during:

- PDF detection;
- MinerU parsing;
- agent execution;
- Git validation;
- commit;
- push.

On failure:

```text
paper.pdf
→ failed/
```

The user should receive:

- notification;
- error state;
- logs;
- Retry.

Never delete the original PDF on failure.

---

## 35. Notifications

Success:

```text
Mamba

Added to Cognitio
3 concepts linked

[Open Paper]
```

Failure:

```text
Mamba.pdf

Cognitio could not complete this paper.

[View Error] [Retry]
```

---

## 36. Privacy

Cognitio is local-first.

By default:

- the PDF remains local during processing;
- agent execution happens through the user's selected local agent;
- Cognitio does not require its own hosted LLM service;
- only generated Markdown/assets are pushed to GitHub.

Users remain subject to the privacy policies of their chosen agent, MinerU, GitHub, and hosting provider.

---

## 37. Security

v0.1 principles:

1. Spawn binaries directly rather than interpolating shell commands.
2. Constrain the agent working directory to the Wiki repository.
3. Use available workspace/sandbox restrictions.
4. Keep secrets out of Git and skills.
5. Never commit `.env` or credentials.
6. Preserve original PDFs on failure.

---

## 38. Expected Scale

v0.1 is optimized for personal knowledge bases:

```text
10–1,000 papers
```

Priority order:

```text
reliability
simplicity
knowledge quality
```

Large-scale search infrastructure is intentionally deferred.

---

## 39. Success Metrics

### Activation

- successful first-paper processing rate;
- time from install to first published paper.

### Usage

- papers processed per active user per week;
- users reaching 10+ papers;
- users reaching 100+ papers;
- 30-day repeat usage.

### Reliability

- MinerU parse success rate;
- agent completion rate;
- Git push success rate;
- site deployment success rate.

### Knowledge Quality

Sampled evaluation of:

- metadata correctness;
- summary accuracy;
- contribution extraction;
- concept reuse;
- duplicate concept avoidance;
- valid links.

---

## 40. v0.1 Acceptance Criteria

A user can:

1. Install Cognitio.
2. Configure a watch folder.
3. Select Codex, Claude Code, OpenCode, or Auto.
4. Optionally override the model.
5. Configure MinerU.
6. Select or create a Wiki Git repository.
7. Drop a PDF into the watched folder.
8. See Processing status.
9. Have the agent parse and analyze the paper.
10. Have the agent search existing Wiki pages.
11. Create a Paper Markdown page.
12. Add relevant wikilinks.
13. Commit and push to GitHub.
14. Trigger Quartz deployment.
15. Open the new paper on the website.
16. See backlinks and graph navigation.
17. Retry failed jobs.

---

## 41. v0.1 Technical Stack

### Local Utility

```text
Tauri 2
Rust
Tokio
Svelte + TypeScript
```

### External Local Tools

```text
Codex CLI
Claude Code CLI
OpenCode CLI
MinerU
Git
```

### Knowledge

```text
Markdown
YAML Frontmatter
[[wikilinks]]
BibTeX
Git
```

### Remote

```text
GitHub
```

### Website

```text
Quartz 5
GitHub Actions
GitHub Pages
```

---

## 42. Deferred Architecture

### MCP

Add when users need remote agent access without cloning the repository.

Possible tools:

```text
search
read_page
related_pages
```

### Remote Search

Add when Quartz search no longer scales.

Possible future stack:

```text
Edge worker
D1 FTS
Vectorize
hybrid retrieval
reranker
```

### R2

Add if GitHub becomes unsuitable for large/private artifacts.

### Structured Claim Database

Add only if the product requires claim-level provenance or systematic evidence retrieval.

### ACP

Add if Cognitio evolves into a rich interactive Agent Client.

---

## 43. Product Principles

### Local First

The core AI workflow runs on the user's machine.

### Existing Tools First

Use Codex, Claude Code, OpenCode, MinerU, GitHub, and Quartz rather than recreating them.

### Markdown Is the Product

The knowledge base must remain useful without Cognitio.

### Git Is Enough Until It Isn't

Do not introduce databases until user scale or features require them.

### One Skill

v0.1 should have one primary workflow skill.

### Explicit Agent Invocation

Cognitio explicitly invokes its skill rather than hoping the agent discovers it.

### Renderer Independence

Avoid unnecessary Quartz-specific content syntax.

### Derived UI

Backlinks, graphs, and indexes are derived from Markdown rather than manually maintained.

---

## 44. Final v0.1 Architecture

```text
                 User
                  │
             drops PDF
                  │
                  ▼
        ┌──────────────────┐
        │ Cognitio Menubar  │
        │                  │
        │ watch            │
        │ settings         │
        │ status           │
        └────────┬─────────┘
                 │
                 ▼
              AgentRunner
                 │
        ┌────────┼────────┐
        ▼        ▼        ▼
      Codex    Claude   OpenCode
        │        │        │
        └────────┼────────┘
                 ▼
            $llmwiki Skill
                 │
                 ▼
              MinerU
                 │
                 ▼
          Parsed Markdown
                 │
                 ▼
        Agent reads + reasons
                 │
                 ▼
       Search existing Wiki
                 │
                 ▼
        Linked Markdown Wiki
                 │
                 ▼
             git commit
                 │
                 ▼
              git push
                 │
─────────────────┼────────────────
                 ▼
               GitHub
                 │
                 ▼
              Quartz 5
                 │
                 ▼
         GitHub Pages
                 │
                 ▼
          Academic Cognitio
```

---

## 45. Product Definition

Cognitio is:

> **A local-first academic knowledge compiler that turns papers into an interconnected Markdown Wiki using the coding agents users already have.**

Product-language version:

> **Drop papers into a folder. Your local AI agent continuously compiles them into a searchable, linked academic knowledge base.**
