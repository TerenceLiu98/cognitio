import { listen } from "@tauri-apps/api/event";
import * as api from "./api";
import { translate } from "./i18n";
import { isViewName } from "./navigation";
import {
  emptySnapshot,
  type AppSettings,
  type AppSnapshot,
  type PreflightReport,
  type RetryMode,
  type ViewName,
} from "./types";

function sameSettings(left: AppSettings, right: AppSettings): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

export class AppController {
  snapshot = $state<AppSnapshot>(structuredClone(emptySnapshot));
  draft = $state<AppSettings>({ ...emptySnapshot.settings });
  active = $state<ViewName>("overview");
  loading = $state(true);
  error = $state<string | null>(null);
  report = $state<PreflightReport | null>(null);
  mineruToken = $state("");
  private baseline: AppSettings = { ...emptySnapshot.settings };
  private pending = $state<string[]>([]);
  busy = $derived(this.pending.length > 0);

  constructor(private readonly backend: typeof api = api) {}

  accept(next: AppSnapshot): boolean {
    if (next.revision < this.snapshot.revision) return false;
    const wasConfigured = this.snapshot.configured;
    const clean = sameSettings(this.draft, this.baseline);
    this.snapshot = next;
    if (clean) this.resetDraft();
    if (!wasConfigured && next.configured) this.active = "overview";
    return true;
  }

  private resetDraft(): void {
    this.draft = { ...this.snapshot.settings };
    this.baseline = { ...this.snapshot.settings };
  }

  private acknowledge(sent: AppSettings): void {
    const current = $state.snapshot(this.draft);
    const saved = this.snapshot.settings;
    this.draft = Object.fromEntries(
      (Object.keys(saved) as (keyof AppSettings)[]).map((key) => [
        key,
        current[key] === sent[key] ? saved[key] : current[key],
      ]),
    ) as unknown as AppSettings;
    this.baseline = { ...saved };
  }

  mount(menuSurface: boolean): () => void {
    let disposed = false;
    const stops: (() => void)[] = [];
    const register = (stop: () => void) =>
      disposed ? stop() : stops.push(stop);
    void this.perform(async () => {
      try {
        if ("__TAURI_INTERNALS__" in window) {
          register(
            await listen<AppSnapshot>("app-snapshot", (event) => {
              if (!disposed) this.accept(event.payload);
            }),
          );
          register(
            await listen<unknown>("navigate-view", (event) => {
              if (!disposed && isViewName(event.payload))
                this.active = event.payload;
            }),
          );
        }
        const initial = await this.backend.getAppSnapshot();
        if (!disposed) {
          this.accept(initial);
          if (!this.snapshot.configured) this.active = "setup";
        }
      } finally {
        this.loading = false;
      }
    });
    const dismiss = (event: KeyboardEvent) => {
      if (menuSurface && event.key === "Escape")
        void this.backend.hideMenuPanel();
    };
    globalThis.addEventListener("keydown", dismiss);
    return () => {
      disposed = true;
      stops.forEach((stop) => stop());
      globalThis.removeEventListener("keydown", dismiss);
    };
  }

  private async execute(
    key: string,
    action: () => Promise<void>,
  ): Promise<void> {
    if (this.pending.includes(key)) return;
    this.pending = [...this.pending, key];
    this.error = null;
    try {
      await action();
    } catch (cause) {
      this.error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      this.pending = this.pending.filter((pending) => pending !== key);
    }
  }

  perform = (action: () => Promise<void>): Promise<void> =>
    this.execute("command", action);

  chooseWorkspace = (): Promise<void> =>
    this.perform(async () => {
      const path = await this.backend.chooseWorkspace();
      if (path) this.draft.workspaceRoot = path;
    });

  initialize = (): Promise<void> =>
    this.execute("initialization", async () => {
      if (!this.draft.workspaceRoot)
        throw new Error(translate(this.draft.locale, "selectFolder"));
      const sent = $state.snapshot(this.draft);
      this.accept(await this.backend.initializeWorkspace(sent));
      this.acknowledge(sent);
    });

  cancelInitialization = (): Promise<void> =>
    this.execute("cancel-initialization", async () => {
      this.accept(await this.backend.cancelInitialization());
    });

  preflight = (): Promise<void> =>
    this.execute("preflight", async () => {
      this.report = await this.backend.runPreflight(
        $state.snapshot(this.draft),
      );
    });

  save = (): Promise<void> =>
    this.execute("settings", async () => {
      const sent = $state.snapshot(this.draft);
      const token = this.mineruToken;
      this.accept(
        await this.backend.applySettings(
          sent,
          token.trim() ? "set" : "unchanged",
          token,
        ),
      );
      if (this.mineruToken === token) this.mineruToken = "";
      this.acknowledge(sent);
    });

  clearMineruToken = (): Promise<void> =>
    this.execute("settings", async () => {
      const sent = $state.snapshot(this.draft);
      this.accept(await this.backend.applySettings(sent, "clear"));
      this.mineruToken = "";
      this.acknowledge(sent);
    });

  toggleWatching = (): Promise<void> =>
    this.execute("watching", async () => {
      this.accept(await this.backend.setWatching(!this.snapshot.watching));
    });

  cancelJob = (id: string): Promise<void> =>
    this.execute(`job:${id}`, async () => {
      this.accept(await this.backend.cancelJob(id));
    });

  retryJob = (id: string, mode: RetryMode): Promise<void> =>
    this.execute(`job:${id}`, async () => {
      this.accept(await this.backend.retryJob(id, mode));
    });

  openTarget = (target: "inbox" | "wiki" | "website"): Promise<void> =>
    this.perform(() => this.backend.openTarget(target));

  openFromMenu = (target: "inbox" | "wiki" | "website"): Promise<void> =>
    this.perform(async () => {
      await this.backend.openTarget(target);
      await this.backend.hideMenuPanel();
    });

  navigateFromMenu = (view: ViewName): Promise<void> =>
    this.perform(() => this.backend.showAppView(view));

  exportDiagnostics = (includeDetailedLogs: boolean): Promise<void> =>
    this.perform(async () => {
      await this.backend.exportDiagnostics(includeDetailedLogs);
    });
}
