import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { get } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = new Set(process.argv.slice(2));
const liveCodex = args.has("--live-codex");
const skipStatic = args.has("--skip-static");
const keepStore = args.has("--keep-store");
const store =
  valueArg("--store") ?? mkdtempSync(join(tmpdir(), "mah-acceptance-"));
const harness = valueArg("--harness") ?? join(repoRoot, "target/debug/harness");
const results = [];

function valueArg(name) {
  const argv = process.argv.slice(2);
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

function skipped(id, title, reason) {
  results.push({ id, title, status: "skipped", reason });
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

function writeArtifact(relativePath, content) {
  const path = join(store, "acceptance", relativePath);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
  return path;
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

function staticGates() {
  run("cargo", ["fmt", "--all", "--", "--check"]);
  run("cargo", ["clippy", "--all-targets", "--", "-D", "warnings"]);
  run("cargo", ["test", "--workspace"]);
  run("npx", ["pnpm@9.15.4", "check"]);
  run("cargo", ["build", "-p", "harness-cli"]);
  assert(existsSync(harness), `harness binary not found: ${harness}`);
  return { commands: ["cargo fmt", "cargo clippy", "cargo test", "pnpm check"] };
}

function createMember(id, name, role, extra = []) {
  return harnessJson([
    "agent",
    "create",
    "--id",
    id,
    "--name",
    name,
    "--role",
    role,
    "--provider",
    "codex",
    "--skill",
    "skills/generic-agent-harness/SKILL.md",
    ...extra,
  ]);
}

function createBaseObjects() {
  run(harness, ["init"], { env: { HARNESS_ROOT: store } });
  createMember("lead", "Self-host Lead", "lead");
  createMember("worker", "Implementation Worker", "worker");
  createMember("critic", "Critic Gate", "critic");
  createMember("dashboard-reviewer", "Dashboard Reviewer", "dashboard");
  const team = harnessJson([
    "team",
    "create",
    "--id",
    "team-self-host",
    "--name",
    "Self-host MVP Team",
    "--description",
    "Persistent harness team used by MVP acceptance smoke.",
    "--owner",
    "lead",
    "--member",
    "lead",
    "--member",
    "worker",
    "--member",
    "critic",
    "--member",
    "dashboard-reviewer",
  ]);
  const goal = harnessJson([
    "goal",
    "create",
    "--id",
    "goal-self-host-mvp",
    "--title",
    "Self-host full MVP",
    "--objective",
    "Prove the harness can drive its own development through goal, task, message, evidence, review, decision, dashboard, and evaluation artifacts.",
    "--owner",
    "lead",
    "--success",
    "A real self-hosting workflow is visible in dashboard state and passes strict goal learning.",
  ]);
  assert(team.member_ids.length === 4, "team must include four members");
  return { team_id: team.id, goal_id: goal.id };
}

function createWorkflowTask() {
  const task = harnessJson([
    "task",
    "create",
    "--id",
    "task-self-host-workflow",
    "--goal",
    "goal-self-host-mvp",
    "--title",
    "Prove self-hosting workflow",
    "--objective",
    "Exercise the MVP object protocol and review gate with durable evidence.",
    "--owner",
    "lead",
    "--reviewer",
    "critic",
    "--workspace",
    repoRoot,
    "--branch",
    "acceptance-smoke",
    "--owned-path",
    "scripts/acceptance-mvp.mjs",
    "--owned-path",
    "docs/mvp.md",
    "--acceptance",
    "Strict goal learning status is clean after evaluation.",
  ]);
  const designRef = writeArtifact(
    "goal-design.md",
    [
      "# Goal Design",
      "",
      "Scenario: self-hosting development.",
      "Team: lead, worker, critic, dashboard reviewer.",
      "Task graph: design -> assignment -> worker report -> critic -> decision -> evaluation.",
    ].join("\n"),
  );
  const design = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-goal-design",
    "--task",
    task.id,
    "--source-type",
    "goal_design",
    "--source-ref",
    designRef,
    "--summary",
    "Self-hosting goal design and task graph.",
  ]);
  const assigned = harnessJson(["task", "assign", "--id", task.id, "--assignee", "worker"]);
  const messages = harnessJson(["message", "list", "--task", task.id]);
  assert(messages.some((message) => message.kind === "task"), "assignment task message missing");
  return { task_id: task.id, design_evidence_id: design.id, status: assigned.status };
}

function recordWorkerEvidence() {
  const workerRef = writeArtifact(
    "worker-report.md",
    "Worker report: acceptance smoke exercised the object protocol and proposed the staged MVP gate.",
  );
  const checkRef = writeArtifact("checks.txt", "acceptance smoke deterministic checks passed");
  const criticRef = writeArtifact(
    "critic-findings.md",
    "Critic findings: evidence, assignment order, proposal evidence, and dashboard visibility are required.",
  );
  const worker = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-worker-report",
    "--task",
    "task-self-host-workflow",
    "--source-type",
    "worker_report",
    "--source-ref",
    workerRef,
    "--summary",
    "Worker output for self-hosting workflow.",
  ]);
  const check = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-check-passed",
    "--task",
    "task-self-host-workflow",
    "--source-type",
    "check_passed",
    "--source-ref",
    checkRef,
    "--summary",
    "Deterministic MVP smoke checks passed.",
  ]);
  const critic = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-critic-findings",
    "--task",
    "task-self-host-workflow",
    "--source-type",
    "critic_findings",
    "--source-ref",
    criticRef,
    "--summary",
    "Critic accepted evidence shape and blocked fake completion paths.",
  ]);
  const report = harnessJson([
    "message",
    "send",
    "--id",
    "msg-worker-report",
    "--from",
    "worker",
    "--task",
    "task-self-host-workflow",
    "--channel",
    "provider-report",
    "--kind",
    "report",
    "--content",
    "Worker completed the deterministic self-hosting smoke and attached evidence.",
    "--evidence",
    worker.id,
  ]);
  return {
    worker_evidence_id: worker.id,
    check_evidence_id: check.id,
    critic_evidence_id: critic.id,
    report_message_id: report.id,
  };
}

function providerFixtureAndIngest() {
  const fixtureRef = writeArtifact(
    "provider-events.jsonl",
    [
      JSON.stringify({
        method: "thread/status/changed",
        params: {
          threadId: "thread-acceptance",
          status: { type: "active" },
        },
      }),
      JSON.stringify({
        method: "turn/completed",
        params: {
          threadId: "thread-acceptance",
          turn: { id: "turn-acceptance", status: "completed" },
        },
      }),
    ].join("\n"),
  );
  const before = harnessJson(["event", "list", "--agent", "worker"]).length;
  const ingest = harnessJson([
    "agent",
    "ingest",
    "--agent",
    "worker",
    "--runtime",
    "runtime-fixture",
    "--task",
    "task-self-host-workflow",
    "--source",
    fixtureRef,
  ]);
  const events = harnessJson(["event", "list", "--agent", "worker"]);
  assert(events.length > before, "provider fixture did not create events");
  return { fixture_ref: fixtureRef, events_ingested: ingest.events_ingested };
}

function negativeReviewGate() {
  harnessJson([
    "task",
    "create",
    "--id",
    "task-negative-review",
    "--goal",
    "goal-self-host-mvp",
    "--title",
    "Negative review gate smoke",
    "--objective",
    "Confirm missing critic evidence blocks acceptance.",
    "--owner",
    "lead",
    "--reviewer",
    "critic",
    "--owned-path",
    "docs/mvp.md",
  ]);
  const designRef = writeArtifact("negative-design.md", "Negative gate design.");
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-negative-design",
    "--task",
    "task-negative-review",
    "--source-type",
    "goal_design",
    "--source-ref",
    designRef,
    "--summary",
    "Design evidence for negative gate smoke.",
  ]);
  harnessJson(["task", "assign", "--id", "task-negative-review", "--assignee", "worker"]);
  const checkRef = writeArtifact("negative-check.txt", "check only; critic intentionally absent");
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-negative-check",
    "--task",
    "task-negative-review",
    "--source-type",
    "check_passed",
    "--source-ref",
    checkRef,
    "--summary",
    "Check-only evidence for negative gate smoke.",
  ]);
  harnessJson([
    "proposal",
    "create",
    "--id",
    "proposal-negative",
    "--task",
    "task-negative-review",
    "--agent",
    "worker",
    "--title",
    "Incomplete proposal",
    "--summary",
    "Missing critic and worker output by design.",
    "--changed-path",
    "docs/mvp.md",
    "--evidence",
    "evidence-negative-check",
  ]);
  const result = spawnSync(
    harness,
    [
      "review",
      "gate",
      "--task",
      "task-negative-review",
      "--reviewer",
      "critic",
      "--decision",
      "accept",
      "--rationale",
      "This must fail because critic evidence is missing.",
      "--evidence",
      "evidence-negative-check",
    ],
    {
      cwd: repoRoot,
      env: { ...process.env, HARNESS_ROOT: store },
      encoding: "utf8",
    },
  );
  assert(result.status !== 0, "negative review gate unexpectedly passed");
  assert(
    `${result.stderr}\n${result.stdout}`.includes("critic_findings"),
    "negative review gate failed for the wrong reason",
  );
  return { expected_failure: "missing critic_findings" };
}

function acceptWorkflowTask() {
  const proposal = harnessJson([
    "proposal",
    "create",
    "--id",
    "proposal-self-host-workflow",
    "--task",
    "task-self-host-workflow",
    "--agent",
    "worker",
    "--title",
    "Self-hosting workflow smoke",
    "--summary",
    "Acceptance smoke evidence proves the current object protocol and gates.",
    "--changed-path",
    "scripts/acceptance-mvp.mjs",
    "--changed-path",
    "docs/mvp.md",
    "--evidence",
    "evidence-worker-report",
    "--evidence",
    "evidence-check-passed",
    "--evidence",
    "evidence-critic-findings",
  ]);
  const review = harnessJson([
    "review",
    "gate",
    "--task",
    "task-self-host-workflow",
    "--reviewer",
    "critic",
    "--decision",
    "accept",
    "--rationale",
    "Accepted because worker output, checks, critic findings, proposal evidence, and owned paths are present.",
    "--evidence",
    "evidence-worker-report",
    "--evidence",
    "evidence-check-passed",
    "--evidence",
    "evidence-critic-findings",
    "--goal",
    "goal-self-host-mvp",
  ]);
  assert(review.task.status === "done", "accepted task should be done");
  return { proposal_id: proposal.id, decision_id: review.decision.id };
}

function evaluateGoal() {
  const evaluationRef = writeArtifact(
    "goal-evaluation.md",
    [
      "# Goal Evaluation",
      "",
      "Worked: assignment, worker report, critic evidence, review gate, and dashboard snapshot are durable.",
      "Needs follow-up: trusted Codex plugin activation and streaming dashboard remain staged follow-up work.",
    ].join("\n"),
  );
  const caseRef = writeArtifact(
    "goal-case.md",
    "Reusable case: self-hosting smoke for Goal -> Task -> Message -> Evidence -> Decision -> Evaluation.",
  );
  const evaluation = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-goal-evaluation",
    "--task",
    "task-self-host-workflow",
    "--source-type",
    "goal_evaluation",
    "--source-ref",
    evaluationRef,
    "--summary",
    "Goal evaluation for self-hosting MVP smoke.",
  ]);
  const goalCase = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-goal-case",
    "--task",
    "task-self-host-workflow",
    "--source-type",
    "goal_case",
    "--source-ref",
    caseRef,
    "--summary",
    "Reusable self-hosting acceptance case.",
  ]);
  const followUp = harnessJson([
    "task",
    "create",
    "--id",
    "task-follow-up-trusted-plugin",
    "--goal",
    "goal-self-host-mvp",
    "--parent",
    "task-self-host-workflow",
    "--title",
    "Follow-up: activate trusted harness telemetry plugin",
    "--objective",
    "Turn the harness telemetry plugin scaffold into a trusted managed hook installation.",
    "--owner",
    "lead",
    "--reviewer",
    "critic",
    "--owned-path",
    "plugins/harness-telemetry",
  ]);
  const status = harnessJson([
    "goal",
    "learning-status",
    "--id",
    "goal-self-host-mvp",
    "--strict",
    "--require-evaluation",
  ]);
  assert(status.ok === true, `strict goal learning has warnings: ${status.warnings?.join("; ")}`);
  return {
    evaluation_evidence_id: evaluation.id,
    goal_case_evidence_id: goalCase.id,
    follow_up_task_id: followUp.id,
  };
}

function hookAndPluginBridge() {
  const pluginData = join(store, "plugin-data");
  mkdirSync(pluginData, { recursive: true });
  const pluginRoot = join(repoRoot, "plugins/harness-telemetry");
  run(join(pluginRoot, "scripts/harness-telemetry-hook.sh"), [], {
    input: '{"hook_event_name":"Stop","turn_id":"turn-plugin"}',
    env: {
      HARNESS_ROOT: store,
      HARNESS_AGENT_MEMBER_ID: "worker",
      HARNESS_AGENT_RUNTIME_ID: "runtime-plugin-smoke",
      HARNESS_TASK_ID: "task-self-host-workflow",
      HARNESS_BIN: harness,
      PLUGIN_ROOT: pluginRoot,
      PLUGIN_DATA: pluginData,
    },
  });
  const hookResult = spawnSync(join(pluginRoot, "scripts/harness-telemetry-hook.sh"), {
    input: '{"hook_event_name":"Stop","turn_id":"turn-unbound"}',
    cwd: repoRoot,
    env: {
      ...process.env,
      PLUGIN_ROOT: pluginRoot,
      PLUGIN_DATA: pluginData,
    },
    encoding: "utf8",
  });
  if (hookResult.status !== 0) {
    throw new Error(hookResult.stderr || "unbound plugin hook path failed");
  }
  const events = harnessJson(["event", "list", "--agent", "worker"]);
  assert(events.some((event) => event.event_type === "codex_hook.Stop"), "hook record event missing");
  const unboundDir = join(pluginData, "unbound-events");
  assert(existsSync(unboundDir) && readdirSync(unboundDir).length > 0, "unbound plugin fallback missing");
  return { plugin_root: pluginRoot, unbound_events: readdirSync(unboundDir).length };
}

async function dashboardApi() {
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  assert(snapshot.kanban.done.includes("task-self-host-workflow"), "done task missing from dashboard");
  assert(snapshot.goal_learning_status.some((item) => item.goal_id === "goal-self-host-mvp" && item.ok), "goal learning not visible as ok");
  const port = 18787 + Math.floor(Math.random() * 1000);
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
  assert(liveSnapshot.tasks.length >= 2, "live API snapshot missing tasks");
  return { snapshot_tasks: snapshot.tasks.length, api_tasks: liveSnapshot.tasks.length };
}

function earningEngineAdapterSurface() {
  const adapter = JSON.parse(readFileSync(join(repoRoot, "examples/adapters/earning-engine/adapter.json"), "utf8"));
  const descriptorDir = join(repoRoot, "examples/adapters/earning-engine/tool-descriptors");
  const descriptors = readdirSync(descriptorDir).filter((file) => file.endsWith(".json"));
  assert(adapter.adapterId || adapter.name, "earning-engine adapter missing adapter id");
  assert(descriptors.length >= 8, "earning-engine adapter needs the strategy matrix descriptors");
  const task = harnessJson([
    "task",
    "create",
    "--id",
    "task-earning-engine-adapter-pilot",
    "--goal",
    "goal-self-host-mvp",
    "--parent",
    "task-self-host-workflow",
    "--title",
    "Follow-up: run Earning Engine adapter pilot",
    "--objective",
    "Use the adapter descriptors to drive a strategy-matrix audit and produce evidence-backed decisions.",
    "--owner",
    "lead",
    "--reviewer",
    "critic",
    "--owned-path",
    "examples/adapters/earning-engine",
  ]);
  const evidence = harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-ee-adapter-surface",
    "--task",
    task.id,
    "--source-type",
    "goal_case",
    "--source-ref",
    "examples/adapters/earning-engine/pilot-workflow.md",
    "--summary",
    "Earning Engine adapter surface is present for the strategy-matrix pilot.",
  ]);
  return { adapter: adapter.adapterId ?? adapter.name, descriptors: descriptors.length, evidence_id: evidence.id };
}

function liveCodexRuntime() {
  const agent = harnessJson(
    [
      "agent",
      "create",
      "--id",
      "live-worker",
      "--name",
      "Live Worker",
      "--role",
      "worker",
      "--provider",
      "codex",
      "--start",
    ],
    { env: { HARNESS_AGENT_START_TIMEOUT_MS: "15000" } },
  );
  try {
    const health = harnessJson(["agent", "health", "--id", agent.id]);
    assert(String(health.health.protocol_probe).startsWith("pass:"), "protocol probe did not pass");
    const message = harnessJson([
      "agent",
      "send",
      "--from",
      "lead",
      "--to",
      agent.id,
      "--task",
      "task-self-host-workflow",
      "--kind",
      "task",
      "--content",
      "Reply with one short sentence: live Codex runtime smoke complete.",
    ]);
    const delivery = harnessJson([
      "agent",
      "deliver",
      "--agent",
      agent.id,
      "--message",
      message.id,
      "--timeout-ms",
      "90000",
    ]);
    const delivered = delivery.delivered?.[0];
    assert(delivered?.delivery_status === "delivered", "live Codex delivery did not deliver");
    assert(delivered.provider_thread_id, "live Codex delivery missing provider_thread_id");
    return {
      agent_id: agent.id,
      provider_thread_id: delivered.provider_thread_id,
      terminal_source: delivered.terminal_source,
    };
  } finally {
    harnessJson(["agent", "close", "--id", agent.id]);
  }
}

function createLiveAgent(id, name, role) {
  const agent = harnessJson(
    [
      "agent",
      "create",
      "--id",
      id,
      "--name",
      name,
      "--role",
      role,
      "--description",
      `${name} used by the live multi-member dogfood gate.`,
      "--provider",
      "codex",
      "--start",
    ],
    { env: { HARNESS_AGENT_START_TIMEOUT_MS: "15000" } },
  );
  const health = harnessJson(["agent", "health", "--id", agent.id]);
  assert(String(health.health.protocol_probe).startsWith("pass:"), `${id} protocol probe did not pass`);
  return agent;
}

function deliverLiveMessage(agentId, taskId, content, channel) {
  const message = harnessJson([
    "agent",
    "send",
    "--from",
    "lead",
    "--to",
    agentId,
    "--task",
    taskId,
    "--kind",
    "task",
    "--channel",
    channel,
    "--content",
    content,
  ]);
  const delivery = harnessJson([
    "agent",
    "deliver",
    "--agent",
    agentId,
    "--message",
    message.id,
    "--timeout-ms",
    "90000",
  ]);
  const delivered = delivery.delivered?.[0];
  assert(delivered?.delivery_status === "delivered", `${agentId} delivery did not deliver`);
  assert(delivered.provider_thread_id, `${agentId} delivery missing provider_thread_id`);
  return delivered;
}

function closeLiveAgent(id) {
  try {
    harnessJson(["agent", "close", "--id", id]);
  } catch {
    // Best-effort cleanup: the primary assertion failure is more useful than a
    // secondary close failure when debugging live provider delivery.
  }
}

function liveMultiMemberTeamDogfood() {
  const liveAgentIds = [];
  let worker;
  let critic;
  try {
    worker = createLiveAgent("live-team-worker", "Live Team Worker", "worker");
    liveAgentIds.push(worker.id);
    critic = createLiveAgent("live-team-critic", "Live Team Critic", "critic");
    liveAgentIds.push(critic.id);
    const team = harnessJson([
      "team",
      "create",
      "--id",
      "team-live-dogfood",
      "--name",
      "Live Dogfood Team",
      "--description",
      "Worker and Critic persistent Codex AgentMembers used to prove live multi-agent operation.",
      "--owner",
      "lead",
      "--member",
      "lead",
      "--member",
      worker.id,
      "--member",
      critic.id,
    ]);
    const task = harnessJson([
      "task",
      "create",
      "--id",
      "task-live-team-dogfood",
      "--goal",
      "goal-self-host-mvp",
      "--parent",
      "task-self-host-workflow",
      "--title",
      "Live multi-member dogfood",
      "--objective",
      "Use persistent Codex Worker and Critic AgentMembers to exercise the harness message and review loop.",
      "--owner",
      "lead",
      "--reviewer",
      critic.id,
      "--workspace",
      repoRoot,
      "--owned-path",
      "scripts/acceptance-mvp.mjs",
      "--acceptance",
      "Worker and Critic both deliver through Codex app-server and appear in Dashboard provider sessions.",
    ]);
    harnessJson([
      "task",
      "assign",
      "--id",
      task.id,
      "--assignee",
      worker.id,
      "--channel",
      "live-team-assignment",
    ]);
    const workerDelivery = deliverLiveMessage(
      worker.id,
      task.id,
      "You are the Worker AgentMember in a Multi-Agent Harness acceptance test. Reply with one concise report sentence confirming you received the task and would attach evidence refs.",
      "live-team-task",
    );
    const criticDelivery = deliverLiveMessage(
      critic.id,
      task.id,
      "You are the Critic AgentMember in a Multi-Agent Harness acceptance test. Reply with one concise critique sentence confirming Worker output must be checked with evidence before acceptance.",
      "live-team-review",
    );
    const snapshot = harnessJson(["dashboard", "snapshot"]);
    const liveTeam = snapshot.teams.find((item) => item.id === team.id);
    const liveTask = snapshot.tasks.find((item) => item.id === task.id);
    const sessions = snapshot.provider_sessions.filter((session) =>
      ["live-team-worker", "live-team-critic"].includes(session.agent_member_id),
    );
    const reportMessages = snapshot.messages.filter(
      (message) =>
        message.kind === "report" &&
        ["live-team-worker", "live-team-critic"].includes(message.from_agent_id),
    );
    assert(liveTeam?.member_ids?.length === 3, "dashboard missing live dogfood team");
    assert(liveTask?.assignee_agent_id === worker.id, "dashboard missing live dogfood task assignment");
    assert(liveTask?.reviewer_agent_id === critic.id, "dashboard missing live dogfood task reviewer");
    assert(sessions.length >= 2, "dashboard missing live team provider sessions");
    assert(reportMessages.length >= 2, "dashboard missing live team report messages");
    const status = harnessJson([
      "goal",
      "learning-status",
      "--id",
      "goal-self-host-mvp",
      "--strict",
      "--require-evaluation",
    ]);
    assert(status.ok === true, `goal learning failed after live team: ${status.warnings?.join("; ")}`);
    return {
      worker_thread_id: workerDelivery.provider_thread_id,
      critic_thread_id: criticDelivery.provider_thread_id,
      team_id: team.id,
      task_id: task.id,
      provider_sessions: sessions.length,
      report_messages: reportMessages.length,
    };
  } finally {
    for (const id of liveAgentIds.slice().reverse()) closeLiveAgent(id);
  }
}

stage("S0", "Static repository gates", () =>
  skipStatic ? { skipped_by_flag: "--skip-static" } : staticGates(),
);
stage("S1", "Object store, members, team, goal", createBaseObjects);
stage("S2", "Goal design before assignment", createWorkflowTask);
stage("S3", "Worker report, checks, critic evidence", recordWorkerEvidence);
stage("S4", "Provider notification fixture ingestion", providerFixtureAndIngest);
stage("S5", "Negative review gate rejects fake acceptance", negativeReviewGate);
stage("S6", "Proposal, review gate, decision", acceptWorkflowTask);
stage("S7", "Goal evaluation and reusable case", evaluateGoal);
stage("S8", "Hook bridge and plugin fallback", hookAndPluginBridge);
await stageAsync("S9", "Dashboard snapshot and API", dashboardApi);
stage("S10", "Earning Engine adapter surface", earningEngineAdapterSurface);
if (liveCodex) {
  stage("S11", "Live persistent Codex AgentMember", liveCodexRuntime);
  stage("S12", "Live multi-member team dogfood", liveMultiMemberTeamDogfood);
} else {
  skipped("S11", "Live persistent Codex AgentMember", "pass --live-codex to spend provider tokens");
  skipped("S12", "Live multi-member team dogfood", "pass --live-codex to run Worker and Critic persistent members");
}

finish(0);

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

function finish(exitCode) {
  const summary = {
    ok: exitCode === 0,
    store,
    live_codex: liveCodex,
    results,
  };
  console.log(JSON.stringify(summary, null, 2));
  if (exitCode === 0 && !keepStore && store.startsWith(tmpdir())) {
    rmSync(store, { recursive: true, force: true });
  }
  process.exit(exitCode);
}
