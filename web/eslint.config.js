// Flat-config baseline follows the public Vite react-ts template:
// @eslint/js recommended + typescript-eslint recommended + react-hooks +
// react-refresh (src only), with Prettier owning formatting
// (eslint-config-prettier). See docs/code-standards-web.md for the rules.
import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";
import prettier from "eslint-config-prettier";

export default tseslint.config(
  { ignores: ["dist", "node_modules", "test-results", "playwright-report"] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    rules: {
      // Warnings stay visible in every run; the single sanctioned `any`
      // escape hatch is the Row alias in src/components/rpc-types.ts.
      "@typescript-eslint/no-explicit-any": "error",
      "no-sequences": ["error", { allowInParentheses: false }],
      // Leading-underscore names document intentionally unused bindings.
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
      // `catch {}` is the deliberate fire-and-forget pattern around UI loads.
      "no-empty": ["error", { allowEmptyCatch: true }],
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      // Declared explicitly (not via preset spread) so the meaning does not
      // drift with plugin major versions.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
    },
  },
  {
    files: ["tests/**/*.ts", "playwright.config.ts"],
    languageOptions: { globals: globals.node },
    rules: {
      // Spec helpers ferry untyped kernel JSON rows; the kernel contract, not
      // the spec, owns those shapes.
      "@typescript-eslint/no-explicit-any": "off",
    },
  },
  {
    files: ["*.js"],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },
  prettier,
);
