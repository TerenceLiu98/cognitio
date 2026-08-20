import { invoke } from "@tauri-apps/api/core";

import { detectLocale } from "./i18n";
import {
  defaultSettings,
  type AppSettings,
  type AppSnapshot,
  type PreflightReport,
} from "./types";

const browserSnapshot: AppSnapshot = {
  ready: true,
  configured: false,
  watching: false,
  mineruTokenConfigured: false,
  settings: { ...defaultSettings, locale: detectLocale() },
  tools: [
    {
      id: "git",
      detected: true,
      authenticated: null,
      version: "2.x",
      detail: null,
    },
    {
      id: "gh",
      detected: false,
      authenticated: false,
      version: null,
      detail: null,
    },
    {
      id: "node",
      detected: true,
      authenticated: null,
      version: "24.x",
      detail: null,
    },
    {
      id: "codex",
      detected: true,
      authenticated: true,
      version: null,
      detail: null,
    },
    {
      id: "claude",
      detected: false,
      authenticated: null,
      version: null,
      detail: null,
    },
    {
      id: "opencode",
      detected: false,
      authenticated: null,
      version: null,
      detail: null,
    },
  ],
  jobs: [],
  logs: [],
  initialization: {
    operationId: null,
    state: "idle",
    phase: null,
    progress: 0,
    message: null,
    error: null,
    canRetry: false,
  },
};

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function plainSettings(settings: AppSettings): AppSettings {
  return JSON.parse(JSON.stringify(settings)) as AppSettings;
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>(command, args);
}

export async function getAppSnapshot(): Promise<AppSnapshot> {
  return isTauriRuntime()
    ? call<AppSnapshot>("get_app_snapshot")
    : structuredClone(browserSnapshot);
}

export async function saveSettings(
  settings: AppSettings,
): Promise<AppSnapshot> {
  const plain = plainSettings(settings);
  if (isTauriRuntime())
    return call<AppSnapshot>("save_settings", { settings: plain });
  browserSnapshot.settings = plain;
  return structuredClone(browserSnapshot);
}

export async function saveMineruToken(token: string): Promise<AppSnapshot> {
  if (isTauriRuntime())
    return call<AppSnapshot>("save_mineru_token", { token });
  browserSnapshot.mineruTokenConfigured = token.trim().length > 0;
  return structuredClone(browserSnapshot);
}

export async function chooseWorkspace(): Promise<string | null> {
  return isTauriRuntime() ? call<string | null>("choose_workspace") : null;
}

export async function initializeWorkspace(
  settings: AppSettings,
): Promise<AppSnapshot> {
  const plain = plainSettings(settings);
  if (isTauriRuntime())
    return call<AppSnapshot>("initialize_workspace", { settings: plain });
  browserSnapshot.settings = plain;
  browserSnapshot.initialization = {
    operationId: crypto.randomUUID(),
    state: "running",
    phase: "template",
    progress: 30,
    message: "Installing bundled Quartz template",
    error: null,
    canRetry: false,
  };
  return structuredClone(browserSnapshot);
}

export async function cancelInitialization(): Promise<AppSnapshot> {
  if (isTauriRuntime()) return call<AppSnapshot>("cancel_initialization");
  browserSnapshot.initialization.state = "cancelled";
  browserSnapshot.initialization.message = "Initialization cancelled";
  browserSnapshot.initialization.canRetry = true;
  return structuredClone(browserSnapshot);
}

export async function runPreflight(
  settings: AppSettings,
): Promise<PreflightReport> {
  const plain = plainSettings(settings);
  if (isTauriRuntime())
    return call<PreflightReport>("run_preflight", { settings: plain });
  return {
    ready: Boolean(plain.workspaceRoot),
    items: [
      {
        id: "workspace",
        ok: Boolean(plain.workspaceRoot),
        message: "Workspace",
        detail: null,
      },
    ],
  };
}

export async function setWatching(watching: boolean): Promise<AppSnapshot> {
  if (isTauriRuntime()) return call<AppSnapshot>("set_watching", { watching });
  browserSnapshot.watching = watching;
  return structuredClone(browserSnapshot);
}

export async function retryJob(jobId: string): Promise<AppSnapshot> {
  return isTauriRuntime()
    ? call<AppSnapshot>("retry_job", { jobId })
    : structuredClone(browserSnapshot);
}

export async function cancelJob(jobId: string): Promise<AppSnapshot> {
  return isTauriRuntime()
    ? call<AppSnapshot>("cancel_job", { jobId })
    : structuredClone(browserSnapshot);
}

export async function openTarget(
  target: "inbox" | "wiki" | "website",
): Promise<void> {
  if (isTauriRuntime()) await call<void>("open_target", { target });
}

export async function exportDiagnostics(): Promise<string | null> {
  return isTauriRuntime() ? call<string | null>("export_diagnostics") : null;
}
