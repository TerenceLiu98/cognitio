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
