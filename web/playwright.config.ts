import { defineConfig } from "@playwright/test";
const profile = process.env.DEVDESK_E2E_PROFILE ?? "debug";
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
    baseURL: "http://127.0.0.1:4318",
    channel: "chromium",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
  },
  webServer: {
    command: `../target/${profile}/devdesk-host --port 4318`,
    url: "http://127.0.0.1:4318",
    reuseExistingServer: false,
    timeout: 30000,
  },
});
