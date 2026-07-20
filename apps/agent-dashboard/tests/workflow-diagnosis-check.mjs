#!/usr/bin/env node
// Dashboard workflow failure-diagnosis verification (issue #194).
//
// Proves the load-bearing classification in src/model/workflowSelectors.ts
// that the run cards / run detail / Goal Workbench phase panels branch on:
//
//  - terminalReasonInfo: every WorkflowTerminalReason wire token maps to a
//    human class chip (label/gloss/tone) and the abandoned-run classes
//    (canceled_by_operator / driver_exited / orphan_reaped) are flagged
//    `abandoned` — the "abandoned run vs leaf-level provider failure"
//    distinction the issue calls out. Unknown/legacy values return undefined
//    (old snapshots must render without a bogus chip).
//  - workflowRunVerdictInfo: reads verdict ok/reason + success_criterion off
//    `final_output` (the shape `verdict(ok, reason=...)` journals), tolerant
//    of runs that never reached their verdict call.
//  - splitPartialOutputSteps: separates usable (completed/cached) steps from
//    failed/reaped/canceled/running ones — the partial-output core ask.
//  - schemaSelectionInfo: surfaces the #192 metadata (attempt count /
//    selected candidate index / empty-field count / strict) and flags the
//    "looked valid but empty" trap (empty_field_count > 0); text-mode steps
//    (no schema_attempt_count) yield undefined.
//
// Mirrors the transpile pattern of tests/workflow-step-matcher-check.mjs: it
// exercises the REAL implementations by transpiling workflowSelectors.ts (and
// its transitive runtime deps) with the TypeScript compiler API and importing
// the emitted ESM.
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
  const dir = await mkdtemp(join(tmpdir(), "workflow-diagnosis-"));
  const opts = { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 };
  for (const name of ["workflowSelectors"]) {
    const src = await readFile(join(modelDir, `${name}.ts`), "utf8");
    let js = ts.transpileModule(src, { compilerOptions: opts }).outputText;
    await writeFile(join(dir, `${name}.mjs`), js, "utf8");
  }
  const mod = await import(pathToFileURL(join(dir, "workflowSelectors.mjs")).href);
  await rm(dir, { recursive: true, force: true });
  return mod;
}

function step(id, status, extra = {}) {
  return {
    id,
    run_id: "r1",
    phase: "p1",
    label: id,
    status,
    started_at: "unix-ms:1783338433971",
    ...extra,
  };
}

async function main() {
  console.log("== Dashboard workflow failure-diagnosis checks (issue #194) ==");
  const {
    terminalReasonInfo,
    workflowRunVerdictInfo,
    splitPartialOutputSteps,
    schemaSelectionInfo,
  } = await loadWorkflowSelectors();

  // 1) Every wire token classifies; abandoned classes are flagged.
  {
    const reasons = [
      "canceled_by_operator",
      "driver_exited",
      "orphan_reaped",
      "leaf_timeout",
      "idle_timeout",
      "provider_failed",
      "verdict_failed",
      "completed",
    ];
    const abandoned = new Set(["canceled_by_operator", "driver_exited", "orphan_reaped"]);
    const missing = reasons.filter((r) => {
      const info = terminalReasonInfo(r);
      return !info || !info.label || !info.gloss || info.abandoned !== abandoned.has(r);
    });
    if (missing.length === 0) {
      ok("terminalReasonInfo: all 8 wire tokens map to label+gloss with correct abandoned flag");
    } else {
      bad(`terminalReasonInfo: bad mapping for [${missing.join(",")}]`);
    }
  }

  // 2) The acceptance-store canceled run's reason reads as the operator class.
  {
    const info = terminalReasonInfo("canceled_by_operator");
    if (info?.label === "canceled by operator" && info.tone === "warn" && info.abandoned) {
      ok("canceled_by_operator: chip reads 'canceled by operator' (warn, abandoned)");
    } else {
      bad(`canceled_by_operator: got ${JSON.stringify(info)}`);
    }
  }

  // 3) Unknown / legacy values yield no chip (old snapshots render clean).
  {
    if (
      terminalReasonInfo(undefined) === undefined &&
      terminalReasonInfo(null) === undefined &&
      terminalReasonInfo("") === undefined &&
      terminalReasonInfo("some_future_reason") === undefined
    ) {
      ok("terminalReasonInfo: undefined/null/empty/unknown values classify as no-chip");
    } else {
      bad("terminalReasonInfo: legacy/unknown value produced a chip");
    }
  }

  // 4) Verdict + success criterion read off final_output; absent shapes tolerated.
  {
    const run = {
      id: "r",
      workflow_name: "w",
      status: "completed",
      step_ids: [],
      created_at: "unix-ms:0",
      final_output: {
        success_criterion: "strict schema selects a non-empty candidate",
        verdict: { ok: true, reason: "strict schema returned non-empty structured content" },
      },
    };
    const v = workflowRunVerdictInfo(run);
    const none = workflowRunVerdictInfo({ ...run, final_output: undefined });
    if (
      v.ok === true &&
      v.reason === "strict schema returned non-empty structured content" &&
      v.successCriterion === "strict schema selects a non-empty candidate" &&
      none.ok === undefined && none.reason === undefined && none.successCriterion === undefined
    ) {
      ok("workflowRunVerdictInfo: reads ok/reason/success_criterion; empty final_output yields {}");
    } else {
      bad(`workflowRunVerdictInfo: got ${JSON.stringify({ v, none })}`);
    }
  }

  // 5) Partial-output split: completed/cached usable; failed/running/queued not.
  {
    const steps = [
      step("done-1", "completed"),
      step("cache-1", "cached"),
      step("dead-1", "failed", { terminal_reason: "canceled_by_operator" }),
      step("live-1", "running"),
      step("wait-1", "queued"),
    ];
    const { usable, invalid } = splitPartialOutputSteps(steps);
    const usableIds = usable.map((s) => s.id).join(",");
    const invalidIds = invalid.map((s) => s.id).join(",");
    if (usableIds === "done-1,cache-1" && invalidIds === "dead-1,live-1,wait-1") {
      ok("splitPartialOutputSteps: usable=[completed,cached], invalid=[failed,running,queued]");
    } else {
      bad(`splitPartialOutputSteps: usable=[${usableIds}] invalid=[${invalidIds}]`);
    }
  }

  // 6) Schema metadata: the strict acceptance-run shape (1 attempt, candidate
  //    0 of 1, 0 empty fields, strict) surfaces verbatim; no empty-field flag.
  {
    const s = step("strict-probe", "completed", {
      result: {
        ok: true,
        schema_attempt_count: 1,
        selected_json_index: 0,
        schema_candidate_count: 1,
        empty_field_count: 0,
        schema_strict: true,
      },
    });
    const info = schemaSelectionInfo(s);
    if (
      info &&
      info.attemptCount === 1 &&
      info.selectedIndex === 0 &&
      info.candidateCount === 1 &&
      info.emptyFieldCount === 0 &&
      info.strict === true &&
      info.hasEmptyFields === false
    ) {
      ok("schemaSelectionInfo: strict run reads 1 attempt / candidate 0 of 1 / 0 empty fields");
    } else {
      bad(`schemaSelectionInfo strict: got ${JSON.stringify(info)}`);
    }
  }

  // 7) The "looked valid but empty" trap is flagged; text-mode steps opt out.
  {
    const empty = schemaSelectionInfo(
      step("gate", "completed", {
        result: { ok: true, schema_attempt_count: 2, selected_json_index: 1, empty_field_count: 2 },
      }),
    );
    const text = schemaSelectionInfo(step("essay", "completed", { result: { ok: true } }));
    const bare = schemaSelectionInfo(step("legacy", "completed"));
    if (empty?.hasEmptyFields === true && empty.emptyFieldCount === 2 && text === undefined && bare === undefined) {
      ok("schemaSelectionInfo: empty_field_count>0 flags the trap; text/legacy steps yield undefined");
    } else {
      bad(`schemaSelectionInfo trap: got ${JSON.stringify({ empty, text, bare })}`);
    }
  }

  console.log(`\n   workflow-diagnosis checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`workflow-diagnosis-check crashed: ${error.stack || error}`);
  process.exit(1);
});
