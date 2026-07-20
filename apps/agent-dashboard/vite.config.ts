import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";

export default defineConfig({
  root: "apps/agent-dashboard",
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: process.env.HARNESS_CAPTURE_API_PROXY
    ? {
        // Screenshot acceptance keeps browser reads same-origin. The target is
        // supplied only by the capture runner; normal development is unchanged.
        proxy: {
          "/v1": { target: process.env.HARNESS_CAPTURE_API_PROXY, changeOrigin: true },
          "/health": { target: process.env.HARNESS_CAPTURE_API_PROXY, changeOrigin: true },
        },
      }
    : undefined,
  build: {
    outDir: "web",
    emptyOutDir: true,
  },
});
