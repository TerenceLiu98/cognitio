<script lang="ts">
  import {
    Check,
    CircleAlert,
    FolderSearch,
    RefreshCw,
    X,
  } from "@lucide/svelte";

  import type { Translator } from "$lib/i18n";
  import type {
    AppSettings,
    InitializationSummary,
    PreflightReport,
    ToolCapability,
  } from "$lib/types";

  let {
    settings = $bindable(),
    tools,
    report,
    busy,
    initialization,
    t,
    onChoose,
    onPreflight,
    onInitialize,
    onCancel,
  }: {
    settings: AppSettings;
    tools: ToolCapability[];
    report: PreflightReport | null;
    busy: boolean;
    initialization: InitializationSummary;
    t: Translator;
    onChoose: () => void;
    onPreflight: () => void;
    onInitialize: () => void;
    onCancel: () => void;
  } = $props();
</script>

<section aria-labelledby="setup-heading">
  <div class="section-heading">
    <div>
      <h1 id="setup-heading">{t("workspaceSetup")}</h1>
      <p>{t("workspaceSetupHint")}</p>
    </div>
  </div>

  <div class="form-section">
    <h2>1. {t("workspaceRoot")}</h2>
    <div class="path-control">
      <input
        aria-label={t("workspaceRoot")}
        readonly
        value={settings.workspaceRoot}
        placeholder="/Users/name/Cognitio"
      />
      <button class="secondary" type="button" onclick={onChoose}
        ><FolderSearch size={16} />{t("choose")}</button
      >
    </div>
  </div>

  <div class="form-grid">
    <label
      >{t("siteTitle")}
      <input
        bind:value={settings.siteTitle}
        maxlength="80"
        placeholder="Research Library"
      />
    </label>
    <label
      >{t("agent")}
      <select bind:value={settings.agentProvider}>
        <option value="auto">Auto</option><option value="codex">Codex</option
        ><option value="claude">Claude Code</option><option value="opencode"
          >OpenCode</option
        >
      </select>
    </label>
    <label
      >{t("model")}
      <input bind:value={settings.model} placeholder={t("agentDefault")} />
    </label>
    <label
      >{t("parserMode")}
      <select bind:value={settings.mineruMode}
        ><option value="precision">{t("precision")}</option><option
          value="flash">{t("flash")}</option
        ></select
      >
      <small>{t("mineruUploadNotice")}</small>
    </label>
    <label
      >{t("repository")}
      <select bind:value={settings.repositoryVisibility}
        ><option value="private">{t("private")}</option><option value="public"
          >{t("public")}</option
        ></select
      >
    </label>
    <label
      >{t("gitRemote")}<input
        bind:value={settings.gitRemote}
        placeholder={t("gitRemoteHint")}
      />
    </label>
    <div class="readonly-setting">
      <span>{t("publishing")}</span>
      <strong>{t("githubActionsPages")}</strong>
    </div>
  </div>

  <div class="form-section">
    <h2>2. {t("tools")}</h2>
    <div class="tool-grid">
      {#each tools as tool (tool.id)}
        <div class="tool-row">
          {#if tool.detected && tool.authenticated !== false}<Check
              size={16}
              class="ok"
            />{:else}<CircleAlert size={16} class="warning" />{/if}
          <strong>{tool.id}</strong>
          <span
            >{tool.version ??
              (tool.detected ? t("detected") : t("missing"))}</span
          >
          <small
            >{tool.authenticated === false
              ? t("actionRequired")
              : tool.authenticated === true
                ? t("authenticated")
                : ""}</small
          >
        </div>
      {/each}
    </div>
    <button
      class="secondary"
      type="button"
      disabled={busy}
      onclick={onPreflight}><RefreshCw size={16} />{t("preflight")}</button
    >
  </div>

  {#if report}
    <div class:ready={report.ready} class="report" role="status">
      {#each report.items as item (item.id)}
        <p>
          {item.ok ? "✓" : "!"}
          {item.message}{item.detail ? `: ${item.detail}` : ""}
        </p>
      {/each}
    </div>
  {/if}

  {#if initialization.state !== "idle"}
    <div class="initialization" role="status" aria-live="polite">
      <div class="initialization-heading">
        <strong>{initialization.message ?? t("initializing")}</strong>
        <span>{initialization.progress}%</span>
      </div>
      <progress max="100" value={initialization.progress}></progress>
      {#if initialization.error}<p class="initialization-error">
          {initialization.error}
        </p>{/if}
    </div>
  {/if}

  <div class="footer-actions">
    {#if initialization.state === "running"}
      <button class="secondary" type="button" onclick={onCancel}
        ><X size={16} />{t("cancel")}</button
      >
    {/if}
    <button
      class="primary"
      type="button"
      disabled={busy ||
        initialization.state === "running" ||
        !settings.workspaceRoot ||
        !settings.siteTitle.trim()}
      onclick={onInitialize}
    >
      {#if busy}<RefreshCw size={16} class="spin" />{:else}<Check
          size={16}
        />{/if}
      {initialization.canRetry ? t("retry") : t("initialize")}
    </button>
  </div>
</section>

<style>
  .form-section {
    margin-top: 30px;
  }
  .form-section h2 {
    margin: 0 0 12px;
    font-size: 13px;
    font-weight: 650;
  }
  .path-control {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
  }
  .form-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 18px;
    margin-top: 28px;
    padding-block: 24px;
    border-block: 1px solid var(--border);
  }
  label {
    display: grid;
    gap: 7px;
    color: var(--muted);
    font-size: 12px;
  }
  .readonly-setting {
    display: grid;
    gap: 7px;
    color: var(--muted);
    font-size: 12px;
  }
  .readonly-setting strong {
    min-height: 38px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 11px;
    color: var(--text);
    background: var(--surface);
    font-weight: 500;
  }
  .tool-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    margin-bottom: 12px;
  }
  .tool-row {
    display: grid;
    grid-template-columns: 18px 80px minmax(0, 1fr);
    align-items: center;
    gap: 7px;
    min-height: 44px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    font-size: 12px;
  }
  .tool-row strong {
    text-transform: capitalize;
  }
  .tool-row span {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tool-row small {
    grid-column: 2 / -1;
    color: #8a5d24;
  }
  :global(.tool-row .ok) {
    color: var(--accent);
  }
  :global(.tool-row .warning) {
    color: #b4742e;
  }
  .report {
    margin-top: 18px;
    border-left: 3px solid #b4742e;
    padding: 4px 14px;
    color: var(--muted);
    background: #fff7e9;
    font-size: 12px;
  }
  .report.ready {
    border-color: var(--accent);
    background: #eef7ef;
  }
  .report p {
    margin: 5px 0;
  }
  .footer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 28px;
  }
  .initialization {
    margin-top: 18px;
    border-left: 3px solid var(--accent);
    padding: 12px 14px;
    background: var(--surface);
  }
  .initialization-heading {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    font-size: 12px;
  }
  progress {
    width: 100%;
    height: 7px;
    margin-top: 10px;
    accent-color: var(--accent);
  }
  .initialization-error {
    margin: 8px 0 0;
    color: #8a3838;
    font-size: 12px;
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (max-width: 650px) {
    .form-grid,
    .tool-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
