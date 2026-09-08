<script lang="ts">
  import {
    ChevronRight,
    ExternalLink,
    FileText,
    FolderOpen,
    Logs,
    Pause,
    Play,
    Settings,
  } from "@lucide/svelte";

  import type { Translator } from "$lib/i18n";
  import {
    activeStates,
    countJobs,
    jobPhase,
    processingJobs,
  } from "$lib/job-presentation";
  import type { AppSnapshot, JobState, ViewName } from "$lib/types";

  let {
    snapshot,
    loading,
    error,
    t,
    onToggle,
    onOpen,
    onNavigate,
  }: {
    snapshot: AppSnapshot;
    loading: boolean;
    error: string | null;
    t: Translator;
    onToggle: () => void;
    onOpen: (target: "inbox" | "wiki" | "website") => void;
    onNavigate: (view: ViewName) => void;
  } = $props();

  let activeJobs = $derived(processingJobs(snapshot.jobs));
  let recentJobs = $derived(
    snapshot.jobs
      .filter((job) => !activeJobs.some((active) => active.id === job.id))
      .slice(0, 3),
  );

  function count(states: JobState[]): number {
    return countJobs(snapshot.jobs, states);
  }
</script>

<main class="menu-panel" class:loading aria-label={t("appName")}>
  <header>
    <div class="identity">
      <span class="brand-mark"><FileText size={16} /></span>
      <div>
        <strong>{t("appName")}</strong>
        <span class="app-status">
          <i class:running={snapshot.configured && snapshot.watching}></i>
          {snapshot.configured
            ? snapshot.watching
              ? t("watching")
              : t("paused")
            : t("notConfigured")}
        </span>
      </div>
    </div>
    <div class="header-actions">
      <button
        type="button"
        class="icon-button"
        disabled={!snapshot.configured || loading}
        title={snapshot.watching ? t("pause") : t("resume")}
        aria-label={snapshot.watching ? t("pause") : t("resume")}
        onclick={onToggle}
      >
        {#if snapshot.watching}<Pause size={16} />{:else}<Play size={16} />{/if}
      </button>
      <button
        type="button"
        class="icon-button"
        title={t("settings")}
        aria-label={t("settings")}
        onclick={() => onNavigate(snapshot.configured ? "settings" : "setup")}
      >
        <Settings size={16} />
      </button>
    </div>
  </header>

  {#if error}
    <p class="panel-error" role="alert">{t("commandFailed")} {error}</p>
  {/if}

  <section class="activity" aria-labelledby="activity-title">
    <div class="section-title">
      <h1 id="activity-title">{t("currentActivity")}</h1>
      {#if activeJobs.length > 0}<span>{activeJobs.length}</span>{/if}
    </div>
    {#if activeJobs.length > 0}
      {#each activeJobs as activeJob (activeJob.id)}
        <div class="active-job">
          <div class="job-title">
            <FileText size={17} />
            <div>
              <strong>{activeJob.filename}</strong>
              <span>{jobPhase(activeJob, snapshot.settings.locale)}</span>
            </div>
          </div>
          <div class="progress-track" aria-label={`${activeJob.progress}%`}>
            <span style={`width: ${activeJob.progress}%`}></span>
          </div>
          <div class="job-meta">
            <span>{activeJob.state}</span>
            <span>{activeJob.progress}%</span>
          </div>
        </div>
      {/each}
    {:else}
      <div class="idle-state">
        <span class="idle-icon"><FileText size={18} /></span>
        <div>
          <strong>{t("noActiveJob")}</strong><span>{t("workspaceReady")}</span>
        </div>
      </div>
    {/if}
  </section>

  <section class="summary" aria-label={t("queueSummary")}>
    <div><span>{t("queued")}</span><strong>{count(activeStates)}</strong></div>
    <div>
      <span>{t("completed")}</span><strong>{count(["succeeded"])}</strong>
    </div>
    <div><span>{t("blocked")}</span><strong>{count(["blocked"])}</strong></div>
    <div><span>{t("failed")}</span><strong>{count(["failed"])}</strong></div>
  </section>

  {#if recentJobs.length > 0}
    <section class="recent" aria-labelledby="recent-title">
      <div class="section-title">
        <h2 id="recent-title">{t("recentJobs")}</h2>
        <button type="button" onclick={() => onNavigate("overview")}
          >{t("overview")}<ChevronRight size={14} /></button
        >
      </div>
      <div class="recent-list">
        {#each recentJobs as job (job.id)}
          <div class="recent-row">
            <span
              class:success={job.state === "succeeded"}
              class:problem={job.state === "blocked" || job.state === "failed"}
            ></span>
            <div>
              <strong>{job.filename}</strong><small
                >{jobPhase(job, snapshot.settings.locale)}</small
              >
            </div>
            <em>{job.progress}%</em>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <nav class="panel-links" aria-label={t("quickActions")}>
    <button type="button" onclick={() => onOpen("inbox")}>
      <FolderOpen size={17} /><span>{t("openInbox")}</span><ChevronRight
        size={15}
      />
    </button>
    <button type="button" onclick={() => onOpen("wiki")}>
      <FileText size={17} /><span>{t("openWiki")}</span><ChevronRight
        size={15}
      />
    </button>
    <button
      type="button"
      disabled={!snapshot.settings.siteUrl}
      onclick={() => onOpen("website")}
    >
      <ExternalLink size={17} /><span>{t("openWebsite")}</span><ChevronRight
        size={15}
      />
    </button>
  </nav>

  <footer>
    <button type="button" onclick={() => onNavigate("logs")}
      ><Logs size={15} />{t("logs")}</button
    >
    <button
      type="button"
      onclick={() => onNavigate(snapshot.configured ? "settings" : "setup")}
      ><Settings size={15} />{t("settings")}</button
    >
  </footer>
</main>

<style>
  .menu-panel {
    min-height: 100vh;
    overflow: auto;
    color: #202124;
    background: rgba(250, 250, 249, 0.985);
  }
  .menu-panel.loading {
    opacity: 0.7;
    pointer-events: none;
  }
  header {
    display: flex;
    min-height: 68px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  .identity,
  .header-actions,
  .job-title,
  .app-status,
  footer,
  footer button {
    display: flex;
    align-items: center;
  }
  .identity {
    min-width: 0;
    gap: 10px;
  }
  .identity > div {
    display: grid;
    min-width: 0;
    gap: 3px;
  }
  .identity strong {
    font-size: 14px;
    font-weight: 650;
  }
  .brand-mark {
    display: grid;
    width: 32px;
    height: 32px;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 7px;
    color: #fff;
    background: #34795a;
  }
  .app-status {
    gap: 6px;
    color: var(--muted);
    font-size: 11px;
  }
  .app-status i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #9b9d9b;
  }
  .app-status i.running {
    background: #38a868;
  }
  .header-actions {
    gap: 2px;
  }
  .panel-error {
    margin: 10px 16px 0;
    border-radius: 6px;
    padding: 8px 10px;
    color: #8f352f;
    background: #fce9e7;
    font-size: 11px;
  }
  .activity,
  .recent {
    padding: 15px 16px 16px;
    border-bottom: 1px solid var(--border);
  }
  .section-title {
    display: flex;
    min-height: 20px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 11px;
  }
  .section-title h1,
  .section-title h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 650;
  }
  .section-title > span {
    color: var(--muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .section-title button {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    border: 0;
    padding: 2px 0;
    color: var(--muted);
    background: transparent;
    font-size: 11px;
    cursor: pointer;
  }
  .active-job {
    display: grid;
    gap: 11px;
  }
  .active-job + .active-job {
    margin-top: 13px;
    border-top: 1px solid var(--border);
    padding-top: 13px;
  }
  .job-title {
    min-width: 0;
    gap: 9px;
  }
  .job-title > div {
    display: grid;
    min-width: 0;
    gap: 3px;
  }
  .job-title strong,
  .recent-row strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .job-title strong {
    font-size: 13px;
  }
  .job-title span,
  .idle-state span,
  .recent-row small {
    overflow: hidden;
    color: var(--muted);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .progress-track {
    height: 6px;
    overflow: hidden;
    border-radius: 3px;
    background: #dedfdd;
  }
  .progress-track span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: #34795a;
  }
  .job-meta {
    display: flex;
    justify-content: space-between;
    color: var(--muted);
    font-size: 11px;
    text-transform: capitalize;
  }
  .idle-state {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
  }
  .idle-state > div {
    display: grid;
    gap: 3px;
  }
  .idle-state strong {
    font-size: 13px;
  }
  .idle-icon {
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    border-radius: 7px;
    color: #68706c;
    background: #eceeeb;
  }
  .summary {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border-bottom: 1px solid var(--border);
  }
  .summary div {
    display: grid;
    min-width: 0;
    gap: 3px;
    padding: 11px 8px 12px;
    border-right: 1px solid var(--border);
    text-align: center;
  }
  .summary div:last-child {
    border-right: 0;
  }
  .summary span {
    overflow: hidden;
    color: var(--muted);
    font-size: 10px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .summary strong {
    font-size: 16px;
    font-weight: 620;
    font-variant-numeric: tabular-nums;
  }
  .recent-list {
    display: grid;
    gap: 9px;
  }
  .recent-row {
    display: grid;
    grid-template-columns: 7px minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
  }
  .recent-row > span {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #d59b42;
  }
  .recent-row > span.success {
    background: #43a76a;
  }
  .recent-row > span.problem {
    background: #c65d52;
  }
  .recent-row > div {
    display: grid;
    min-width: 0;
    gap: 2px;
  }
  .recent-row strong {
    font-size: 11px;
    font-weight: 580;
  }
  .recent-row em {
    color: var(--muted);
    font-size: 10px;
    font-style: normal;
    font-variant-numeric: tabular-nums;
  }
  .panel-links {
    display: grid;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .panel-links button {
    display: grid;
    min-height: 38px;
    grid-template-columns: 22px minmax(0, 1fr) auto;
    align-items: center;
    gap: 7px;
    border: 0;
    padding: 0 16px;
    color: var(--text);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .panel-links button:hover:not(:disabled) {
    background: #eceeeb;
  }
  .panel-links button:disabled {
    color: #aaa;
  }
  .panel-links button span {
    font-size: 12px;
  }
  footer {
    justify-content: flex-end;
    gap: 4px;
    padding: 8px 10px;
  }
  footer button {
    min-height: 28px;
    gap: 6px;
    border: 0;
    border-radius: 5px;
    padding: 0 8px;
    color: var(--muted);
    background: transparent;
    font-size: 11px;
    cursor: pointer;
  }
  footer button:hover {
    color: var(--text);
    background: #eceeeb;
  }
</style>
