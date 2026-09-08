import { detectLocale } from "./i18n";
import {
  defaultSettings,
  type AppSettings,
  type AppSnapshot,
  type PreflightReport,
} from "./types";

const browserSnapshot: AppSnapshot = {
  revision: 0,
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

if (import.meta.env.MODE === "e2e") {
  browserSnapshot.configured = true;
  browserSnapshot.watching = true;
  browserSnapshot.settings = {
    ...browserSnapshot.settings,
    workspaceRoot: "/tmp/cognitio-e2e",
    gitRemote: "owner/research-library",
    siteUrl: "https://owner.github.io/research-library/",
  };
  browserSnapshot.jobs = [
    {
      id: "running-job",
      revision: 0,
      stage: "generating",
      blockReason: null,
      filename: "zmag007.pdf",
      state: "running",
      phase: "Generating linked Paper and Concept pages",
      progress: 42,
      agent: "codex",
      createdAt: "2026-08-20T00:05:00Z",
      updatedAt: "2026-08-20T00:12:00Z",
      error: null,
      allowedActions: [],
      blockScope: null,
      mineruMode: "precision",
      deployment: {
        status: "notTracked",
        url: null,
        error: null,
        updatedAt: null,
      },
    },
    {
      id: "blocked-job",
      revision: 0,
      stage: "publishing",
      blockReason: "remoteUnavailable",
      filename: "paper.pdf",
      state: "blocked",
      phase: "Task commit exists but push failed",
      progress: 85,
      agent: "codex",
      createdAt: "2026-08-20T00:00:00Z",
      updatedAt: "2026-08-20T00:00:00Z",
      error: "Retry to resume Git push",
      allowedActions: ["retry", "reparse"],
      blockScope: "job",
      mineruMode: "precision",
      deployment: {
        status: "failed",
        url: "https://github.com/owner/research-library/actions",
        error: "GitHub Pages concluded with failure",
        updatedAt: "2026-08-20T00:10:00Z",
      },
    },
  ];
}

function current(): AppSnapshot {
  return structuredClone(browserSnapshot);
}
function changed(): AppSnapshot {
  browserSnapshot.revision += 1;
  return current();
}

export async function getAppSnapshot(): Promise<AppSnapshot> {
  return current();
}
export async function chooseWorkspace(): Promise<string | null> {
  return null;
}
export async function applySettings(
  settings: AppSettings,
  tokenAction: "unchanged" | "set" | "clear",
  _token = "",
): Promise<AppSnapshot> {
  browserSnapshot.settings = structuredClone(settings);
  if (tokenAction !== "unchanged")
    browserSnapshot.mineruTokenConfigured = tokenAction === "set";
  return changed();
}
export async function initializeWorkspace(
  settings: AppSettings,
): Promise<AppSnapshot> {
  browserSnapshot.settings = structuredClone(settings);
  browserSnapshot.initialization = {
    operationId: crypto.randomUUID(),
    state: "running",
    phase: "template",
    progress: 30,
    message: "Installing bundled Quartz template",
    error: null,
    canRetry: false,
  };
  return changed();
}
export async function cancelInitialization(): Promise<AppSnapshot> {
  browserSnapshot.initialization = {
    ...browserSnapshot.initialization,
    state: "cancelled",
    message: "Initialization cancelled",
    canRetry: true,
  };
  return changed();
}
export async function runPreflight(
  settings: AppSettings,
): Promise<PreflightReport> {
  return {
    ready: Boolean(settings.workspaceRoot),
    items: [
      {
        id: "workspace",
        ok: Boolean(settings.workspaceRoot),
        message: "Workspace",
        detail: null,
      },
    ],
  };
}
export async function setWatching(watching: boolean): Promise<AppSnapshot> {
  browserSnapshot.watching = watching;
  return changed();
}
export async function retryJob(..._args: unknown[]): Promise<AppSnapshot> {
  return current();
}
export async function cancelJob(..._args: unknown[]): Promise<AppSnapshot> {
  return current();
}
export async function openTarget(..._args: unknown[]): Promise<void> {}
export async function showAppView(..._args: unknown[]): Promise<void> {}
export async function hideMenuPanel(): Promise<void> {}
export async function exportDiagnostics(
  ..._args: unknown[]
): Promise<string | null> {
  return null;
}
