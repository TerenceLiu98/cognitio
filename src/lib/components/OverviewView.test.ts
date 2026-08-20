import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";

import { translate } from "$lib/i18n";
import { emptySnapshot, type AppSnapshot, type JobSummary } from "$lib/types";

import OverviewView from "./OverviewView.svelte";

function job(overrides: Partial<JobSummary>): JobSummary {
  return {
    id: "job-1",
    filename: "paper.pdf",
    state: "blocked",
    phase: "Task commit exists but is not pushed",
    progress: 85,
    agent: "codex",
    createdAt: "2026-08-20T00:00:00Z",
    updatedAt: "2026-08-20T00:00:00Z",
    error: "Retry to resume Git push",
    allowedActions: ["retry"],
    blockScope: "job",
    mineruMode: "precision",
    deployment: {
      status: "notTracked",
      url: null,
      error: null,
      updatedAt: null,
    },
    ...overrides,
  };
}

function snapshot(jobs: JobSummary[]): AppSnapshot {
  return {
    ...structuredClone(emptySnapshot),
    configured: true,
    watching: true,
    jobs,
  };
}

describe("OverviewView job actions", () => {
  it("renders only actions allowed by the backend", async () => {
    const onRetry = vi.fn();
    render(OverviewView, {
      snapshot: snapshot([
        job({ allowedActions: ["retry", "reparse"] }),
        job({ id: "verifying", state: "verifying", allowedActions: [] }),
        job({ id: "succeeded", state: "succeeded", allowedActions: [] }),
      ]),
      t: (key) => translate("en", key),
      onOpen: vi.fn(),
      onRetry,
      onCancel: vi.fn(),
    });

    expect(screen.getAllByLabelText("Retry")).toHaveLength(1);
    expect(screen.getAllByLabelText("Reparse")).toHaveLength(1);
    expect(screen.queryByLabelText("Cancel")).not.toBeInTheDocument();
    await fireEvent.click(screen.getByLabelText("Retry"));
    expect(onRetry).toHaveBeenCalledWith("job-1", "reuse_valid");
  });
});
