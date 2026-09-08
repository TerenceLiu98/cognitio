import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppSnapshot,
  PreflightReport,
  RetryMode,
  ViewName,
} from "./types";

function plain(settings: AppSettings): AppSettings {
  return JSON.parse(JSON.stringify(settings)) as AppSettings;
}

export function getAppSnapshot(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("get_app_snapshot");
}

export function applySettings(
  settings: AppSettings,
  tokenAction: "unchanged" | "set" | "clear",
  token = "",
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("apply_settings", {
    settings: plain(settings),
    tokenAction,
    token,
  });
}

export function chooseWorkspace(): Promise<string | null> {
  return invoke<string | null>("choose_workspace");
}

export function initializeWorkspace(
  settings: AppSettings,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("initialize_workspace", {
    settings: plain(settings),
  });
}

export function cancelInitialization(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("cancel_initialization");
}

export function runPreflight(settings: AppSettings): Promise<PreflightReport> {
  return invoke<PreflightReport>("run_preflight", {
    settings: plain(settings),
  });
}

export function setWatching(watching: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("set_watching", { watching });
}

export function retryJob(
  jobId: string,
  retryMode: RetryMode,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("retry_job", { jobId, retryMode });
}

export function cancelJob(jobId: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>("cancel_job", { jobId });
}

export function openTarget(
  target: "inbox" | "wiki" | "website",
): Promise<void> {
  return invoke<void>("open_target", { target });
}

export function showAppView(view: ViewName): Promise<void> {
  return invoke<void>("show_app_view", { view });
}

export function hideMenuPanel(): Promise<void> {
  return invoke<void>("hide_menu_panel");
}

export function exportDiagnostics(
  includeDetailedLogs = false,
): Promise<string | null> {
  return invoke<string | null>("export_diagnostics", { includeDetailedLogs });
}
