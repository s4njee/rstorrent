// Vitest configuration for frontend unit tests (formatters, store logic, tree
// state). Uses jsdom so component/store code that touches the DOM works.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    globals: true,
  },
});
