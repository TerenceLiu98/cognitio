export type Locale = "en" | "zh";
export type ViewName = "overview" | "setup" | "settings" | "logs";
export type AgentProvider = "auto" | "codex" | "claude" | "opencode";
export type MineruMode = "precision" | "flash";
export type AfterProcessing = "keep" | "move_to_done" | "trash";
export type HostingProvider = "cloudflare" | "github_pages";
export type RepositoryVisibility = "private" | "public";
export type JobState =
  | "detected"
  | "stabilizing"
  | "queued"
  | "preflight"
  | "running"
  | "verifying"
  | "succeeded"
  | "blocked"
  | "failed"
  | "cancelled";

export interface AppSettings {
  schemaVersion: number;
  locale: Locale;
  workspaceRoot: string;
  launchAtLogin: boolean;
  afterProcessing: AfterProcessing;
  agentProvider: AgentProvider;
  model: string | null;
  mineruMode: MineruMode;
  repositoryVisibility: RepositoryVisibility;
  gitRemote: string;
  hostingProvider: HostingProvider;
  siteUrl: string;
}

export interface ToolCapability {
  id: "git" | "gh" | "node" | "codex" | "claude" | "opencode";
  detected: boolean;
  authenticated: boolean | null;
  version: string | null;
  detail: string | null;
}

export interface JobSummary {
  id: string;
  filename: string;
  state: JobState;
  phase: string;
  progress: number;
  agent: Exclude<AgentProvider, "auto"> | null;
  createdAt: string;
  updatedAt: string;
  error: string | null;
}

export interface LogEntry {
  id: string;
  timestamp: string;
  level: "info" | "warn" | "error";
  message: string;
  jobId: string | null;
}

export interface AppSnapshot {
  configured: boolean;
  watching: boolean;
  mineruTokenConfigured: boolean;
  settings: AppSettings;
  tools: ToolCapability[];
  jobs: JobSummary[];
  logs: LogEntry[];
}

export interface PreflightItem {
  id: string;
  ok: boolean;
  message: string;
  detail: string | null;
}

export interface PreflightReport {
  ready: boolean;
  items: PreflightItem[];
}

export const defaultSettings: AppSettings = {
  schemaVersion: 1,
  locale: "en",
  workspaceRoot: "",
  launchAtLogin: false,
  afterProcessing: "move_to_done",
  agentProvider: "auto",
  model: null,
  mineruMode: "precision",
  repositoryVisibility: "private",
  gitRemote: "",
  hostingProvider: "cloudflare",
  siteUrl: "",
};

export const emptySnapshot: AppSnapshot = {
  configured: false,
  watching: false,
  mineruTokenConfigured: false,
  settings: defaultSettings,
  tools: [],
  jobs: [],
  logs: [],
};
