#!/usr/bin/env node
// Dashboard phase-board read-model verification (goal-task-board-model S1).
//
// Proves the load-bearing guarantee of the Goal -> Phase -> [Graph | Kanban]
// restructure: the phase-scoped read-model helpers bucket a goal's tasks STRICTLY
// under their phase, and a phaseless (goal-scoped) task lands ONLY in the
// "(no phase)" set — never leaking into a phase view. Mirrors the dependency-free
// style of tests/project-picker-check.mjs (a self-contained node check that runs
// everywhere CI included; no Playwright, no serve).
//
// It exercises the REAL `phaseKanban` / `phaseTaskDag` / `phaselessGoalTasks`
// implementations from src/model/readModel.ts by transpiling that module (and its
// single runtime dependency, warnings.ts) with the TypeScript compiler API into a
// temp dir and importing the emitted ESM — so a regression in the actual helpers
// is caught, not a re-implementation of them.
//
// Fixture: the planning-tier goal "pm" with one phase "p1" owning two live tasks
// (pk1 planned, pk2 running), plus a phaseless goal-scoped task "pn1".
//
// Exit code: 0 when every check passes, 1 otherwise. Prints a PASS/FAIL matrix
// that scripts/verify-fixes.sh folds into its own.

import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const modelDir = join(here, "..", "src", "model");

let pass = 0;
let fail = 0;
const ok = (m) => { console.log(`  PASS  ${m}`); pass += 1; };
const bad = (m) => { console.log(`  FAIL  ${m}`); fail += 1; };

/** Transpile readModel.ts + warnings.ts to ESM in a temp dir and import them. */
async function loadReadModel() {
  const { default: ts } = await import("typescript");
  const dir = await mkdtemp(join(tmpdir(), "phase-board-"));
  const opts = { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 };
  for (const name of ["warnings", "readModel"]) {
    const src = await readFile(join(modelDir, `${name}.ts`), "utf8");
    let js = ts.transpileModule(src, { compilerOptions: opts }).outputText;
    // Point the one runtime import (readModel -> ./warnings) at the emitted file.
    js = js.replace(/from\s+["']\.\/warnings["']/g, 'from "./warnings.mjs"');
    await writeFile(join(dir, `${name}.mjs`), js, "utf8");
  }
  const mod = await import(pathToFileURL(join(dir, "readModel.mjs")).href);
  await rm(dir, { recursive: true, force: true });
  return mod;
}

function laneIds(lanes) {
  // Flatten every lane's tasks into the set of task ids the lanes cover.
  return new Set(lanes.flatMap((lane) => lane.tasks.map((task) => task.id)));
}

function dagIds(layers) {
  // Flatten every layer/group's tasks into the set of task ids the DAG covers.
  return new Set(
    layers.flatMap((layer) => layer.groups.flatMap((group) => group.tasks.map((t) => t.id))),
  );
}

function setEq(a, b) {
  if (a.size !== b.size) return false;
  for (const v of a) if (!b.has(v)) return false;
  return true;
}

async function main() {
  console.log("== Dashboard phase-board read-model checks (goal-task-board-model) ==");
  const { phaseKanban, phaseTaskDag, phaselessGoalTasks } = await loadReadModel();

  // Planning-tier fixture: goal "pm", phase "p1" with two live tasks + a
  // superseded one that must never appear; plus a phaseless goal-scoped task.
  const tasks = [
    { id: "pk1", goal_id: "pm", phase_id: "p1", status: "planned", depends_on_task_ids: [], owned_paths: ["a"] },
    { id: "pk2", goal_id: "pm", phase_id: "p1", status: "running", depends_on_task_ids: ["pk1"], owned_paths: ["b"] },
    { id: "pk0", goal_id: "pm", phase_id: "p1", status: "superseded", depends_on_task_ids: [], owned_paths: ["c"] },
    { id: "pn1", goal_id: "pm", phase_id: null, status: "planned", depends_on_task_ids: [], owned_paths: ["d"] },
  ];

  // 1) phaseKanban('p1') buckets the phase's live tasks pk1/pk2 (not superseded).
  const lanes = phaseKanban("p1", tasks);
  const lanesP1 = laneIds(lanes);
  if (setEq(lanesP1, new Set(["pk1", "pk2"]))) {
    ok("phaseKanban('p1') buckets pk1 + pk2 (superseded pk0 excluded)");
  } else {
    bad(`phaseKanban('p1') covered {${[...lanesP1].join(",")}}, expected {pk1,pk2}`);
  }
  // Bucketing is by status: pk1 -> planned lane, pk2 -> running lane.
  const planned = lanes.find((l) => l.id === "planned");
  const running = lanes.find((l) => l.id === "running");
  if (planned?.tasks.some((t) => t.id === "pk1") && running?.tasks.some((t) => t.id === "pk2")) {
    ok("phaseKanban('p1') routes pk1->planned, pk2->running lane");
  } else {
    bad("phaseKanban('p1') did NOT route tasks to their status lanes");
  }

  // 2) phaseKanban of a nonexistent phase is empty (all lanes empty).
  const nonexistent = phaseKanban("nope", tasks);
  if (laneIds(nonexistent).size === 0) {
    ok("phaseKanban('nope') is empty (no tasks bucketed)");
  } else {
    bad("phaseKanban('nope') leaked tasks for a nonexistent phase");
  }

  // 3) phaseTaskDag('p1') and phaseKanban('p1') cover the IDENTICAL id set.
  const dag = phaseTaskDag("p1", tasks);
  const dagP1 = dagIds(dag);
  if (setEq(dagP1, lanesP1)) {
    ok(`phaseTaskDag('p1') and phaseKanban('p1') cover the same ids {${[...dagP1].sort().join(",")}}`);
  } else {
    bad(`DAG {${[...dagP1].join(",")}} != Kanban {${[...lanesP1].join(",")}}`);
  }

  // 4) A phaseless task lands ONLY in "(no phase)" — not in any phase view.
  const phaseless = phaselessGoalTasks("pm", tasks);
  const phaselessIds = new Set(phaseless.map((t) => t.id));
  const inNoPhase = phaselessIds.has("pn1");
  const inPhaseViews = lanesP1.has("pn1") || dagP1.has("pn1");
  if (inNoPhase && !inPhaseViews) {
    ok("phaseless task pn1 lands ONLY in (no phase), never in a phase view");
  } else {
    bad(`phaseless pn1 misrouted (noPhase=${inNoPhase}, inPhaseViews=${inPhaseViews})`);
  }
  // The (no phase) set must exclude phased tasks (pk1/pk2/pk0 belong to p1).
  if (!phaselessIds.has("pk1") && !phaselessIds.has("pk2") && !phaselessIds.has("pk0")) {
    ok("phaselessGoalTasks('pm') excludes every phased task");
  } else {
    bad("phaselessGoalTasks('pm') leaked a phased task into (no phase)");
  }

  console.log(`\n   phase-board checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`phase-board-check crashed: ${error.stack || error}`);
  process.exit(1);
});
