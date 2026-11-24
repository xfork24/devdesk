import { defineConfig } from "vite";
import { resolve } from "node:path";

const sdk = "/assets/devdesk-ui.mjs";
const shared = new Set(["react", "react/jsx-runtime", "react-dom", sdk]);

export default defineConfig({
  base: "./",
  esbuild: { jsx: "automatic" },
  resolve: {
    alias: [
      {
        find: /^(?:.*\/)?(?:domain-sdk|workbench-layout|ActionFeedback|action-feedback)(?:\.tsx?)?$/,
        replacement: sdk,
      },
    ],
  },
  build: {
    outDir: "../target/web/software",
    emptyOutDir: true,
    modulePreload: false,
    rollupOptions: {
      preserveEntrySignatures: "strict",
      input: resolve("../plugins/software-development/web/entry.tsx"),
      external: (id) => shared.has(id),
      output: {
        paths: () => sdk,
        entryFileNames: "workbench.mjs",
        chunkFileNames: "chunks/[name]-[hash].mjs",
      },
    },
  },
});
