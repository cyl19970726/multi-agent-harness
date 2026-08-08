import assert from "node:assert/strict";
import { resolve } from "node:path";

import { loadConfigFromFile } from "vite";

const configPath = resolve("apps/agent-dashboard/vite.config.ts");
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

assert.equal(await configuredRevision(exact), exact.toLowerCase());
assert.equal(await configuredRevision("unknown"), "unknown");
delete process.env.FIRM_BUILD_GIT_REV;

console.log("dashboard build provenance: full-40 and unknown contracts passed");
