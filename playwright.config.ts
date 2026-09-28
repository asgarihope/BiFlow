import { defineConfig, devices } from "@playwright/test";

// The execution environment can set both variables; Node warns before every
// web-server and worker process unless one is removed before Playwright forks.
delete process.env.NO_COLOR;

const e2ePort = Number(process.env.BIFLOW_E2E_PORT ?? "1420");
if (!Number.isInteger(e2ePort) || e2ePort < 1 || e2ePort > 65535) {
  throw new Error("BIFLOW_E2E_PORT must be an integer between 1 and 65535");
}
const e2eBaseUrl = `http://127.0.0.1:${e2ePort}`;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: {
    baseURL: e2eBaseUrl,
    trace: "on-first-retry",
    viewport: { width: 1120, height: 760 },
    ...(process.env.BIFLOW_CHROMIUM_PATH
      ? {
          launchOptions: {
            executablePath: process.env.BIFLOW_CHROMIUM_PATH,
          },
        }
      : {}),
  },
  webServer: {
    command: `pnpm --dir apps/desktop dev --host 127.0.0.1 --port ${e2ePort}`,
    url: e2eBaseUrl,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1120, height: 760 },
      },
    },
  ],
});
