import { resolve } from "node:path";

const dependencies = resolve(import.meta.dirname, "../../../web/node_modules");

export default {
  root: import.meta.dirname,
  esbuild: { jsx: "automatic" },
  resolve: {
    alias: {
      react: resolve(dependencies, "react"),
      "react-dom": resolve(dependencies, "react-dom"),
    },
  },
  build: {
    outDir: "../../../target/web/fixture",
    emptyOutDir: true,
  },
};
