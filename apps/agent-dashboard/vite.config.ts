import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  root: "apps/agent-dashboard",
  base: "./",
  plugins: [react()],
  build: {
    outDir: "web",
    emptyOutDir: true,
  },
});
