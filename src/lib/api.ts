import type {
  AppSettings,
  AppSnapshot,
  PreflightReport,
  RetryMode,
  ViewName,
} from "./types";

function backend() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
    ? import("./tauri-api")
    : import("./browser-api");
}

export async function getAppSnapshot(): Promise<AppSnapshot> {
  return (await backend()).getAppSnapshot();
}

export async function applySettings(
  settings: AppSettings,
  tokenAction: "unchanged" | "set" | "clear",
  token = "",
): Promise<AppSnapshot> {
  return (await backend()).applySettings(settings, tokenAction, token);
}

export async function chooseWorkspace(): Promise<string | null> {
  return (await backend()).chooseWorkspace();
}

export async function initializeWorkspace(
  settings: AppSettings,
): Promise<AppSnapshot> {
  return (await backend()).initializeWorkspace(settings);
}

export async function cancelInitialization(): Promise<AppSnapshot> {
  return (await backend()).cancelInitialization();
}

export async function runPreflight(
  settings: AppSettings,
): Promise<PreflightReport> {
  return (await backend()).runPreflight(settings);
}

export async function setWatching(watching: boolean): Promise<AppSnapshot> {
  return (await backend()).setWatching(watching);
}

export async function retryJob(
  jobId: string,
  retryMode: RetryMode,
): Promise<AppSnapshot> {
  return (await backend()).retryJob(jobId, retryMode);
}

export async function cancelJob(jobId: string): Promise<AppSnapshot> {
  return (await backend()).cancelJob(jobId);
}

export async function openTarget(
  target: "inbox" | "wiki" | "website",
): Promise<void> {
  return (await backend()).openTarget(target);
}

export async function showAppView(view: ViewName): Promise<void> {
  return (await backend()).showAppView(view);
}

export async function hideMenuPanel(): Promise<void> {
  return (await backend()).hideMenuPanel();
}

export async function exportDiagnostics(
  includeDetailedLogs = false,
): Promise<string | null> {
  return (await backend()).exportDiagnostics(includeDetailedLogs);
}
