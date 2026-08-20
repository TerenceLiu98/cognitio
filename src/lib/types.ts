export type Locale = "en" | "zh";
export type ViewName = "overview" | "setup" | "settings" | "logs";
export type AgentProvider = "auto" | "codex" | "claude" | "opencode";
export type MineruMode = "precision" | "flash";
export type AfterProcessing = "keep" | "move_to_done" | "trash";
export type RepositoryVisibility = "private" | "public";
export type InitializationState =
  "idle" | "running" | "succeeded" | "failed" | "cancelled" | "interrupted";
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
  siteTitle: string;
  siteUrl: string;
}

export interface InitializationSummary {
  operationId: string | null;
  state: InitializationState;
  phase: string | null;
  progress: number;
  message: string | null;
  error: string | null;
  canRetry: boolean;
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
  ready: boolean;
  configured: boolean;
  watching: boolean;
  mineruTokenConfigured: boolean;
  settings: AppSettings;
  tools: ToolCapability[];
  jobs: JobSummary[];
  logs: LogEntry[];
  initialization: InitializationSummary;
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
  schemaVersion: 3,
  locale: "en",
  workspaceRoot: "",
  launchAtLogin: false,
  afterProcessing: "move_to_done",
  agentProvider: "auto",
  model: null,
  mineruMode: "precision",
  repositoryVisibility: "private",
  gitRemote: "",
  siteTitle: "Research Library",
  siteUrl: "",
};

export const emptySnapshot: AppSnapshot = {
  ready: false,
  configured: false,
  watching: false,
  mineruTokenConfigured: false,
  settings: defaultSettings,
  tools: [],
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
