// Workflow evaluation harness runner. For each task it runs the `baseline` arm
// (a single agent) and the `workflow` arm (an orchestration program) through the
// SAME runtime (`harness workflow run-script`), `repeats` times each, grades the
// run's STRUCTURED findings with the task's objective grader, and reports mean
// quality + cost + variance + win-rate per task/category.
//
// Usage:
//   node scripts/eval-workflows.mjs [--task <id>] [--repeats N] [--dry-run]
//                                   [--store <dir>] [--harness <path>]
// --dry-run uses the mock provider (no tokens) to validate the plumbing; scores
// are then meaningless (mock findings) but the loop is exercised end to end.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const dryRun = argv.includes("--dry-run");
const harness = valueArg("--harness") ?? join(repoRoot, "target/debug/harness");
const tasksRoot = join(repoRoot, "evals/tasks");
const onlyTask = valueArg("--task");
const repeatsOverride = valueArg("--repeats") ? Number(valueArg("--repeats")) : undefined;
// Per-node ephemeral-worker timeout. The eval's codex/claude turns are short
// (~30-60s), so 3 min bounds a wedged worker without killing healthy ones.
const timeoutMs = valueArg("--timeout-ms") ?? "180000";
const ARMS = ["baseline", "workflow"];

function valueArg(name) {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
}

function parseMs(ts) {
  const m = /^unix-ms:(\d+)$/.exec(ts ?? "");
  return m ? Number(m[1]) : NaN;
}

/** Run one arm once; return { findings, tokens, wallMs, workers, status }. */
function runArm(task, arm) {
  const store = mkdtempSync(join(tmpdir(), `mah-eval-${task.id}-`));
  const program = join(tasksRoot, task.id, `${arm}.star`);
  const args = JSON.stringify({ subject: task.subjectText });
  const cmd = [
    "workflow",
    "run-script",
    program,
    "--args",
    args,
    "--initiated-by",
    `eval:${arm}`,
    "--timeout-ms",
    timeoutMs,
  ];
  if (dryRun) cmd.push("--dry-run");
  const res = spawnSync(harness, cmd, {
    cwd: repoRoot,
    env: { ...process.env, HARNESS_ROOT: store },
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (res.status !== 0) {
    throw new Error(`${arm} run failed (${res.status}): ${res.stderr?.slice(0, 800)}`);
  }
  const out = JSON.parse(res.stdout);
  const steps = out.steps ?? [];
  // The final structured findings = the LAST step whose result carries them.
  let findings = [];
  for (const step of steps) {
    const structured = step.result?.structured;
    if (structured && Array.isArray(structured.findings)) findings = structured.findings;
  }
  const tokens = steps.reduce((sum, s) => sum + (s.result?.tokens?.total ?? 0), 0);
  const wallMs = parseMs(out.run?.ended_at) - parseMs(out.run?.created_at);
  return {
    findings,
    tokens,
    wallMs: Number.isNaN(wallMs) ? null : wallMs,
    workers: steps.length,
    status: out.run?.status,
  };
}

function mean(xs) {
  return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0;
}
function stdev(xs) {
  if (xs.length < 2) return 0;
  const m = mean(xs);
  return Math.sqrt(mean(xs.map((x) => (x - m) ** 2)));
}

async function main() {
  if (!existsSync(harness)) {
    const build = spawnSync("cargo", ["build", "-p", "harness-cli"], { cwd: repoRoot, stdio: "inherit" });
    if (build.status !== 0) throw new Error("cargo build failed");
  }

  const taskIds = readdirSync(tasksRoot).filter(
    (id) => existsSync(join(tasksRoot, id, "task.json")) && (!onlyTask || id === onlyTask),
  );
  const report = { mode: dryRun ? "dry-run" : "live", tasks: [] };

  for (const id of taskIds) {
    const task = JSON.parse(readFileSync(join(tasksRoot, id, "task.json"), "utf8"));
    task.subjectText = task.subject
      ? readFileSync(join(tasksRoot, id, task.subject), "utf8")
      : "";
    const repeats = repeatsOverride ?? task.repeats ?? 3;
    const { grade } = await import(join(repoRoot, "evals/graders", `${task.grader}.mjs`));

    const arms = {};
    for (const arm of ARMS) {
      const runs = [];
      for (let i = 0; i < repeats; i += 1) {
        const r = runArm(task, arm);
        const g = grade({ findings: r.findings, arm });
        runs.push({ ...r, score: g.score, signals: g.signals });
        process.stderr.write(
          `  ${id} · ${arm} · run ${i + 1}/${repeats}: score=${g.score} ` +
            `tokens=${r.tokens} signals=${JSON.stringify(g.signals)}\n`,
        );
      }
      const scores = runs.map((r) => r.score);
      arms[arm] = {
        meanScore: Number(mean(scores).toFixed(3)),
        scoreStdev: Number(stdev(scores).toFixed(3)),
        meanTokens: Math.round(mean(runs.map((r) => r.tokens))),
        meanWallMs: Math.round(mean(runs.map((r) => r.wallMs ?? 0))),
        meanWorkers: Number(mean(runs.map((r) => r.workers)).toFixed(1)),
        runs,
      };
    }

    const b = arms.baseline;
    const w = arms.workflow;
    const qualityDelta = Number((w.meanScore - b.meanScore).toFixed(3));
    const costRatio = b.meanTokens ? Number((w.meanTokens / b.meanTokens).toFixed(2)) : null;
    report.tasks.push({
      id,
      title: task.title,
      category: task.category,
      repeats,
      baseline: b,
      workflow: w,
      qualityDelta,
      costRatio,
      verdict:
        qualityDelta > 0
          ? `workflow +${qualityDelta} quality at ${costRatio}x cost`
          : qualityDelta === 0
            ? `no quality gain (${costRatio}x cost) — structure did not help here`
            : `workflow WORSE by ${qualityDelta} (${costRatio}x cost)`,
    });
  }

  writeFileSync(join(repoRoot, "evals/report.json"), JSON.stringify(report, null, 2));

  // Markdown summary to stdout.
  console.log(`\n# Workflow eval report (${report.mode})\n`);
  console.log("| task | category | baseline | workflow | Δquality | cost× | verdict |");
  console.log("| --- | --- | --- | --- | --- | --- | --- |");
  for (const t of report.tasks) {
    console.log(
      `| ${t.id} | ${t.category} | ${t.baseline.meanScore}±${t.baseline.scoreStdev} ` +
        `(${t.baseline.meanTokens}tok) | ${t.workflow.meanScore}±${t.workflow.scoreStdev} ` +
        `(${t.workflow.meanTokens}tok) | ${t.qualityDelta >= 0 ? "+" : ""}${t.qualityDelta} | ` +
        `${t.costRatio}× | ${t.verdict} |`,
    );
  }
  console.log(`\nFull results: evals/report.json`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
