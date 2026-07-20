#!/usr/bin/env node

import { access, copyFile, mkdir, readFile, readdir } from "node:fs/promises";
import { constants } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourceRoot = join(
  repoRoot,
  "apps/agent-dashboard/fixtures/workbench-layout-v2-native-v1",
);

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

async function exists(path) {
  try {
    await access(path, constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function validateJsonl(path) {
  const text = await readFile(path, "utf8");
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    try {
      JSON.parse(line);
    } catch (error) {
      throw new Error(`${path}:${index + 1}: invalid JSON: ${error.message}`);
    }
  }
}

async function main() {
  const outputArg = argument("--output");
  if (!outputArg) {
    throw new Error("usage: node scripts/materialize-workbench-layout-fixture.mjs --output <empty-directory>");
  }

  const outputRoot = resolve(outputArg);
  if (outputRoot === repoRoot || outputRoot === dirname(repoRoot) || outputRoot === "/") {
    throw new Error(`refusing broad fixture output path: ${outputRoot}`);
  }

  const manifest = JSON.parse(await readFile(join(sourceRoot, "fixture-manifest.json"), "utf8"));
  if (await exists(outputRoot)) {
    const entries = await readdir(outputRoot);
    if (entries.length > 0) {
      throw new Error(`fixture output must be empty: ${outputRoot}`);
    }
  } else {
    await mkdir(outputRoot, { recursive: true });
  }

  for (const ledger of manifest.ledgers) {
    if (basename(ledger) !== ledger || !ledger.endsWith(".jsonl")) {
      throw new Error(`unsafe ledger name in fixture manifest: ${ledger}`);
    }
    const source = join(sourceRoot, ledger);
    await validateJsonl(source);
    await copyFile(source, join(outputRoot, ledger));
  }
  await copyFile(
    join(sourceRoot, "fixture-manifest.json"),
    join(outputRoot, "fixture-manifest.json"),
  );

  console.log(JSON.stringify({
    fixture: manifest.id,
    store_root: outputRoot,
    mission_id: manifest.mission_id,
    wave_id: manifest.wave_id,
    team_run_id: manifest.team_run_id,
    member_run_id: manifest.member_run_id,
    capture_now: manifest.capture_now,
    routes: manifest.routes,
  }, null, 2));
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
