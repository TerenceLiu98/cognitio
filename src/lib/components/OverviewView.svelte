<script lang="ts">
  import {
    CircleX,
    ExternalLink,
    FileText,
    FolderOpen,
    RefreshCw,
    RotateCcw,
  } from "@lucide/svelte";

  import type { Translator } from "$lib/i18n";
  import type { AppSnapshot, JobState, RetryMode } from "$lib/types";

  let {
    snapshot,
    t,
    onOpen,
    onRetry,
    onCancel,
  }: {
    snapshot: AppSnapshot;
    t: Translator;
    onOpen: (target: "inbox" | "wiki" | "website") => void;
    onRetry: (id: string, mode: RetryMode) => void;
    onCancel: (id: string) => void;
  } = $props();

  const activeStates: JobState[] = [
    "stabilizing",
    "queued",
    "preflight",
    "running",
    "verifying",
    "archiving",
  ];

  function count(states: JobState[]): number {
    return snapshot.jobs.filter((job) => states.includes(job.state)).length;
  }

  function dateLabel(value: string): string {
    return new Intl.DateTimeFormat(snapshot.settings.locale, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(value));
  }

  function deploymentLabel(
    status: AppSnapshot["jobs"][number]["deployment"]["status"],
  ): string | null {
    if (status === "pending" || status === "running")
      return t("deploymentPending");
    if (status === "succeeded") return t("deploymentReady");
    if (status === "failed") return t("deploymentFailed");
    if (status === "unknown") return t("deploymentUnknown");
    return null;
  }
</script>

<section aria-labelledby="overview-heading">
  <div class="section-heading">
    <div>
      <h1 id="overview-heading">{t("overview")}</h1>
      <p>{snapshot.configured ? t("workspaceReady") : t("setupRequired")}</p>
    </div>
    <div class="quick-actions">
      <button class="secondary" type="button" onclick={() => onOpen("inbox")}>
        <FolderOpen size={16} />
        {t("openInbox")}
      </button>
      <button class="secondary" type="button" onclick={() => onOpen("wiki")}>
        <FileText size={16} />
        {t("openWiki")}
      </button>
      <button
        class="secondary"
        type="button"
        disabled={!snapshot.settings.siteUrl}
        onclick={() => onOpen("website")}
      >
        <ExternalLink size={16} />
        {t("openWebsite")}
      </button>
    </div>
  </div>

  <div class="metrics" aria-label={t("currentActivity")}>
    <div><span>{t("queued")}</span><strong>{count(activeStates)}</strong></div>
    <div>
      <span>{t("completed")}</span><strong>{count(["succeeded"])}</strong>
    </div>
    <div>
      <span>{t("blocked")}</span><strong>{count(["blocked"])}</strong>
    </div>
    <div>
      <span>{t("failed")}</span><strong>{count(["failed"])}</strong>
    </div>
  </div>

  <div class="table-heading">
    <h2>{t("recentJobs")}</h2>
    <span>{snapshot.jobs.length}</span>
  </div>

  {#if snapshot.jobs.length === 0}
    <div class="empty-state">
      <FileText size={28} strokeWidth={1.5} />
      <p>{t("noJobs")}</p>
    </div>
  {:else}
    <div class="job-list">
      <div class="job-header" aria-hidden="true">
        <span>{t("file")}</span><span>{t("status")}</span><span
          >{t("progress")}</span
        ><span>{t("updated")}</span><span></span>
      </div>
      {#each snapshot.jobs as job (job.id)}
        <article class="job-row">
          <div class="filename">
            <FileText size={16} />
            <div>
              <span>{job.filename}</span><small>{job.phase}</small
              >{#if job.error}<details>
                  <summary>{t("details")}</summary>
                  <p>{job.error}</p>
                </details>{/if}
            </div>
          </div>
          <div class="state-cell">
            <span
              class:failed={job.state === "failed"}
              class:blocked={job.state === "blocked"}
              class:success={job.state === "succeeded"}
              class="state">{job.state}</span
            >{#if deploymentLabel(job.deployment.status)}<a
                class="deployment"
                class:failed={job.deployment.status === "failed"}
                href={job.deployment.url ?? undefined}
                target="_blank"
                rel="noreferrer"
                title={job.deployment.error ?? undefined}
                >{deploymentLabel(job.deployment.status)}</a
              >{/if}
          </div>
          <div class="progress">
            <span style={`width: ${job.progress}%`}></span>
          </div>
          <time datetime={job.updatedAt}>{dateLabel(job.updatedAt)}</time>
          <div class="row-actions">
            {#if job.allowedActions.includes("retry")}
              <button
                class="icon-button"
                type="button"
                title={t("retry")}
                aria-label={t("retry")}
                onclick={() => onRetry(job.id, "reuse_valid")}
                ><RotateCcw size={16} /></button
              >
            {/if}
            {#if job.allowedActions.includes("reparse")}
              <button
                class="icon-button"
                type="button"
                title={t("reparse")}
                aria-label={t("reparse")}
                onclick={() => {
                  if (globalThis.confirm(t("confirmReparse")))
                    onRetry(job.id, "force_reparse_current_mode");
                }}><RefreshCw size={16} /></button
              >
            {/if}
            {#if job.allowedActions.includes("cancel")}
              <button
                class="icon-button"
                type="button"
                title={t("cancel")}
                aria-label={t("cancel")}
                onclick={() => {
                  if (globalThis.confirm(t("confirmCancel"))) onCancel(job.id);
                }}><CircleX size={16} /></button
              >
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .metrics {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    margin: 26px 0 34px;
    border-block: 1px solid var(--border);
  }
  .metrics div {
    display: flex;
    min-height: 92px;
    flex-direction: column;
    justify-content: center;
    padding: 14px 20px;
    border-right: 1px solid var(--border);
  }
  .metrics div:last-child {
    border-right: 0;
  }
  .metrics span {
    color: var(--muted);
    font-size: 12px;
  }
  .metrics strong {
    margin-top: 4px;
    font-size: 28px;
    font-weight: 620;
  }
  .quick-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }
  .table-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  .table-heading h2 {
    margin: 0;
    font-size: 15px;
  }
  .table-heading span {
    color: var(--muted);
    font-size: 12px;
  }
  .empty-state {
    display: grid;
    min-height: 210px;
    place-content: center;
    justify-items: center;
    border: 1px dashed var(--border-strong);
    border-radius: 6px;
    color: var(--muted);
    text-align: center;
  }
  .empty-state p {
    margin: 10px 0 0;
  }
  .job-list {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
  }
  .job-header,
  .job-row {
    display: grid;
    grid-template-columns:
      minmax(180px, 2fr) 110px minmax(100px, 1fr)
      150px 72px;
    align-items: center;
    gap: 12px;
  }
  .job-header {
    padding: 9px 14px;
    color: var(--muted);
    background: var(--surface-subtle);
    font-size: 11px;
  }
  .job-row {
    min-height: 58px;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
    font-size: 13px;
  }
  .filename {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 9px;
  }
  .filename span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .filename > div {
    display: grid;
    min-width: 0;
    gap: 3px;
  }
  .filename small,
  .filename summary,
  .filename p {
    color: var(--muted);
    font-size: 11px;
  }
  .filename details,
  .filename p {
    margin: 0;
  }
  .filename summary {
    width: fit-content;
    cursor: pointer;
  }
  .filename p {
    margin-top: 4px;
    white-space: normal;
  }
  .state-cell {
    display: grid;
    justify-items: start;
    gap: 5px;
  }
  .state {
    width: fit-content;
    border-radius: 999px;
    padding: 3px 8px;
    color: #725e20;
    background: #f3ebc9;
    font-size: 11px;
    text-transform: capitalize;
  }
  .state.success {
    color: #2f673a;
    background: #deeee1;
  }
  .state.failed {
    color: #8a3838;
    background: #f4dede;
  }
  .state.blocked {
    color: #744b13;
    background: #f2e2c6;
  }
  .deployment {
    color: var(--muted);
    font-size: 10px;
    text-decoration: none;
  }
  .deployment.failed {
    color: #8a3838;
  }
  .progress {
    height: 5px;
    overflow: hidden;
    border-radius: 3px;
    background: #e2e4e1;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  time {
    color: var(--muted);
    font-size: 12px;
  }
  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }
  @media (max-width: 880px) {
    .job-header {
      display: none;
    }
    .job-row {
      grid-template-columns: minmax(0, 1fr) auto auto;
    }
    .progress,
    time {
      display: none;
    }
  }
  @media (max-width: 600px) {
    .section-heading {
      align-items: flex-start;
    }
    .quick-actions {
      width: 100%;
      justify-content: flex-start;
    }
    .metrics div {
      min-height: 78px;
      padding: 10px 12px;
    }
    .metrics strong {
      font-size: 23px;
    }
  }
</style>
