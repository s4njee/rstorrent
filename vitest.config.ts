// Vitest configuration for frontend unit tests (formatters, store logic, tree
// state). Uses jsdom so component/store code that touches the DOM works.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    globals: true,
    // React picks its build from NODE_ENV when it is first imported, and only
    // the development build exports `act` — a shell that exports
    // NODE_ENV=production would fail every component test with "act is not a
    // function". Pin it so the suite reads the same whatever it inherits.
    env: { NODE_ENV: "test" },
    // Fills in a localStorage jsdom doesn't provide — see the file's comment.
    setupFiles: ["./src/test/setup.ts"],
  },
});
