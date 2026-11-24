import { defineConfig } from "vite";
import { resolve } from "node:path";

export default defineConfig({
  root: "core",
  plugins: [
    {
      name: "core-workbench-imports",
      generateBundle(_, outputs) {
        for (const output of Object.values(outputs)) {
          if (output.type !== "chunk") continue;
          for (const id of Object.keys(output.modules)) {
            if (
              /\/configurations\/|\/src\/(?:App\.tsx|components\/(?:FeatureBranchQueue|RepositorySettings)\.tsx)$/.test(
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
        app: resolve("core/index.html"),
        "devdesk-ui": resolve("src/domain-sdk.ts"),
      },
      output: {
        entryFileNames: (chunk) =>
          chunk.name === "devdesk-ui"
            ? "assets/devdesk-ui.mjs"
            : "assets/[name]-[hash].js",
      },
    },
  },
});
