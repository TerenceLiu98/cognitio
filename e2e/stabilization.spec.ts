import { expect, test } from "@playwright/test";

test("uses backend-provided job actions and locks the repository", async ({
  page,
}) => {
  await page.goto("/");

  await expect(page.getByText("paper.pdf")).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Reparse" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Cancel" })).toHaveCount(0);
  await expect(page.getByText("Site deployment failed")).toBeVisible();

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByLabel("GitHub repository")).toHaveAttribute(
    "readonly",
    "",
  );
  await expect(
    page.getByText("Processing changes apply to new tasks."),
  ).toBeVisible();
});

test("renders a compact menubar task surface", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 560 });
  await page.goto("/?surface=menubar");

  await expect(page.getByLabel("Cognitio")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Current activity" }),
  ).toBeVisible();
  await expect(page.getByText("zmag007.pdf").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Open inbox" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Settings" }).first(),
  ).toBeVisible();

  const hasHorizontalOverflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth >
      document.documentElement.clientWidth,
  );
  expect(hasHorizontalOverflow).toBe(false);
});
