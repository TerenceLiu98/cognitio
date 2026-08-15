import { describe, expect, it } from "vitest";

import { detectLocale, translate } from "./i18n";

describe("i18n", () => {
  it("selects Chinese locale variants", () => {
    expect(detectLocale("zh-CN")).toBe("zh");
    expect(translate("zh", "overview")).toBe("概览");
  });

  it("falls back to English for other locales", () => {
    expect(detectLocale("fr-FR")).toBe("en");
    expect(translate("en", "overview")).toBe("Overview");
  });
});
