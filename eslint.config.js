// ESLint flat config (ESLint 9). Lints the TypeScript/React frontend.
// Type-aware rules are kept light to stay fast; correctness rules that matter
// for a Tauri app (no floating promises in event wiring, hook deps) are on.
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

export default tseslint.config(
  // `target` is the workspace build dir (now at the repo root); it holds
  // Tauri-generated JS that must not be linted. `src-tauri/target` is kept for
  // any stale pre-workspace build dir. `design` holds the design handoffs —
  // static reference bundles, not source. Its `.dc.html` prototypes ship a
  // vendored runtime (`support.js`) that the handoff explicitly says is for
  // reading only; linting it reports 90+ errors about code we will never keep.
  {
    ignores: [
      "dist",
      "dist-web",
      "target",
      "src-tauri/target",
      "node_modules",
      ".claude",
      "design/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  // Node-executed scripts (no bundler, so no browser globals): declare the few
  // Node globals they use rather than pulling in the `globals` package.
  {
    files: ["scripts/**/*.mjs", "tools/**/*.mjs"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: {
        process: "readonly",
        console: "readonly",
        URL: "readonly",
        URLSearchParams: "readonly",
      },
    },
  },
  {
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // Allow intentional unused args prefixed with underscore.
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
);
