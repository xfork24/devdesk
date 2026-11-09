import { defineConfig } from "@playwright/test";
const profile = process.env.DEVDESK_E2E_PROFILE ?? "debug";
const port = Number(process.env.DEVDESK_E2E_PORT ?? "4318");
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Invalid DEVDESK_E2E_PORT");
if (profile !== "debug" && profile !== "release") {
  throw new Error("DEVDESK_E2E_PROFILE must be debug or release");
}
export default defineConfig({
  testDir: "./tests",
  workers: 1,
  timeout: 120000,
  expect: { timeout: 15000 },
  use: {
    actionTimeout: 15000,
    baseURL: `http://127.0.0.1:${port}`,
    channel: "chromium",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
  },
  webServer: {
    command: `../target/${profile}/devdesk-host --port ${port}`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 30000,
  },
});
