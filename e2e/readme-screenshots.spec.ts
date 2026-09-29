import { expect, test, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const screenshotDir = join(process.cwd(), "docs/screenshots");

async function openAdvanced(page: Page) {
  await page.goto("/");
  await page.waitForFunction(
    "typeof window.__BIFLOW_RESET_MOCK === 'function'",
  );
  await page.evaluate(() => {
    window.__BIFLOW_RESET_MOCK?.();
    localStorage.setItem("biflow-ui-mode-v1", "advanced");
  });
  await page.reload();
  await expect(page.getByRole("radio", { name: "Advanced" })).toBeVisible();
}

function nav(page: Page, name: string) {
  return page
    .getByRole("navigation", { name: "Primary navigation" })
    .getByRole("button", { name, exact: true });
}

async function connectStack(page: Page) {
  await page.getByRole("button", { name: /^Health/ }).click();
  const install = page.getByRole("button", { name: "Install", exact: true });
  while ((await install.count()) > 0) {
    await install.first().click();
  }
  await page.getByRole("button", { name: /^Health/ }).click();
  await page.locator("[data-connection-action='connect']").click();
  await expect(
    page.getByRole("heading", { name: "Connected", exact: true }),
  ).toBeVisible();
}

test.describe("readme screenshots", () => {
  test.skip(
    !process.env.BIFLOW_CAPTURE_README,
    "set BIFLOW_CAPTURE_README=1 to refresh docs/screenshots",
  );

  test("captures desktop and mobile product shots", async ({ page }) => {
    mkdirSync(screenshotDir, { recursive: true });
    await page.setViewportSize({ width: 1120, height: 760 });
    await openAdvanced(page);
    await connectStack(page);
    await page.getByLabel("Site or IP").fill("aparat.com");
    await page.getByRole("radio", { name: "DIRECT", exact: true }).click();
    await page.getByRole("button", { name: "Add", exact: true }).click();
    await expect(page.getByTestId("toast")).toBeVisible();
    await expect(page.getByTestId("toast")).toHaveCount(0, { timeout: 5_000 });
    await page.screenshot({
      path: join(screenshotDir, "desktop.png"),
      animations: "disabled",
    });

    // Troubleshoot opens on the live-connections table.
    await nav(page, "Troubleshoot").click();
    const connections = page.getByTestId("live-connections");
    await expect(connections).toBeVisible();
    await expect(connections.getByText("digikala.ir")).toBeVisible();
    await page.screenshot({
      path: join(screenshotDir, "diagnostics.png"),
      animations: "disabled",
    });

    await nav(page, "Home").click();
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByTestId("bottom-nav")).toBeVisible();
    await page.screenshot({
      path: join(screenshotDir, "mobile.png"),
      animations: "disabled",
    });
  });
});
