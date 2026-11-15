import { test as base } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
// Each test owns its Host and dataset; runtime failures cannot leak into another test.
export const test = base.extend<{ kernelOrigin: string }>({
  // An empty destructuring pattern is Playwright's way to ignore fixture args.
  // eslint-disable-next-line no-empty-pattern
  kernelOrigin: async ({}, use) => {
    const directory = mkdtempSync(join(tmpdir(), "devdesk-browser-kernel-"));
    const kernelEnv = { ...process.env };
    delete kernelEnv.DEVDESK_ACCESS_TOKEN;
    const child = spawn(
      resolve(
        import.meta.dirname,
        "../../../target",
        process.env.DEVDESK_E2E_PROFILE ?? "debug",
        "devdesk-host",
      ),
      ["--data", directory, "--port", "0"],
      {
        env: kernelEnv,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let diagnostics = "";
    child.stderr.on("data", (data) => {
      diagnostics = (diagnostics + data).slice(-4000);
    });
    try {
      const origin = await new Promise<string>((done, fail) => {
        const lines = createInterface({ input: child.stdout });
        const timer = setTimeout(
          () => fail(new Error("Isolated kernel startup timed out")),
          15000,
        );
        child.once("error", (error) => {
          clearTimeout(timer);
          fail(error);
        });
        child.once("exit", () => {
          clearTimeout(timer);
          fail(new Error(diagnostics));
        });
        lines.on("line", (line) => {
          if (line.startsWith("http://")) {
            clearTimeout(timer);
            lines.close();
            done(line.trim());
          }
        });
      });
      await use(origin);
    } finally {
      if (child.exitCode === null)
        await new Promise<void>((done) => {
          const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
          child.once("exit", () => {
            clearTimeout(timer);
            done();
          });
          child.kill("SIGINT");
        });
      rmSync(directory, { recursive: true, force: true });
    }
  },
  baseURL: async ({ kernelOrigin }, use) => use(kernelOrigin),
});
