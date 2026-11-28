import { defineConfig, normalizePath } from "vite";
import { resolve } from "node:path";

const pluginRoot = `${normalizePath(resolve("../plugins"))}/`;

export default defineConfig({
  root: "src",
  plugins: [
    {
      name: "core-workbench-imports",
      generateBundle(_, outputs) {
        for (const output of Object.values(outputs)) {
          if (output.type !== "chunk") continue;
          for (const id of Object.keys(output.modules)) {
            if (
              id.startsWith(pluginRoot) ||
              /\/src\/(?:core\/App\.tsx|kit\/(?:FeatureBranchQueue|RepositorySettings)\.tsx)$/.test(
                id,
              )
            ) {
              this.error(`The Web Core bundle includes a domain module: ${id}`);
            }
          }
        }
      },
    },
  ],
  build: {
    outDir: "../dist/core",
    emptyOutDir: true,
    rollupOptions: {
      preserveEntrySignatures: "exports-only",
      input: {
        app: resolve("src/core/index.html"),
        "devdesk-ui": resolve("src/sdk/domain-sdk.ts"),
        "devdesk-kit": resolve("src/kit/kit-main.ts"),
      },
      output: {
        entryFileNames: (chunk) =>
          chunk.name === "devdesk-ui"
            ? "assets/devdesk-ui.mjs"
            : chunk.name === "devdesk-kit"
              ? "assets/devdesk-kit.mjs"
              : "assets/[name]-[hash].js",
      },
    },
  },
});
