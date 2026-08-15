import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";

import { translate } from "$lib/i18n";
import AppSidebar from "./AppSidebar.svelte";

describe("AppSidebar", () => {
  it("renders Chinese navigation and reports navigation", async () => {
    const onNavigate = vi.fn();
    render(AppSidebar, {
      active: "overview",
      configured: true,
      watching: true,
      t: (key) => translate("zh", key),
      onNavigate,
    });

    expect(screen.getByText("正在监听")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(onNavigate).toHaveBeenCalledWith("settings");
  });
});
