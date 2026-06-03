import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { get } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Acceptance for the Starlark workflow front-end (skill + CLI) — the SOLE
// dynamic authoring surface. In the default (mock/CI) mode it builds the
// harness, authors a small imperative `.star` program (a mandatory
// `workflow(name, design_intent)` header, then a serial `phase`/`agent` chain
// plus a data-driven `parallel()` barrier), runs it through `harness workflow
// run-script --dry-run` (mock delivery, no provider tokens), and asserts a
// WorkflowRun + ordered WorkflowSteps were journaled with the expected serial ->
// parallel shape, that the run snapshots the raw script text as a starlark spec
// AND persists the declared design_intent, that a program WITHOUT a design_intent
// is rejected fail-fast, and that the run/steps read back from the durable store.
// `--live` swaps the dry-run for real provider delivery (spends tokens).

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const live = argv.includes("--live");
const keepStore = argv.includes("--keep-store");
const store =
  valueArg("--store") ?? mkdtempSync(join(tmpdir(), "mah-workflow-starlark-"));
const harness = valueArg("--harness") ?? join(repoRoot, "target/debug/harness");
const results = [];

function valueArg(name) {
  const index = argv.indexOf(name);
  return index >= 0 ? argv[index + 1] : undefined;
}

function stage(id, title, fn) {
  const startedAt = Date.now();
  try {
    const evidence = fn() ?? {};
    results.push({
      id,
      title,
      status: "pass",
      duration_ms: Date.now() - startedAt,
      evidence,
    });
  } catch (error) {
    results.push({
      id,
      title,
      status: "fail",
      duration_ms: Date.now() - startedAt,
      error: error instanceof Error ? error.message : String(error),
    });
    finish(1);
  }
}

async function stageAsync(id, title, fn) {
  const startedAt = Date.now();
  try {
    const evidence = (await fn()) ?? {};
    results.push({
      id,
      title,
      status: "pass",
      duration_ms: Date.now() - startedAt,
      evidence,
    });
  } catch (error) {
    results.push({
      id,
      title,
      status: "fail",
      duration_ms: Date.now() - startedAt,
      error: error instanceof Error ? error.message : String(error),
    });
    finish(1);
  }
}

function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, {
    cwd: options.cwd ?? repoRoot,
    env: { ...process.env, ...(options.env ?? {}) },
    encoding: "utf8",
    input: options.input,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(
      [
        `${command} ${commandArgs.join(" ")} failed with ${result.status}`,
        result.stdout.trim(),
        result.stderr.trim(),
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }
  return result.stdout;
}

function harnessJson(commandArgs, options = {}) {
  const stdout = run(harness, commandArgs, {
    ...options,
    env: { HARNESS_ROOT: store, ...(options.env ?? {}) },
  });
  return JSON.parse(stdout);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function httpJson(url) {
  return new Promise((resolvePromise, reject) => {
    get(url, (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => {
        body += chunk;
      });
      response.on("end", () => {
        if ((response.statusCode ?? 500) >= 400) {
          reject(new Error(`${url} returned ${response.statusCode}: ${body}`));
          return;
        }
        try {
          resolvePromise(JSON.parse(body));
        } catch (error) {
          reject(error);
        }
      });
    }).on("error", reject);
  });
}

async function fetchWithRetry(url, attempts = 40) {
  let lastError;
  for (let i = 0; i < attempts; i += 1) {
    try {
      return await httpJson(url);
    } catch (error) {
      lastError = error;
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
    }
  }
  throw lastError;
}

// Author the Starlark program: an imperative serial `phase`/`agent` chain
// followed by a data-driven `parallel()` barrier. The script reads its
// parameterization from the injected `args` global (so the run journals the
// args too), chains the planner's output text into the audit prompts, and
// fans the audit out across BOTH providers via a list comprehension. Each
// step carries an explicit `phase`/`label` so the journaled steps assert the
// shape deterministically.
const DESIGN_INTENT =
  "Plan once, then fan the audit out across both providers in isolated worktrees, " +
  "and finally synthesize — so independent audits cannot collide and the report sees all of them.";

function authorScript() {
  const script = `workflow("starlark-acceptance", "${DESIGN_INTENT}")

phase("plan")
plan = agent("Plan an investigation of " + args["topic"], label = "planner")

phase("audit")
audits = parallel([
    {
        "prompt": "Audit " + a["area"] + " for: " + plan,
        "provider": a["provider"],
        "label": a["label"],
        "isolation": "worktree",
    }
    for a in args["areas"]
])

phase("synthesize")
agent(
    "Write the final report for " + args["topic"],
    provider = "claude",
    label = "report",
)
`;
  const scriptDir = join(store, "acceptance");
  mkdirSync(scriptDir, { recursive: true });
  const scriptPath = join(scriptDir, "triage.star");
  writeFileSync(scriptPath, script);
  const runArgs = {
    topic: "the failing login path",
    areas: [
      { area: "code paths", provider: "codex", label: "code-audit" },
      { area: "docs and history", provider: "claude", label: "doc-audit" },
    ],
  };
  return { scriptPath, script, runArgs };
}

// Author a program that OMITS the mandatory `workflow(name, design_intent)`
// header — it must be rejected fail-fast before any run completes.
function authorScriptWithoutDesignIntent() {
  const script = `phase("plan")
agent("Plan an investigation of the failing login path", label = "planner")
`;
  const scriptDir = join(store, "acceptance");
  mkdirSync(scriptDir, { recursive: true });
  const scriptPath = join(scriptDir, "no-intent.star");
  writeFileSync(scriptPath, script);
  return { scriptPath };
}

// Run a program that lacks a design_intent and assert the CLI rejects it with a
// non-zero exit and an error mentioning design_intent.
function assertRejectedWithoutDesignIntent(scriptPath) {
  const result = spawnSync(
    harness,
    ["workflow", "run-script", scriptPath, "--dry-run"],
    {
      cwd: repoRoot,
      env: { ...process.env, HARNESS_ROOT: store },
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
    },
  );
  assert(
    result.status !== 0,
    "run-script must reject a program without a design_intent",
  );
  const combined = `${result.stdout}${result.stderr}`;
  assert(
    /design_intent/.test(combined),
    `rejection must mention design_intent, got: ${combined.trim()}`,
  );
  return { rejected: true };
}

// Initialize the store. Agent steps reference a PROVIDER directly and spin up a
// fresh ephemeral worker per node — there are NO pre-created members to bind, so
// setup is just `harness init`.
function setupStore() {
  run(harness, ["init"], { env: { HARNESS_ROOT: store } });
  return { store };
}

// Run the authored script through the CLI contract. The script reads `args`
// from `--args <json>`; the spec's `provider` values drive delivery, so there
// is no member binding to pass. Mock/CI mode uses --dry-run (mock delivery, no
// tokens); --live spends real provider tokens.
function runScript(scriptPath, runArgs) {
  const cmdArgs = [
    "workflow",
    "run-script",
    scriptPath,
    "--name",
    "starlark-acceptance",
    "--args",
    JSON.stringify(runArgs),
  ];
  if (!live) cmdArgs.push("--dry-run");
  const result = harnessJson(cmdArgs);
  assert(result.run, "run-script must return a run");
  assert(Array.isArray(result.steps), "run-script must return steps");
  return result;
}

// Assert the run's emitted shape: serial plan -> parallel audit (2 providers) ->
// serial synthesize, 4 steps in the right phases, args parameterization carried
// through, and a populated final_output.
function assertShape(result) {
  const { run, steps } = result;
  assert(
    run.workflow_name === "starlark-acceptance",
    `wrong workflow name: ${run.workflow_name}`,
  );
  assert(run.status === "completed", `run not completed: ${run.status}`);
  assert(
    run.design_intent === DESIGN_INTENT,
    `run.design_intent not carried through: ${run.design_intent}`,
  );
  assert(
    run.args && run.args.topic === "the failing login path",
    "run.args parameterization not journaled",
  );
  // 1 serial plan + 2 parallel audit siblings + 1 serial synthesize = 4 steps.
  assert(steps.length === 4, `expected 4 steps, got ${steps.length}`);

  const phases = steps.map((step) => step.phase);
  assert(phases[0] === "plan", `step 0 phase: ${phases[0]}`);
  // The parallel barrier journals its two siblings (order is input order).
  const auditPhases = phases.slice(1, 3);
  assert(
    auditPhases.every((phase) => phase === "audit"),
    `parallel phases: ${auditPhases.join(",")}`,
  );
  assert(phases[3] === "synthesize", `step 3 phase: ${phases[3]}`);

  const labels = steps.map((step) => step.label).sort();
  assert(
    JSON.stringify(labels) ===
      JSON.stringify(["code-audit", "doc-audit", "planner", "report"]),
    `labels: ${labels.join(",")}`,
  );

  // Both providers participated (the provider is journaled in each step's result).
  const providers = new Set(steps.map((step) => step.result?.provider));
  assert(providers.has("codex"), "codex provider missing from steps");
  assert(providers.has("claude"), "claude provider missing from steps");
  // The worktree-isolation opt-in survives onto the audit steps' result.
  const isolated = steps.filter(
    (step) => step.result?.isolation === "worktree",
  );
  assert(
    isolated.length === 2,
    `expected 2 worktree-isolated steps, got ${isolated.length}`,
  );

  assert(Array.isArray(run.final_output), "final_output must be present");
  assert(
    run.final_output.length === steps.length,
    `final_output entries (${run.final_output.length}) must match steps (${steps.length})`,
  );
  // agents_spawned is the scheduler counter delta; the parallel barrier routes
  // its fan-out through the scheduler, so the run must report a positive count.
  assert(
    run.agents_spawned >= 2,
    `agents_spawned should be >= 2 (parallel barrier), got ${run.agents_spawned}`,
  );
  return {
    run_id: run.id,
    step_count: steps.length,
    phases,
    agents_spawned: run.agents_spawned,
  };
}

// Read the run back out of the durable store to prove it was journaled (not
// just returned from the in-memory dispatch). The dashboard snapshot is the
// same projection the UI renders, so this also proves the surface can see it.
// Unlike the JSON IR, the script body is not a serializable spec, so the run
// snapshots the raw script text under spec = {"lang":"starlark","script": ...};
// assert that audit record reads back too.
function assertJournaled(runId, scriptText) {
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const run = snapshot.workflow_runs.find((item) => item.id === runId);
  assert(run, `run ${runId} not journaled to the store`);
  assert(run.status === "completed", "journaled run not completed");
  assert(
    run.spec && run.spec.lang === "starlark",
    "journaled run must snapshot a starlark spec",
  );
  assert(
    run.spec.script === scriptText,
    "journaled run must snapshot the raw script text",
  );
  assert(
    run.design_intent === DESIGN_INTENT,
    "journaled run must persist the declared design_intent",
  );
  const steps = snapshot.workflow_steps.filter(
    (item) => item.run_id === runId,
  );
  assert(steps.length === 4, `journaled steps: ${steps.length}`);
  assert(
    steps.every((step) => step.status === "completed"),
    "journaled steps not all completed",
  );
  return {
    journaled_runs: snapshot.workflow_runs.length,
    journaled_steps: steps.length,
  };
}

// Prove the run is visible over the live HTTP snapshot the dashboard consumes.
async function assertLiveApi(runId) {
  const port = 18888 + Math.floor(Math.random() * 1000);
  const server = spawn(harness, ["serve", "--addr", `127.0.0.1:${port}`, "--once"], {
    cwd: repoRoot,
    env: { ...process.env, HARNESS_ROOT: store },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  server.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });
  const liveSnapshot = await fetchWithRetry(`http://127.0.0.1:${port}/v1/snapshot`);
  await new Promise((resolvePromise) => server.once("exit", resolvePromise));
  assert(!stderr.trim(), `serve stderr was not empty: ${stderr}`);
  const run = (liveSnapshot.workflow_runs ?? []).find((item) => item.id === runId);
  assert(run, "starlark run missing from live API snapshot");
  return { api_runs: liveSnapshot.workflow_runs.length };
}

if (!existsSync(harness)) {
  run("cargo", ["build", "-p", "harness-cli"]);
}

let authored;
let runResult;
let shape;

stage("s0", "harness binary is built", () => {
  if (!existsSync(harness)) run("cargo", ["build", "-p", "harness-cli"]);
  assert(existsSync(harness), `harness binary not found: ${harness}`);
  return { harness, mode: live ? "live" : "mock" };
});
stage("s1", "store initialized (ephemeral providers, no members)", () => {
  return setupStore();
});
stage("s2", "agent authors a Starlark workflow program (.star)", () => {
  authored = authorScript();
  return {
    script_path: authored.scriptPath,
    primitives: ["phase", "agent", "parallel"],
  };
});
stage("s3", "harness workflow run-script evaluates the program", () => {
  runResult = runScript(authored.scriptPath, authored.runArgs);
  return { run_id: runResult.run.id, steps: runResult.steps.length };
});
stage("s4", "run has serial -> parallel -> serial shape + final_output", () => {
  shape = assertShape(runResult);
  return shape;
});
stage("s5", "WorkflowRun + steps journaled; script text + design_intent snapshotted", () =>
  assertJournaled(runResult.run.id, authored.script),
);
stage("s6", "program WITHOUT a design_intent is rejected fail-fast", () => {
  const { scriptPath } = authorScriptWithoutDesignIntent();
  return assertRejectedWithoutDesignIntent(scriptPath);
});
await stageAsync("s7", "starlark run visible over the live dashboard API", () =>
  assertLiveApi(runResult.run.id),
);

finish(0);

function finish(exitCode) {
  const report = {
    status: exitCode === 0 ? "pass" : "fail",
    mode: live ? "live" : "mock",
    store,
    results,
  };
  console.log(JSON.stringify(report, null, 2));
  if (exitCode === 0 && !keepStore && store.startsWith(tmpdir())) {
    rmSync(store, { recursive: true, force: true });
  }
  process.exit(exitCode);
}
