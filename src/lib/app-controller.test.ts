import { describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { AppController } from "./app-controller.svelte";
import { emptySnapshot, type AppSnapshot } from "./types";

function snapshot(revision: number): AppSnapshot {
  return { ...structuredClone(emptySnapshot), ready: true, revision };
}

describe("AppController", () => {
  it("ignores older responses and preserves setup edits across background events", () => {
    const app = new AppController();
    app.accept(snapshot(2));
    app.draft.siteTitle = "Unsaved library";
    const update = snapshot(4);
    update.initialization.state = "running";
    app.accept(update);
    expect(app.draft.siteTitle).toBe("Unsaved library");
    expect(app.accept(snapshot(3))).toBe(false);
    expect(app.snapshot.initialization.state).toBe("running");
    expect(app.snapshot.revision).toBe(4);
  });

  it("refreshes an unchanged draft when settings change remotely", () => {
    const app = new AppController();
    const update = snapshot(1);
    update.settings.locale = "zh";
    app.accept(update);
    expect(app.draft.locale).toBe("zh");
  });

  it("deduplicates saves and preserves edits made while a save is pending", async () => {
    let finish!: (snapshot: AppSnapshot) => void;
    const applySettings = vi.fn(
      () =>
        new Promise<AppSnapshot>((resolve) => {
          finish = resolve;
        }),
    );
    const app = new AppController({ ...api, applySettings });
    app.accept(snapshot(1));
    app.draft.siteTitle = "Submitted title";
    const saving = app.save();
    await app.save();
    expect(applySettings).toHaveBeenCalledTimes(1);
    app.draft.siteTitle = "New unsaved title";
    const saved = snapshot(2);
    saved.settings.siteTitle = "Submitted title";
    finish(saved);
    await saving;
    expect(app.snapshot.settings.siteTitle).toBe("Submitted title");
    expect(app.draft.siteTitle).toBe("New unsaved title");
    expect(app.busy).toBe(false);
  });

  it("releases command state after failure so the user can retry", async () => {
    const setWatching = vi
      .fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(snapshot(2));
    const app = new AppController({ ...api, setWatching });
    await app.toggleWatching();
    expect(app.error).toBe("offline");
    expect(app.busy).toBe(false);
    await app.toggleWatching();
    expect(setWatching).toHaveBeenCalledTimes(2);
    expect(app.error).toBeNull();
  });
});
