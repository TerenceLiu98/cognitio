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
  <p class="settings-note">{t("appliesToNewJobs")}</p>

  <div class="preference-stack">
    <section class="preference-group" aria-labelledby="general-settings">
      <h2 id="general-settings">{t("general")}</h2>
      <div class="preference-list">
        <label class="preference-row">
          <span class="row-label"><strong>{t("language")}</strong></span>
          <select bind:value={settings.locale}>
            <option value="en">{t("english")}</option>
            <option value="zh">{t("chinese")}</option>
          </select>
        </label>
        <label class="preference-row switch-row">
          <span class="row-label"><strong>{t("launchAtLogin")}</strong></span>
          <input
            class="switch-input"
            type="checkbox"
            bind:checked={settings.launchAtLogin}
          />
          <span class="switch" aria-hidden="true"></span>
        </label>
      </div>
    </section>

    <section class="preference-group" aria-labelledby="processing-settings">
      <h2 id="processing-settings">{t("processing")}</h2>
      <div class="preference-list">
        <label class="preference-row">
          <span class="row-label"><strong>{t("agent")}</strong></span>
          <select bind:value={settings.agentProvider}>
            <option value="auto">Auto</option>
            <option value="codex">Codex</option>
            <option value="claude">Claude Code</option>
            <option value="opencode">OpenCode</option>
          </select>
        </label>
        <label class="preference-row">
          <span class="row-label"><strong>{t("model")}</strong></span>
          <input bind:value={settings.model} placeholder={t("agentDefault")} />
        </label>
        <label class="preference-row">
          <span class="row-label">
            <strong>{t("parserMode")}</strong>
            <small>{t("mineruUploadNotice")}</small>
          </span>
          <select bind:value={settings.mineruMode}>
            <option value="precision">{t("precision")}</option>
            <option value="flash">{t("flash")}</option>
          </select>
        </label>
        <label class="preference-row">
          <span class="row-label"><strong>{t("afterProcessing")}</strong></span>
          <select bind:value={settings.afterProcessing}>
            <option value="keep">{t("keep")}</option>
            <option value="move_to_done">{t("moveToDone")}</option>
            <option value="trash">{t("trash")}</option>
          </select>
        </label>
        <div class="preference-row token-row">
          <label class="row-label" for="mineru-token">
            <strong>{t("mineruToken")}</strong>
            <small
              >{tokenConfigured ? t("tokenStored") : t("tokenNotStored")}</small
            >
          </label>
          <div class="token-control">
            <input
              id="mineru-token"
              type="password"
              autocomplete="off"
              bind:value={mineruToken}
              placeholder={tokenConfigured
                ? t("tokenPlaceholder")
                : t("tokenNotStored")}
            />
            {#if tokenConfigured}
              <button
                class="clear-token"
                type="button"
                onclick={onClearToken}
                title={t("clearToken")}
                aria-label={t("clearToken")}
              >
                <Trash2 size={15} />
              </button>
            {/if}
          </div>
        </div>
      </div>
    </section>

    <section class="preference-group" aria-labelledby="publishing-settings">
      <h2 id="publishing-settings">{t("publishing")}</h2>
      <div class="preference-list">
        <label class="preference-row">
          <span class="row-label">
            <strong>{t("siteTitle")}</strong>
            <small>{t("siteTitleHint")}</small>
          </span>
          <input
            bind:value={settings.siteTitle}
            maxlength="80"
            placeholder="Research Library"
          />
        </label>
        <label class="preference-row">
          <span class="row-label">
            <strong>{t("gitRemote")}</strong>
            <small>{t("repositoryLocked")}</small>
          </span>
          <input
            bind:value={settings.gitRemote}
            readonly
            placeholder={t("gitRemoteHint")}
          />
        </label>
        <label class="preference-row">
          <span class="row-label"><strong>{t("websiteUrl")}</strong></span>
          <input
            type="url"
            bind:value={settings.siteUrl}
            placeholder="https://wiki.example.com"
          />
        </label>
      </div>
    </section>
  </div>

  <div class="footer-actions">
    <button class="primary" type="button" disabled={busy} onclick={onSave}>
      <Save size={16} />{busy ? t("saving") : t("save")}
    </button>
  </div>
</section>

<style>
  .settings-note {
    margin: 10px 0 0;
    color: #8b622e;
    font-size: 11px;
  }
  .preference-stack {
    display: grid;
    gap: 24px;
    margin-top: 24px;
  }
  .preference-group h2 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 650;
  }
  .preference-list {
    overflow: hidden;
    border-radius: 8px;
    background: #f5f6f4;
  }
  .preference-row {
    display: grid;
    min-height: 56px;
    grid-template-columns: minmax(180px, 1fr) minmax(220px, 42%);
    align-items: center;
    gap: 24px;
    padding: 10px 14px;
    border-bottom: 1px solid #dfe1de;
  }
  .preference-row:last-child {
    border-bottom: 0;
  }
  .row-label {
    display: grid;
    min-width: 0;
    gap: 3px;
    color: var(--text);
  }
  .row-label strong {
    font-size: 12px;
    font-weight: 540;
  }
  .row-label small {
    color: var(--muted);
    font-size: 10px;
    line-height: 1.35;
  }
  .preference-row input,
  .preference-row select {
    justify-self: end;
  }
  .switch-row {
    position: relative;
    grid-template-columns: 1fr auto;
    cursor: pointer;
  }
  .switch-input {
    position: absolute;
    width: 1px !important;
    height: 1px;
    opacity: 0;
  }
  .switch {
    position: relative;
    width: 34px;
    height: 20px;
    border-radius: 10px;
    background: #c8cbc8;
    transition: background 120ms ease;
  }
  .switch::after {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.18);
    content: "";
    transition: transform 120ms ease;
  }
  .switch-input:checked + .switch {
    background: var(--accent);
  }
  .switch-input:checked + .switch::after {
    transform: translateX(14px);
  }
  .switch-input:focus-visible + .switch {
    outline: 2px solid #4a90e8;
    outline-offset: 2px;
  }
  .token-control {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 30px;
    align-items: center;
    gap: 5px;
  }
  .clear-token {
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    border: 0;
    border-radius: 5px;
    padding: 0;
    color: #9a3f39;
    background: transparent;
    cursor: pointer;
  }
  .clear-token:hover {
    background: #f3dfdc;
  }
  .footer-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 22px;
  }
  @media (max-width: 720px) {
    .preference-row {
      grid-template-columns: 1fr;
      gap: 8px;
    }
    .preference-row input,
    .preference-row select {
      justify-self: stretch;
    }
    .switch-row {
      grid-template-columns: 1fr auto;
    }
  }
</style>
