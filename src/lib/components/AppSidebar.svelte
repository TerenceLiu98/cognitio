<script lang="ts">
  import { FileCog, House, Logs, Settings } from "@lucide/svelte";

  import type { Translator } from "$lib/i18n";
  import type { ViewName } from "$lib/types";

  let {
    active,
    configured,
    watching,
    t,
    onNavigate,
  }: {
    active: ViewName;
    configured: boolean;
    watching: boolean;
    t: Translator;
    onNavigate: (view: ViewName) => void;
  } = $props();

  const items: { id: ViewName; icon: typeof House }[] = [
    { id: "overview", icon: House },
    { id: "setup", icon: FileCog },
    { id: "settings", icon: Settings },
    { id: "logs", icon: Logs },
  ];
</script>

<aside class="sidebar" aria-label="Primary navigation">
  <div class="brand">
    <span class="brand-mark">LW</span>
    <span>{t("appName")}</span>
  </div>

  <nav>
    {#each items as item}
      <button
        type="button"
        class:active={active === item.id}
        aria-current={active === item.id ? "page" : undefined}
        onclick={() => onNavigate(item.id)}
      >
        <item.icon size={18} strokeWidth={1.8} />
        <span>{t(item.id)}</span>
      </button>
    {/each}
  </nav>

  <div class="connection">
    <span class:running={configured && watching} class="dot"></span>
    <span
      >{configured
        ? watching
          ? t("watching")
          : t("paused")
        : t("notConfigured")}</span
    >
  </div>
</aside>

<style>
  .sidebar {
    position: fixed;
    inset: 0 auto 0 0;
    z-index: 10;
    display: flex;
    width: 220px;
    flex-direction: column;
    padding: 24px 16px 18px;
    color: #f5f5f2;
    background: #202522;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px 24px;
    font-size: 16px;
    font-weight: 680;
  }

  .brand-mark {
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    border: 1px solid #626c64;
    border-radius: 6px;
    color: #b8d8bd;
    font-size: 11px;
  }

  nav {
    display: grid;
    gap: 4px;
  }

  nav button {
    display: flex;
    min-height: 40px;
    align-items: center;
    gap: 11px;
    border: 0;
    border-radius: 6px;
    padding: 0 11px;
    color: #b9bfba;
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  nav button:hover,
  nav button.active {
    color: #fff;
    background: #343b36;
  }

  .connection {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: auto;
    padding: 12px 9px 0;
    border-top: 1px solid #3b413d;
    color: #aeb4af;
    font-size: 12px;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: 0 0 auto;
    border-radius: 50%;
    background: #7e8780;
  }

  .dot.running {
    background: #70b879;
  }

  @media (max-width: 720px) {
    .sidebar {
      inset: auto 0 0;
      width: auto;
      height: 64px;
      flex-direction: row;
      padding: 7px 10px;
      border-top: 1px solid #3b413d;
    }

    .brand,
    .connection {
      display: none;
    }

    nav {
      display: grid;
      width: 100%;
      grid-template-columns: repeat(4, 1fr);
      gap: 2px;
    }

    nav button {
      min-width: 0;
      height: 50px;
      flex-direction: column;
      justify-content: center;
      gap: 2px;
      padding: 0 3px;
      font-size: 10px;
    }
  }
</style>
