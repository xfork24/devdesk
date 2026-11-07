import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  workers: 1,
  timeout: 120000,
  use: {
    actionTimeout: 15000,
    baseURL: "http://127.0.0.1:4318",
    channel: "chromium",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "../target/debug/devdesk-host --port 4318",
    url: "http://127.0.0.1:4318",
    reuseExistingServer: false,
    timeout: 30000,
  },
});
