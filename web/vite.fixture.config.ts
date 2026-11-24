import { defineConfig } from "vite";

export default defineConfig({
  root: "fixtures/workbench",
  build: {
    outDir: "../../dist/fixture",
    emptyOutDir: true,
  },
});
