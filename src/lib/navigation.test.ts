import { describe, expect, it } from "vitest";

import { isViewName } from "$lib/navigation";

describe("isViewName", () => {
  it("accepts application views and rejects arbitrary event payloads", () => {
    expect(isViewName("overview")).toBe(true);
    expect(isViewName("settings")).toBe(true);
    expect(isViewName("logs")).toBe(true);
    expect(isViewName("preferences")).toBe(false);
    expect(isViewName({ view: "logs" })).toBe(false);
  });
});
