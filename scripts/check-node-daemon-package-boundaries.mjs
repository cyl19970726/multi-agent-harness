#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { encoding: "utf8" }));
const daemon = metadata.packages.find((pkg) => pkg.name === "firm-node-daemon");
const cli = metadata.packages.find((pkg) => pkg.name === "firm-cli");
const store = metadata.packages.find((pkg) => pkg.name === "firm-store");
assert(daemon && cli && store, "workspace must own daemon, CLI and Store packages");
assert(daemon.targets.every((target) => target.kind.includes("lib")), "daemon is a library, not another binary");
assert(cli.dependencies.some((dep) => dep.name === daemon.name && dep.kind === null), "CLI must compose the daemon library");
assert(!store.dependencies.some((dep) => dep.name === daemon.name), "Store must not depend on daemon scheduling");

// Coupled CLI fixtures use the same implementation through a finite, opt-in
// adapter. Normal builds must not export that adapter or activate its hooks.
assert.deepEqual(daemon.features.default, [], "daemon test support must be default-off");
assert.deepEqual(daemon.features["test-support"], [], "test support must not activate production dependencies");
for (const pkg of metadata.packages) {
  for (const dep of pkg.dependencies.filter((dep) => dep.name === daemon.name)) {
    if (dep.features.includes("test-support")) {
      assert(pkg.name === cli.name && dep.kind === "dev", "only CLI dev-dependency may enable daemon test support");
    }
  }
}
assert(cli.dependencies.some((dep) => dep.name === daemon.name && dep.kind === "dev" && dep.features.includes("test-support")), "real CLI fixtures must enable the finite adapter");
assert.match(read("crates/firm-node-daemon/src/lib.rs"), /#\[cfg\(feature = "test-support"\)\]\s*pub use supervisor_daemon::test_support;/, "test support export must be feature guarded");
assert.match(read("crates/firm-node-daemon/src/supervisor_daemon.rs"), /#\[cfg\(feature = "test-support"\)\]\s*pub mod test_support;/, "test support module must be feature guarded");

// These are the dependencies actually used by this finite boundary. Provider
// event records are neutral DTOs; provider-specific packages remain CLI-only.
const requiredDependencies = new Map([
  ["firm-core", "harness_core"],
  ["firm-store", "harness_store"],
  ["firm-runtime-host", "harness_runtime_host"],
  ["firm-provider-events", "harness_provider_events"],
  ["serde", "serde"], ["serde_json", "serde_json"],
  ["thiserror", "thiserror"],
]);
const paths = execFileSync("git", ["ls-files", "-co", "--exclude-standard", "crates/firm-node-daemon/src"], { encoding: "utf8" })
  .trim().split("\n").filter((path) => path.endsWith(".rs") && existsSync(path));
const productionPaths = paths.filter((path) => !path.includes("test_support") && !path.includes("/tests/") && !path.endsWith("_tests.rs"));
const production = productionPaths.map(read).join("\n");
for (const dependency of daemon.dependencies) {
  assert(requiredDependencies.has(dependency.name), `daemon dependency is outside its concrete neutral boundary: ${dependency.name}`);
}
for (const [name, rustName] of requiredDependencies) {
  assert(daemon.dependencies.some((dep) => dep.name === name && dep.kind === null), `missing actual daemon dependency ${name}`);
  assert(production.includes(`${rustName}::`), `unused declared daemon dependency ${name}`);
}
for (const token of ["harness_provider_codex::", "harness_provider_claude::", "harness_provider_kimi::", "harness_provider_pi::", "harness_provider_deepseek::", "crate::provider_adapter", "crate::daemon_application::", "crate::daemon_client::"]) {
  assert(!production.includes(token), `daemon production imports CLI/provider composition: ${token}`);
}
for (const token of ["pub struct MultiTeamDaemon", "pub struct MultiTeamContext", "pub contexts:", "pub supervisor_start_gate:", "pub session_runtimes:"]) {
  assert(!production.includes(token), `daemon leaked an internal registry/lock: ${token}`);
}
for (const path of ["supervisor_daemon.rs", "daemon_error.rs", "daemon_protocol.rs", "daemon_application_port.rs", "daemon_support.rs"]) {
  assert(!existsSync(`crates/firm-cli/src/${path}`), `duplicate old CLI production owner: ${path}`);
}
for (const path of ["daemon_application.rs", "daemon_client.rs", "provider_adapter.rs"]) {
  assert(existsSync(`crates/firm-cli/src/${path}`), `CLI application/client owner missing: ${path}`);
}
console.log("NodeDaemon package boundaries are valid.");
