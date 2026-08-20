<script lang="ts">
  import { AlertTriangle, Pause, Play, X } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";

  import AppSidebar from "$lib/components/AppSidebar.svelte";
  import LogsView from "$lib/components/LogsView.svelte";
  import OverviewView from "$lib/components/OverviewView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import SetupView from "$lib/components/SetupView.svelte";
  import * as api from "$lib/api";
  import { translate } from "$lib/i18n";
  import { isViewName } from "$lib/navigation";
  import {
    emptySnapshot,
    type AppSettings,
    type AppSnapshot,
    type PreflightReport,
    type RetryMode,
    type ViewName,
  } from "$lib/types";

  let snapshot: AppSnapshot = $state(structuredClone(emptySnapshot));
  let draft: AppSettings = $state({ ...emptySnapshot.settings });
  let active: ViewName = $state("overview");
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let report = $state<PreflightReport | null>(null);
  let mineruToken = $state("");
  let t = $derived((key: Parameters<typeof translate>[1]) =>
    translate(draft.locale, key),
  );

  onMount(() => {
    let disposed = false;
    let unlisteners: (() => void)[] = [];
    void (async () => {
      if ("__TAURI_INTERNALS__" in window) {
        const stops = await Promise.all([
          listen<AppSnapshot>("app-snapshot", (event) => {
            const wasConfigured = snapshot.configured;
            snapshot = event.payload;
            if (!snapshot.configured || !wasConfigured) {
              draft = { ...snapshot.settings };
            }
            if (!wasConfigured && snapshot.configured) active = "overview";
          }),
          listen<unknown>("navigate-view", (event) => {
            if (isViewName(event.payload)) active = event.payload;
          }),
        ]);
        if (disposed) stops.forEach((stop) => stop());
        else unlisteners = stops;
      }
      await perform(async () => {
        snapshot = await api.getAppSnapshot();
        draft = { ...snapshot.settings };
        if (!snapshot.configured) active = "setup";
      });
      loading = false;
    })();
    return () => {
      disposed = true;
      unlisteners.forEach((stop) => stop());
    };
  });

  async function perform(action: () => Promise<void>): Promise<void> {
    error = null;
    try {
      await action();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function chooseWorkspace(): Promise<void> {
    await perform(async () => {
      const path = await api.chooseWorkspace();
      if (path) draft.workspaceRoot = path;
    });
  }

  async function initialize(): Promise<void> {
    if (!draft.workspaceRoot) {
      error = t("selectFolder");
      return;
    }
    busy = true;
    await perform(async () => {
      snapshot = await api.initializeWorkspace($state.snapshot(draft));
      draft = { ...snapshot.settings };
    });
    busy = false;
  }

  async function cancelInitialization(): Promise<void> {
    await perform(async () => {
      snapshot = await api.cancelInitialization();
    });
  }

  async function preflight(): Promise<void> {
    busy = true;
    await perform(async () => {
      report = await api.runPreflight($state.snapshot(draft));
    });
    busy = false;
  }

  async function save(): Promise<void> {
    busy = true;
    await perform(async () => {
      snapshot = await api.applySettings(
        $state.snapshot(draft),
        mineruToken.trim().length > 0 ? "set" : "unchanged",
        mineruToken,
      );
      mineruToken = "";
      draft = { ...snapshot.settings };
    });
    busy = false;
  }

  async function clearMineruToken(): Promise<void> {
    busy = true;
    await perform(async () => {
      snapshot = await api.applySettings($state.snapshot(draft), "clear");
      mineruToken = "";
    });
    busy = false;
  }

  async function toggleWatching(): Promise<void> {
    await perform(async () => {
      snapshot = await api.setWatching(!snapshot.watching);
    });
  }

  async function updateJob(
    action: (id: string) => Promise<AppSnapshot>,
    id: string,
  ): Promise<void> {
    await perform(async () => {
      snapshot = await action(id);
    });
  }

  async function retryJob(id: string, mode: RetryMode): Promise<void> {
    await perform(async () => {
      snapshot = await api.retryJob(id, mode);
    });
  }
</script>

<svelte:head><title>{t("appName")}</title></svelte:head>

<AppSidebar
  {active}
  configured={snapshot.configured}
  watching={snapshot.watching}
  {t}
  onNavigate={(view) => (active = view)}
/>

<main>
  <header class="topbar">
    <div class="status">
      <span class:running={snapshot.configured && snapshot.watching}
      ></span>{snapshot.configured
        ? snapshot.watching
          ? t("watching")
          : t("paused")
        : t("notConfigured")}
    </div>
    <button
      class="secondary compact"
      type="button"
      disabled={!snapshot.configured || loading}
      onclick={toggleWatching}
    >
      {#if snapshot.watching}<Pause size={15} />{t("pause")}{:else}<Play
          size={15}
        />{t("resume")}{/if}
    </button>
  </header>

  {#if error}
    <div class="error-banner" role="alert">
      <AlertTriangle size={17} /><span>{t("commandFailed")} {error}</span
      ><button
        type="button"
        aria-label={t("cancel")}
        onclick={() => (error = null)}><X size={16} /></button
      >
    </div>
  {/if}

  <div class="content" class:loading>
    {#if active === "overview"}
      <OverviewView
        {snapshot}
        {t}
        onOpen={(target) => perform(() => api.openTarget(target))}
        onRetry={retryJob}
        onCancel={(id) => updateJob(api.cancelJob, id)}
      />
    {:else if active === "setup"}
      <SetupView
        settings={draft}
        tools={snapshot.tools}
        {report}
        {busy}
        initialization={snapshot.initialization}
        {t}
        onChoose={chooseWorkspace}
        onPreflight={preflight}
        onInitialize={initialize}
        onCancel={cancelInitialization}
      />
    {:else if active === "settings"}
      <SettingsView
        settings={draft}
        bind:mineruToken
        tokenConfigured={snapshot.mineruTokenConfigured}
        {busy}
        {t}
        onSave={save}
        onClearToken={clearMineruToken}
      />
    {:else}
      <LogsView
        entries={snapshot.logs}
        locale={draft.locale}
        {t}
        onExport={(includeDetailedLogs) =>
          perform(async () => {
            await api.exportDiagnostics(includeDetailedLogs);
          })}
      />
    {/if}
  </div>
</main>

<style>
  :global(*) {
    box-sizing: border-box;
  }
  :global(:root) {
    font-family:
      Inter,
      ui-sans-serif,
      -apple-system,
      BlinkMacSystemFont,
      "Segoe UI",
      sans-serif;
    color: #252a27;
    background: #f7f7f4;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    --text: #252a27;
    --muted: #68706a;
    --surface: #fff;
    --surface-subtle: #f1f2ef;
    --border: #dedfda;
    --border-strong: #c8cbc5;
    --accent: #3f7a4b;
  }
  :global(body) {
    min-width: 320px;
    min-height: 100vh;
    margin: 0;
    background: #f7f7f4;
  }
  :global(button),
  :global(input),
  :global(select) {
    font: inherit;
  }
  :global(button:focus-visible),
  :global(input:focus-visible),
  :global(select:focus-visible) {
    outline: 2px solid #5d8865;
    outline-offset: 2px;
  }
  :global(input),
  :global(select) {
    width: 100%;
    min-height: 40px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    padding: 0 11px;
    color: var(--text);
    background: var(--surface);
  }
  :global(input:read-only) {
    color: var(--muted);
    background: var(--surface-subtle);
  }
  :global(button.primary),
  :global(button.secondary),
  :global(button.icon-button) {
    display: inline-flex;
    min-height: 36px;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border-radius: 5px;
    padding: 0 12px;
    cursor: pointer;
  }
  :global(button.primary) {
    border: 1px solid #32673c;
    color: #fff;
    background: var(--accent);
  }
  :global(button.secondary) {
    border: 1px solid var(--border-strong);
    color: var(--text);
    background: var(--surface);
  }
  :global(button.icon-button) {
    width: 32px;
    min-height: 32px;
    border: 0;
    padding: 0;
    color: var(--muted);
    background: transparent;
  }
  :global(button:hover:not(:disabled)) {
    filter: brightness(0.96);
  }
  :global(button:disabled) {
    opacity: 0.48;
    cursor: not-allowed;
  }
  :global(.section-heading) {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 18px;
  }
  :global(.section-heading h1) {
    margin: 0;
    font-size: 24px;
    font-weight: 650;
    letter-spacing: 0;
  }
  :global(.section-heading p) {
    margin: 6px 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  main {
    min-height: 100vh;
    margin-left: 220px;
  }
  .topbar {
    display: flex;
    height: 58px;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    padding: 0 30px;
    border-bottom: 1px solid var(--border);
    background: rgba(247, 247, 244, 0.96);
  }
  .status {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--muted);
    font-size: 12px;
  }
  .status span {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #929893;
  }
  .status span.running {
    background: #5da969;
  }
  .compact {
    min-height: 30px !important;
    padding-inline: 9px !important;
    font-size: 12px;
  }
  .content {
    width: min(100%, 1120px);
    margin: 0 auto;
    padding: 42px 42px 80px;
    transition: opacity 120ms ease;
  }
  .content.loading {
    opacity: 0.55;
    pointer-events: none;
  }
  .error-banner {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
    margin: 16px 30px 0;
    border: 1px solid #e1b4ae;
    border-radius: 6px;
    padding: 10px 12px;
    color: #7e3531;
    background: #fff0ee;
    font-size: 12px;
  }
  .error-banner button {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  @media (max-width: 720px) {
    main {
      margin-left: 0;
      padding-bottom: 64px;
    }
    .topbar {
      height: 52px;
      padding-inline: 16px;
    }
    .content {
      padding: 28px 18px 60px;
    }
    .error-banner {
      margin-inline: 16px;
    }
  }
</style>
