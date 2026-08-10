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
 * build. Mirrors `crates/firm-cli/build.rs`'s server-side embedding. Release
 * builds from a source archive may provide the exact revision through
 * `FIRM_BUILD_GIT_REV`.
 */
function resolveDashboardGitRev(): string {
  const suppliedRaw = process.env.FIRM_BUILD_GIT_REV;
  if (suppliedRaw !== undefined) {
    const supplied = suppliedRaw.trim();
    return /^[0-9a-f]{40}$/i.test(supplied) ? supplied.toLowerCase() : "unknown";
  }
  try {
    const rev = execFileSync("git", ["rev-parse", "--verify", "HEAD^{commit}"], {
      cwd: fileURLToPath(new URL(".", import.meta.url)),
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
    return /^[0-9a-f]{40}$/i.test(rev) ? rev.toLowerCase() : "unknown";
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
  server: {
        // Screenshot acceptance keeps browser reads same-origin. The target is
        // supplied only by the capture runner; normal development is unchanged.
        proxy: {
          "/v1": { target: process.env.HARNESS_CAPTURE_API_PROXY ?? "http://127.0.0.1:8787", changeOrigin: true },
          "/health": { target: process.env.HARNESS_CAPTURE_API_PROXY ?? "http://127.0.0.1:8787", changeOrigin: true },
        },
      },
  build: {
    outDir: "web",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        // The app was a single >500 kB chunk. Split the stable dependency
        // layer so app changes do not re-download the whole vendor bundle.
        // React, Radix, and lucide share an import cycle boundary, so they
        // form one framework chunk instead of two cross-importing chunks.
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("react") || id.includes("scheduler") || id.includes("@radix-ui") || id.includes("lucide-react")) return "framework-vendor";
          return "vendor";
        },
      },
    },
  },
});
