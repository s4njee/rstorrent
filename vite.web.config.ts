import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// Build/serve config for the web console (web.html → src/web/main.tsx), the
// frontend's only target.
//
//   npm run dev:web   — dev server on :1421, proxying /api to the rstorrent-web
//                       server on :9080, so `fetch("/api/...")` is same-origin.
//                       Also serves the fixture demo at /demo.html.
//   npm run build:web — bundle to dist-web/, which the server embeds (rust-embed).
// `dev` / `build` are aliases of the two.
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
