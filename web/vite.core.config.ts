import { defineConfig } from "vite";
import { resolve } from "node:path";

export default defineConfig({
  root: "core",
  build: {
    outDir: "../dist/core",
    emptyOutDir: true,
    rollupOptions: {
      preserveEntrySignatures: "exports-only",
      input: {
        app: resolve("core/index.html"),
        "devdesk-ui": resolve("src/domain-sdk.tsx"),
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
