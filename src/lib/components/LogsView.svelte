<script lang="ts">
  import { Download, HardDrive, Logs } from "@lucide/svelte";
  import type { Translator } from "$lib/i18n";
  import type { LogEntry } from "$lib/types";

  let {
    entries,
    locale,
    t,
    onExport,
  }: {
    entries: LogEntry[];
    locale: string;
    t: Translator;
    onExport: (includeDetailedLogs: boolean) => void;
  } = $props();
</script>

<section aria-labelledby="logs-heading">
  <div class="section-heading">
    <div>
      <h1 id="logs-heading">{t("allLogs")}</h1>
      <p><HardDrive size={14} />{t("localOnly")}</p>
    </div>
    <div class="export-actions">
      <button class="secondary" type="button" onclick={() => onExport(false)}
        ><Download size={16} />{t("exportDiagnostics")}</button
      >
      <button class="secondary" type="button" onclick={() => onExport(true)}
        ><Download size={16} />{t("exportDetailedDiagnostics")}</button
      >
    </div>
  </div>
  {#if entries.length === 0}
    <div class="empty-state">
      <Logs size={28} strokeWidth={1.5} />
      <p>{t("noLogs")}</p>
    </div>
  {:else}
    <div class="log-list">
      {#each entries as entry (entry.id)}
        <article
          class:error={entry.level === "error"}
          class:warn={entry.level === "warn"}
        >
          <time datetime={entry.timestamp}
            >{new Intl.DateTimeFormat(locale, {
              dateStyle: "short",
              timeStyle: "medium",
            }).format(new Date(entry.timestamp))}</time
          >
          <span class="level">{entry.level}</span>
          <p>{entry.message}</p>
          {#if entry.jobId}<code>{entry.jobId}</code>{/if}
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .section-heading p {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .export-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }
  .empty-state {
    display: grid;
    min-height: 240px;
    place-content: center;
    justify-items: center;
    margin-top: 30px;
    border: 1px dashed var(--border-strong);
    border-radius: 6px;
    color: var(--muted);
  }
  .log-list {
    margin-top: 30px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
  }
  article {
    display: grid;
    grid-template-columns: 160px 58px minmax(0, 1fr) auto;
    gap: 12px;
    padding: 11px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  article:last-child {
    border-bottom: 0;
  }
  article.error {
    border-left: 3px solid #b04b4b;
  }
  article.warn {
    border-left: 3px solid #b4742e;
  }
  time,
  code {
    color: var(--muted);
  }
  .level {
    text-transform: uppercase;
  }
  article p {
    margin: 0;
  }
  @media (max-width: 700px) {
    article {
      grid-template-columns: 1fr auto;
    }
    article p {
      grid-column: 1 / -1;
    }
    article code {
      display: none;
    }
  }
</style>
