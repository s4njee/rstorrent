import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// Build/serve config for the browser web UI (web.html → src/web/main.tsx),
// separate from the Tauri app (vite.config.ts / index.html).
//
//   npm run dev:web   — dev server on :1421, proxying /api to the rstorrent-web
//                       server on :9080, so `fetch("/api/...")` is same-origin.
//   npm run build:web — bundle to dist-web/, which the server embeds (rust-embed).
export default defineConfig({
  plugins: [react()],
  server: {
    port: 1421,
    strictPort: true,
    proxy: {
      "/api": "http://127.0.0.1:9080",
    },
  },
  build: {
    outDir: "dist-web",
    emptyOutDir: true,
    rollupOptions: {
      input: resolve(import.meta.dirname, "web.html"),
    },
  },
});
