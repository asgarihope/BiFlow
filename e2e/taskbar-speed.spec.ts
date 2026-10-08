import { expect, test } from "@playwright/test";

test("Windows taskbar preference has a Persian label", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "platform", { get: () => "Win32" });
    localStorage.setItem("biflow-language", "fa");
    localStorage.setItem("biflow-ui-mode-v1", "advanced");
  });
  await page.goto("/");
  await page
    .locator("[data-testid='sidebar-nav']")
    .getByRole("button", { name: "تنظیمات", exact: true })
    .click();
  await page.locator("#settings-tab-behavior").click();
  await expect(page.getByLabel("نمایش سرعت شبکه در نوار وظیفه")).toBeVisible();
});

test("Windows taskbar preference saves and toggles without a stack restart", async ({
  page,
}) => {
  await page.addInitScript(() =>
    Object.defineProperty(navigator, "platform", { get: () => "Win32" }),
  );
  await page.goto("/");
  await page.waitForFunction(
    "typeof window.__BIFLOW_RESET_MOCK === 'function'",
  );
  await page.evaluate(() => {
    window.__BIFLOW_RESET_MOCK?.();
    localStorage.setItem("biflow-ui-mode-v1", "advanced");
  });
  await page.reload();
  await page.getByRole("button", { name: /^Health/ }).click();
  const install = page.getByRole("button", { name: "Install", exact: true });
  let remaining = await install.count();
  while (remaining > 0) {
    await install.first().click();
    remaining -= 1;
    await expect(install).toHaveCount(remaining);
  }
  await page.getByRole("button", { name: /^Health/ }).click();
  await page.locator("[data-connection-action='connect']").click();
  await expect(
    page.getByRole("heading", { name: "Connected", exact: true }),
  ).toBeVisible();
  const settings = page
    .getByRole("navigation", { name: "Primary navigation" })
    .getByRole("button", { name: "Settings", exact: true });
  await settings.click();
  await page.getByRole("tab", { name: "Behavior", exact: true }).click();
  const toggle = page.getByLabel("Show network speed on taskbar");
  await expect(toggle).not.toBeChecked();
  await toggle.check();
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(
    page.getByRole("button", { name: "Save settings" }),
  ).toBeDisabled();
  await expect(page.getByTestId("settings-apply-banner")).toHaveCount(0);
  await page
    .getByRole("navigation", { name: "Primary navigation" })
    .getByRole("button", { name: "Home", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Connected", exact: true }),
  ).toBeVisible();
  await settings.click();
  await page.getByRole("tab", { name: "Behavior", exact: true }).click();
  await expect(toggle).toBeChecked();
  await toggle.uncheck();
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(
    page.getByRole("button", { name: "Save settings" }),
  ).toBeDisabled();
  await page
    .getByRole("navigation", { name: "Primary navigation" })
    .getByRole("button", { name: "Home", exact: true })
    .click();
  await settings.click();
  await page.getByRole("tab", { name: "Behavior", exact: true }).click();
  await expect(toggle).not.toBeChecked();
});
