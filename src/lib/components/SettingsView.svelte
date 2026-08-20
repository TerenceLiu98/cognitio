<script lang="ts">
  import { Save, Trash2 } from "@lucide/svelte";
  import type { Translator } from "$lib/i18n";
  import type { AppSettings } from "$lib/types";

  let {
    settings = $bindable(),
    mineruToken = $bindable(),
    tokenConfigured,
    busy,
    t,
    onSave,
    onClearToken,
  }: {
    settings: AppSettings;
    mineruToken: string;
    tokenConfigured: boolean;
    busy: boolean;
    t: Translator;
    onSave: () => void;
    onClearToken: () => void;
  } = $props();
</script>

<section aria-labelledby="settings-heading">
  <div class="section-heading">
    <div>
      <h1 id="settings-heading">{t("settings")}</h1>
      <p>{t("privacyNotice")}</p>
    </div>
  </div>
  <div class="settings-form">
    <label
      >{t("language")}<select bind:value={settings.locale}
        ><option value="en">{t("english")}</option><option value="zh"
          >{t("chinese")}</option
        ></select
      ></label
    >
    <label
      >{t("agent")}<select bind:value={settings.agentProvider}
        ><option value="auto">Auto</option><option value="codex">Codex</option
        ><option value="claude">Claude Code</option><option value="opencode"
          >OpenCode</option
        ></select
      ></label
    >
    <label
      >{t("model")}<input
        bind:value={settings.model}
        placeholder={t("agentDefault")}
      /></label
    >
    <label
      >{t("parserMode")}<select bind:value={settings.mineruMode}
        ><option value="precision">{t("precision")}</option><option
          value="flash">{t("flash")}</option
        ></select
      ><small>{t("mineruUploadNotice")}</small></label
    >
    <label
      >{t("mineruToken")}<input
        type="password"
        autocomplete="off"
        bind:value={mineruToken}
        placeholder={tokenConfigured
          ? t("tokenPlaceholder")
          : t("tokenNotStored")}
      /><small>{tokenConfigured ? t("tokenStored") : t("tokenNotStored")}</small
      >{#if tokenConfigured}<button
          class="clear-token"
          type="button"
          onclick={onClearToken}
          title={t("clearToken")}
          aria-label={t("clearToken")}
          ><Trash2 size={15} />{t("clearToken")}</button
        >{/if}</label
    >
    <label
      >{t("afterProcessing")}<select bind:value={settings.afterProcessing}
        ><option value="keep">{t("keep")}</option><option value="move_to_done"
          >{t("moveToDone")}</option
        ><option value="trash">{t("trash")}</option></select
      ></label
    >
    <label class="wide"
      >{t("siteTitle")}<input
        bind:value={settings.siteTitle}
        maxlength="80"
        placeholder="Research Library"
      /><small>{t("siteTitleHint")}</small></label
    >
    <label class="wide"
      >{t("gitRemote")}<input
        bind:value={settings.gitRemote}
        placeholder={t("gitRemoteHint")}
      /></label
    >
    <label class="wide"
      >{t("websiteUrl")}<input
        type="url"
        bind:value={settings.siteUrl}
        placeholder="https://wiki.example.com"
      /></label
    >
    <label class="check-row"
      ><input type="checkbox" bind:checked={settings.launchAtLogin} /><span
        >{t("launchAtLogin")}</span
      ></label
    >
  </div>
  <div class="footer-actions">
    <button class="primary" type="button" disabled={busy} onclick={onSave}
      ><Save size={16} />{busy ? t("saving") : t("save")}</button
    >
  </div>
</section>

<style>
  .settings-form {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 20px;
    margin-top: 30px;
    padding-block: 24px;
    border-block: 1px solid var(--border);
  }
  label {
    display: grid;
    gap: 7px;
    color: var(--muted);
    font-size: 12px;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .check-row {
    display: flex;
    grid-column: 1 / -1;
    flex-direction: row;
    align-items: center;
    gap: 9px;
    color: var(--text);
  }
  .check-row input {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
  }
  .clear-token {
    display: inline-flex;
    width: fit-content;
    align-items: center;
    gap: 6px;
    border: 0;
    padding: 2px 0;
    color: #8a3838;
    background: transparent;
    font-size: 11px;
    cursor: pointer;
  }
  .footer-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 28px;
  }
  @media (max-width: 650px) {
    .settings-form {
      grid-template-columns: 1fr;
    }
    .wide,
    .check-row {
      grid-column: auto;
    }
  }
</style>
