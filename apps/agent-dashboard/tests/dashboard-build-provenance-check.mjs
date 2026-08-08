import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { loadConfigFromFile } from "vite";

const configPath = resolve("apps/agent-dashboard/vite.config.ts");
const serverBuildScript = resolve("crates/firm-cli/build.rs");
const exact = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";

async function configuredRevision(value) {
  process.env.FIRM_BUILD_GIT_REV = value;
  const loaded = await loadConfigFromFile(
    { command: "build", mode: "test" },
    configPath,
  );
  assert.ok(loaded, "Vite config must load");
  return JSON.parse(loaded.config.define["import.meta.env.VITE_DASHBOARD_GIT_REV"]);
}

const buildScriptDir = mkdtempSync(join(tmpdir(), "firm-build-provenance-"));
const buildScriptBinary = join(buildScriptDir, "firm-cli-build-script");

try {
  execFileSync(
    "rustc",
    ["--edition=2021", serverBuildScript, "-o", buildScriptBinary],
    { stdio: "pipe" },
  );

  function configuredServerRevision(value) {
    const output = execFileSync(buildScriptBinary, [], {
      cwd: resolve("."),
      env: { ...process.env, FIRM_BUILD_GIT_REV: value },
      encoding: "utf8",
    });
    const prefix = "cargo:rustc-env=FIRM_BUILD_GIT_REV=";
    const revision = output
      .split(/\r?\n/u)
      .find((line) => line.startsWith(prefix))
      ?.slice(prefix.length);
    return revision ?? "unknown";
  }

  for (const [supplied, expected] of [
    [exact, exact.toLowerCase()],
    ["unknown", "unknown"],
    ["not-a-full-object-id", "unknown"],
    ["", "unknown"],
  ]) {
    assert.equal(await configuredRevision(supplied), expected);
    assert.equal(configuredServerRevision(supplied), expected);
  }
} finally {
  delete process.env.FIRM_BUILD_GIT_REV;
  rmSync(buildScriptDir, { recursive: true, force: true });
}

console.log(
  "dashboard build provenance: server/frontend full-40, unknown, and malformed contracts passed",
);
