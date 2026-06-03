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

// Acceptance for the Dynamic Workflow Runtime (skill + CLI). Modeled on
// scripts/acceptance-mvp.mjs. In the default (mock/CI) mode it builds the
// harness, authors a 2-provider dynamic WorkflowSpec JSON-IR, runs it through
// `harness workflow run-spec --dry-run` (mock delivery, no provider tokens),
// and asserts a WorkflowRun + ordered WorkflowSteps were journaled with the
// expected serial -> parallel -> pipeline shape and a populated final_output.
// `--live` swaps the dry-run for real provider delivery (spends tokens).

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const live = argv.includes("--live");
const keepStore = argv.includes("--keep-store");
const store =
  valueArg("--store") ?? mkdtempSync(join(tmpdir(), "mah-dynamic-workflow-"));
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

// Author the dynamic WorkflowSpec: a 2-provider serial -> parallel -> pipeline
// shape. The leading codex `plan` step is serial; the two-provider `audit`
// barrier fans out across both providers (each opting into worktree isolation);
// the `synthesize` pipeline streams a codex stage into a claude stage. Each node
// carries an explicit `phase` so the journaled steps assert the shape
// deterministically.
function authorSpec() {
  const spec = {
    name: "dynamic-acceptance",
    args: { topic: "the failing login path" },
    nodes: [
      {
        type: "agent",
        provider: "codex",
        phase: "plan",
        label: "planner",
        prompt: "Plan an investigation of {{topic}}.",
      },
      {
        type: "parallel",
        nodes: [
          {
            type: "agent",
            provider: "codex",
            phase: "audit",
            label: "code-audit",
            prompt: "Audit the code paths for {{topic}}.",
            isolation: "worktree",
          },
          {
            type: "agent",
            provider: "claude",
            phase: "audit",
            label: "doc-audit",
            prompt: "Audit the docs and history for {{topic}}.",
            isolation: "worktree",
          },
        ],
      },
      {
        type: "pipeline",
        stages: [
          {
            type: "agent",
            provider: "codex",
            phase: "synthesize",
            label: "collate",
            prompt: "Collate the audit findings for {{topic}}.",
          },
          {
            type: "agent",
            provider: "claude",
            phase: "synthesize",
            label: "report",
            prompt: "Write the final report for {{topic}}.",
          },
        ],
      },
    ],
  };
  const specDir = join(store, "acceptance");
  mkdirSync(specDir, { recursive: true });
  const specPath = join(specDir, "dynamic-acceptance.json");
  writeFileSync(specPath, JSON.stringify(spec, null, 2));
  return { specPath, spec };
}

// Initialize the store. Agent nodes reference a PROVIDER directly and spin up a
// fresh ephemeral worker per node — there are NO pre-created members to bind, so
// setup is just `harness init`.
function setupStore() {
  run(harness, ["init"], { env: { HARNESS_ROOT: store } });
  return { store };
}

// Run the authored spec through the CLI contract. The spec's `provider` values
// drive delivery, so there is no member binding to pass. Mock/CI mode uses
// --dry-run (mock delivery, no tokens); --live spends real provider tokens.
function runSpec(specPath) {
  const runArgs = ["workflow", "run-spec", specPath];
  if (!live) runArgs.push("--dry-run");
  const result = harnessJson(runArgs);
  assert(result.run, "run-spec must return a run");
  assert(Array.isArray(result.steps), "run-spec must return steps");
  return result;
}

// Assert the run's emitted shape: 2 providers, serial -> parallel -> pipeline,
// 4 steps in the right phases, and a populated final_output.
function assertShape(result) {
  const { run, steps } = result;
  assert(run.workflow_name === "dynamic-acceptance", "wrong workflow name");
  assert(run.status === "completed", `run not completed: ${run.status}`);
  assert(
    run.args && run.args.topic === "the failing login path",
    "run.args parameterization not journaled",
  );
  // 1 serial plan + 2 parallel audit siblings + 2 pipeline stages = 5 steps.
  assert(steps.length === 5, `expected 5 steps, got ${steps.length}`);

  const phases = steps.map((step) => step.phase);
  assert(phases[0] === "plan", `step 0 phase: ${phases[0]}`);
  // The parallel barrier journals its two siblings (order is collection order).
  const auditPhases = phases.slice(1, 3);
  assert(
    auditPhases.every((phase) => phase === "audit"),
    `parallel phases: ${auditPhases.join(",")}`,
  );
  // The pipeline streams two synthesize stages, in order, after the barrier.
  const pipelinePhases = phases.slice(3, 5);
  assert(
    pipelinePhases.every((phase) => phase === "synthesize"),
    `pipeline phases: ${pipelinePhases.join(",")}`,
  );

  const labels = steps.map((step) => step.label).sort();
  assert(
    JSON.stringify(labels) ===
      JSON.stringify([
        "code-audit",
        "collate",
        "doc-audit",
        "planner",
        "report",
      ]),
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
function assertJournaled(runId) {
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const run = snapshot.workflow_runs.find((item) => item.id === runId);
  assert(run, `run ${runId} not journaled to the store`);
  assert(run.status === "completed", "journaled run not completed");
  const steps = snapshot.workflow_steps.filter(
    (item) => item.run_id === runId,
  );
  assert(steps.length === 5, `journaled steps: ${steps.length}`);
  assert(
    steps.every((step) => step.status === "completed"),
    "journaled steps not all completed",
  );
  return { journaled_runs: snapshot.workflow_runs.length, journaled_steps: steps.length };
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
  assert(run, "dynamic run missing from live API snapshot");
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
stage("s2", "agent authors a dynamic WorkflowSpec (JSON-IR)", () => {
  authored = authorSpec();
  return { spec_path: authored.specPath, node_kinds: ["agent", "parallel", "pipeline"] };
});
stage("s3", "harness workflow run-spec runs the IR", () => {
  runResult = runSpec(authored.specPath);
  return { run_id: runResult.run.id, steps: runResult.steps.length };
});
stage("s4", "run has serial -> parallel -> pipeline shape + final_output", () => {
  shape = assertShape(runResult);
  return shape;
});
stage("s5", "WorkflowRun + steps are journaled to the store", () =>
  assertJournaled(runResult.run.id),
);
await stageAsync("s6", "dynamic run visible over the live dashboard API", () =>
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
