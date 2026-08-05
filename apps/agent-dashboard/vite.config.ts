import { execFileSync } from "node:child_process";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";

/**
 * The commit this frontend was built/dev-served from, embedded once at
 * process start (never re-read per request) — the frontend half of issue
 * #307's provenance surface. Best-effort: a build without git available (a
 * source tarball, a container with git stripped) still succeeds, falling
 * back to "unknown" so the footer/banner degrade instead of breaking the
 * build. Mirrors `crates/harness-cli/build.rs`'s server-side embedding.
 */
function resolveDashboardGitRev(): string {
  try {
    const rev = execFileSync("git", ["rev-parse", "--short", "HEAD"], {
      cwd: fileURLToPath(new URL(".", import.meta.url)),
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
    return rev || "unknown";
  } catch {
    return "unknown";
  }
}

export default defineConfig({
  root: "apps/agent-dashboard",
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  define: {
    "import.meta.env.VITE_DASHBOARD_GIT_REV": JSON.stringify(resolveDashboardGitRev()),
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
