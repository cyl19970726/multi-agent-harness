#!/usr/bin/env node
// ADR 0063: the Star Harness plugin package, the in-repo Dynamic Workflow
// retirement register and archive, the superseded specs, the archived
// operator skills, and the frozen collaboration-skill evaluation workspace
// live only in git history (last tree on master: 918e9002). None of these
// paths, nor the gates that only existed to police them, may come back.

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

const retiredPaths = [
  "plugins",
  ".claude-plugin",
  "specs",
  "archive",
  "collab-skill-workspace",
  "scripts/check-dynamic-workflow-retirement-manifest.mjs",
  "scripts/check-star-harness-plugin.mjs",
  "scripts/check-star-harness-hook.mjs",
  "scripts/sync-star-harness-plugin-skills.mjs",
];

const failures = retiredPaths
  .filter((path) => existsSync(join(ROOT, path)))
  .map((path) => `retired path present: ${path} (ADR 0063; git history is the archive)`);

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log("retired path boundary passed (ADR 0063)");
