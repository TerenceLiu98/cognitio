<script lang="ts">
  import { AlertTriangle, Pause, Play, X } from "@lucide/svelte";
  import { onMount } from "svelte";
  import AppSidebar from "$lib/components/AppSidebar.svelte";
  import LogsView from "$lib/components/LogsView.svelte";
  import MenuPanel from "$lib/components/MenuPanel.svelte";
  import OverviewView from "$lib/components/OverviewView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import SetupView from "$lib/components/SetupView.svelte";
  import { AppController } from "$lib/app-controller.svelte";
  import { translate } from "$lib/i18n";

  const state = new AppController();
  let snapshot = $derived(state.snapshot);
  let draft = $derived(state.draft);
  let active = $derived(state.active);
  let loading = $derived(state.loading);
  let busy = $derived(state.busy);
  let error = $derived(state.error);
  let report = $derived(state.report);
  const menuSurface =
    new URLSearchParams(globalThis.location?.search ?? "").get("surface") ===
    "menubar";
  let t = $derived((key: Parameters<typeof translate>[1]) =>
    translate(draft.locale, key),
  );
  onMount(() => state.mount(menuSurface));
</script>

<svelte:head><title>{t("appName")}</title></svelte:head>

{#if menuSurface}
  <MenuPanel
    {snapshot}
    {loading}
    {error}
    {t}
    onToggle={state.toggleWatching}
    onOpen={state.openFromMenu}
    onNavigate={state.navigateFromMenu}
  />
{:else}
  <AppSidebar
    {active}
    configured={snapshot.configured}
    watching={snapshot.watching}
    {t}
    onNavigate={(view) => (state.active = view)}
  />

  <main>
    <header class="topbar">
      <strong class="view-title">{t(active)}</strong>
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
        onclick={state.toggleWatching}
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
          onclick={() => (state.error = null)}><X size={16} /></button
        >
      </div>
    {/if}

    <div class="content" class:loading>
      {#if active === "overview"}
        <OverviewView
          {snapshot}
          {t}
          onOpen={state.openTarget}
          onRetry={state.retryJob}
          onCancel={state.cancelJob}
        />
      {:else if active === "setup"}
        <SetupView
          settings={draft}
          tools={snapshot.tools}
          {report}
          {busy}
          initialization={snapshot.initialization}
          {t}
          onChoose={state.chooseWorkspace}
          onPreflight={state.preflight}
          onInitialize={state.initialize}
          onCancel={state.cancelInitialization}
        />
      {:else if active === "settings"}
        <SettingsView
          settings={draft}
          bind:mineruToken={state.mineruToken}
          tokenConfigured={snapshot.mineruTokenConfigured}
          {busy}
          {t}
          onSave={state.save}
          onClearToken={state.clearMineruToken}
        />
      {:else}
        <LogsView
          entries={snapshot.logs}
          locale={draft.locale}
          {t}
          onExport={state.exportDiagnostics}
        />
      {/if}
    </div>
  </main>
{/if}

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
    color: #222423;
    background: #fff;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    --text: #222423;
    --muted: #6d716e;
    --surface: #fff;
    --surface-subtle: #f3f4f2;
    --border: #e0e2df;
    --border-strong: #c9ccc8;
    --accent: #0a6fe8;
    --positive: #34795a;
  }
  :global(body) {
    min-width: 320px;
    min-height: 100vh;
    margin: 0;
    background: #fff;
  }
  :global(button),
  :global(input),
  :global(select) {
    font: inherit;
  }
  :global(button:focus-visible),
  :global(input:focus-visible),
  :global(select:focus-visible) {
    outline: 2px solid #4a90e8;
    outline-offset: 2px;
  }
  :global(input),
  :global(select) {
    width: 100%;
    min-height: 34px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    padding: 0 11px;
    color: var(--text);
    background: #fbfbfa;
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
    border: 1px solid #0862cc;
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
    filter: brightness(0.97);
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
    font-size: 21px;
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
    margin-left: 196px;
  }
  .topbar {
    display: flex;
    height: 52px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 24px;
    border-bottom: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.96);
  }
  .view-title {
    font-size: 12px;
    font-weight: 620;
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
    background: #38a868;
  }
  .compact {
    min-height: 30px !important;
    padding-inline: 9px !important;
    font-size: 12px;
  }
  .content {
    width: min(100%, 980px);
    margin: 0 auto;
    padding: 30px 34px 64px;
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
