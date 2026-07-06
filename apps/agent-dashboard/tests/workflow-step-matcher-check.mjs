#!/usr/bin/env node
// Dashboard workflow-step matcher verification (PR #198 review fix, Finding 2).
//
// Proves the load-bearing guarantee of `matchRuntimeSteps` in
// src/model/workflowSelectors.ts: the positional runtime-step fallback may
// fire ONLY when a plan has no parseable labels at all. Once any plan row has
// a label, unmatched rows must render as "not started" (undefined) instead of
// borrowing another step's row by position — and a runtime step already
// matched to one plan row can never be matched again to a different row.
//
// This guards against the regression where a dynamic run creates step rows
// only as agents start (mid-flight, out of textual order), which made
// `runtimeSteps[index]` misattribute a running/failed step's status and
// "Open evidence" link to the wrong plan row, while that step ALSO rendered
// correctly on its own (correctly-labeled) row — a duplicate attribution bug.
//
// Mirrors the dependency-free style of tests/phase-board-check.mjs: it
// exercises the REAL `matchRuntimeSteps` implementation by transpiling
// workflowSelectors.ts (and its transitive runtime deps, readModel.ts +
// warnings.ts) with the TypeScript compiler API into a temp dir and importing
// the emitted ESM — so a regression in the actual matcher is caught, not a
// re-implementation of it.
//
// Exit code: 0 when every check passes, 1 otherwise.

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

/** Transpile workflowSelectors.ts + its runtime deps to ESM in a temp dir and import them. */
async function loadWorkflowSelectors() {
  const { default: ts } = await import("typescript");
  const dir = await mkdtemp(join(tmpdir(), "workflow-step-matcher-"));
  const opts = { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 };
  for (const name of ["warnings", "readModel", "workflowSelectors"]) {
    const src = await readFile(join(modelDir, `${name}.ts`), "utf8");
    let js = ts.transpileModule(src, { compilerOptions: opts }).outputText;
    // Point runtime imports (workflowSelectors -> ./readModel -> ./warnings) at emitted files.
    js = js.replace(/from\s+["']\.\/warnings["']/g, 'from "./warnings.mjs"');
    js = js.replace(/from\s+["']\.\/readModel["']/g, 'from "./readModel.mjs"');
    await writeFile(join(dir, `${name}.mjs`), js, "utf8");
  }
  const mod = await import(pathToFileURL(join(dir, "workflowSelectors.mjs")).href);
  await rm(dir, { recursive: true, force: true });
  return mod;
}

function step(id, label, status = "completed") {
  return { id, run_id: "r1", phase: "p1", label, status, started_at: "2026-01-01T00:00:00Z" };
}

async function main() {
  console.log("== Dashboard workflow-step matcher checks (matchRuntimeSteps) ==");
  const { matchRuntimeSteps } = await loadWorkflowSelectors();

  // 1) Common case: all plan labels parse and match by label. Order in the
  //    runtime array must not matter; each row gets its own labeled step.
  {
    const plan = [{ label: "a" }, { label: "b" }, { label: "c" }];
    const runtime = [step("s-b", "b"), step("s-c", "c"), step("s-a", "a")];
    const matched = matchRuntimeSteps(plan, runtime);
    const ids = matched.map((s) => s?.id);
    if (ids[0] === "s-a" && ids[1] === "s-b" && ids[2] === "s-c") {
      ok("all-labels-match: each plan row gets its own labeled step regardless of runtime order");
    } else {
      bad(`all-labels-match: expected [s-a,s-b,s-c], got [${ids.join(",")}]`);
    }
  }

  // 2) Dynamic/out-of-order fan-out: runtime=[c] only (mid-flight), plan=[a,b,c].
  //    Plan row "a" must NOT borrow step c positionally; it must render as
  //    unmatched (undefined/"not started"). Only row "c" gets the real step.
  {
    const plan = [{ label: "a" }, { label: "b" }, { label: "c" }];
    const runtime = [step("s-c", "c")];
    const matched = matchRuntimeSteps(plan, runtime);
    if (matched[0] === undefined && matched[1] === undefined && matched[2]?.id === "s-c") {
      ok("mid-flight out-of-order: unmatched labeled rows stay pending, no positional borrow");
    } else {
      bad(`mid-flight out-of-order: expected [undefined,undefined,s-c], got [${matched.map((s) => s?.id).join(",")}]`);
    }
  }

  // 3) A runtime step already consumed by label must never be returned again
  //    positionally for a different row (duplicate-attribution guard), even
  //    when another plan row has no label of its own.
  {
    const plan = [{ label: "a" }, { label: undefined }];
    const runtime = [step("s-a", "a")];
    const matched = matchRuntimeSteps(plan, runtime);
    if (matched[0]?.id === "s-a" && matched[1] === undefined) {
      ok("consumed-step guard: a labeled match is never reused for an unlabeled row");
    } else {
      bad(`consumed-step guard: expected [s-a,undefined], got [${matched.map((s) => s?.id).join(",")}]`);
    }
  }

  // 4) Fully label-less plan: position is the only signal, so positional
  //    fallback is allowed for every row.
  {
    const plan = [{ label: undefined }, { label: undefined }];
    const runtime = [step("s-0", ""), step("s-1", "")];
    const matched = matchRuntimeSteps(plan, runtime);
    if (matched[0]?.id === "s-0" && matched[1]?.id === "s-1") {
      ok("fully label-less plan: positional fallback applies to every row");
    } else {
      bad(`fully label-less plan: expected [s-0,s-1], got [${matched.map((s) => s?.id).join(",")}]`);
    }
  }

  // 5) No runtime steps at all: every row is unmatched, regardless of labels.
  {
    const plan = [{ label: "a" }, { label: undefined }];
    const matched = matchRuntimeSteps(plan, []);
    if (matched.every((s) => s === undefined)) {
      ok("no runtime steps: every plan row is unmatched");
    } else {
      bad(`no runtime steps: expected all undefined, got [${matched.map((s) => s?.id).join(",")}]`);
    }
  }

  console.log(`\n   workflow-step-matcher checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`workflow-step-matcher-check crashed: ${error.stack || error}`);
  process.exit(1);
});
