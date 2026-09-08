<script lang="ts">
  import { BookOpenText, FileCog, House, Logs, Settings } from "@lucide/svelte";

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
    <span class="brand-mark"><BookOpenText size={16} /></span>
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
    width: 196px;
    flex-direction: column;
    padding: 18px 12px 14px;
    border-right: 1px solid #d8dad7;
    color: #242625;
    background: #e9eae8;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 42px;
    padding: 0 8px 18px;
    font-size: 14px;
    font-weight: 650;
  }

  .brand-mark {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border-radius: 7px;
    color: #fff;
    background: #34795a;
  }

  nav {
    display: grid;
    gap: 3px;
  }

  nav button {
    display: flex;
    min-height: 36px;
    align-items: center;
    gap: 10px;
    border: 0;
    border-radius: 6px;
    padding: 0 11px;
    color: #4f5451;
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  nav button:hover,
  nav button.active {
    color: #fff;
    background: #0a6fe8;
  }

  nav button:hover:not(.active) {
    color: #202322;
    background: #dcdfdc;
  }

  .connection {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: auto;
    padding: 12px 9px 0;
    border-top: 1px solid #d2d4d1;
    color: #656a67;
    font-size: 12px;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: 0 0 auto;
    border-radius: 50%;
    background: #989d99;
  }

  .dot.running {
    background: #38a868;
  }

  @media (max-width: 720px) {
    .sidebar {
      inset: auto 0 0;
      width: auto;
      height: 64px;
      flex-direction: row;
      padding: 7px 10px;
      border-top: 1px solid #d2d4d1;
      border-right: 0;
      background: rgba(240, 241, 239, 0.98);
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

    nav button:hover:not(.active) {
      background: #dcdfdc;
    }
  }
</style>
