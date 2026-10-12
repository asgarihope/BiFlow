import { expect, test } from "@playwright/test";

for (const { platform, label, persian } of [
  {
    platform: "Win32",
    label: "Show network speed on taskbar",
    persian: "نمایش سرعت شبکه در نوار وظیفه",
  },
  {
    platform: "Linux x86_64",
    label: "Show network speed in panel",
    persian: "نمایش سرعت شبکه در پنل",
  },
]) {
  test(`${platform} speed preference has a Persian label`, async ({ page }) => {
    await page.addInitScript((platform) => {
      Object.defineProperty(navigator, "platform", { get: () => platform });
      localStorage.setItem("biflow-language", "fa");
      localStorage.setItem("biflow-ui-mode-v1", "advanced");
    }, platform);
    await page.goto("/");
    await page
      .locator("[data-testid='sidebar-nav']")
      .getByRole("button", { name: "تنظیمات", exact: true })
      .click();
    await page.getByRole("tab", { name: "نمایش سرعت", exact: true }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByLabel(persian)).toBeVisible();
    await page.getByLabel("واحد سرعت").selectOption("megabits");
    await page.getByRole("button", { name: "Save settings" }).click();
    await expect(
      page.getByRole("button", { name: "Save settings" }),
    ).toBeDisabled();
    await page.getByRole("tab", { name: "رفتار", exact: true }).click();
    await expect(page.getByLabel(persian)).toHaveCount(0);
    await page.getByRole("tab", { name: "نمایش سرعت", exact: true }).click();
    await expect(page.getByLabel("واحد سرعت")).toHaveValue("megabits");
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
  });

  test(`${platform} speed preference saves and toggles without a stack restart`, async ({
    page,
  }) => {
    await page.addInitScript(
      (platform) =>
        Object.defineProperty(navigator, "platform", { get: () => platform }),
      platform,
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
    await page.getByRole("tab", { name: "Speed display", exact: true }).click();
    const toggle = page.getByLabel(label);
    await expect(toggle).not.toBeChecked();
    await toggle.check();
    const unit = page.getByLabel("Speed unit");
    await expect(unit).toHaveValue("auto_bits");
    await unit.selectOption("megabytes");
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
    await page.locator("[data-connection-action='disconnect']").click();
    await expect(
      page.getByRole("heading", { name: "Not connected", exact: true }),
    ).toBeVisible();
    await settings.click();
    await page.getByRole("tab", { name: "Speed display", exact: true }).click();
    await expect(toggle).toBeChecked();
    await expect(unit).toHaveValue("megabytes");
    await unit.selectOption("megabits");
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
    await page.getByRole("tab", { name: "Speed display", exact: true }).click();
    await expect(toggle).not.toBeChecked();
    await expect(unit).toHaveValue("megabits");
  });
}
