#!/usr/bin/env node

import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const canonicalRoot = join(repoRoot, "skills");
const pluginRoot = join(repoRoot, "plugins", "star-harness", "skills");
const names = [
  "orchestrate-mission-waves",
  "collaborate-as-agent-team-member",
];
const check = process.argv.includes("--check");

function filesUnder(root, base = root) {
  if (!existsSync(root)) return [];
  const output = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) output.push(...filesUnder(path, base));
    else output.push(relative(base, path));
  }
  return output.sort();
}

function mismatch(name) {
  const source = join(canonicalRoot, name);
  const mirror = join(pluginRoot, name);
  const sourceFiles = filesUnder(source);
  const mirrorFiles = filesUnder(mirror);
  if (JSON.stringify(sourceFiles) !== JSON.stringify(mirrorFiles)) return true;
  return sourceFiles.some((path) =>
    !readFileSync(join(source, path)).equals(readFileSync(join(mirror, path))),
  );
}

if (check) {
  const drift = names.filter(mismatch);
  if (drift.length) {
    console.error(
      `star-harness generated skill mirrors drifted: ${drift.join(", ")}; ` +
        "run `node scripts/sync-star-harness-plugin-skills.mjs`",
    );
    process.exit(1);
  }
  console.log("star-harness skill mirrors match canonical skills byte-for-byte");
  process.exit(0);
}

mkdirSync(pluginRoot, { recursive: true });
for (const name of names) {
  const target = join(pluginRoot, name);
  rmSync(target, { recursive: true, force: true });
  cpSync(join(canonicalRoot, name), target, { recursive: true });
}
console.log("synchronized canonical Star Harness skills into plugin");
