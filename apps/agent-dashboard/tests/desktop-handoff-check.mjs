import assert from "node:assert/strict";import fs from "node:fs";
const app=fs.readFileSync("apps/agent-dashboard/src/app/App.tsx","utf8");const vite=fs.readFileSync("apps/agent-dashboard/vite.config.ts","utf8");const main=[fs.readFileSync("crates/firm-cli/src/main.rs","utf8"),...fs.readdirSync("crates/firm-cli/src/main_modules").filter(file=>file.endsWith(".rs")).sort().map(file=>fs.readFileSync(`crates/firm-cli/src/main_modules/${file}`,"utf8"))].join("\n");
assert.match(vite,/base:\s*"\.\/"/);assert.match(app,/__AGENTFIRM_BOOTSTRAP__/);assert.match(app,/capabilityToken/);assert.match(app,/window\.location\.origin/);assert.doesNotMatch(app,/localStorage[^\n]*capability/i);
for(const value of ["node_id","daemon_generation","build_sha","protocol_version","schema_version","action_manifest_version","capability_auth"])assert.ok(main.includes(`"${value}"`),`/v1/meta missing ${value}`);
assert.ok(main.includes("build_git_rev()"));console.log("desktop handoff check: PASS");
