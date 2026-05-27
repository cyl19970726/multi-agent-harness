import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const keepStore = argv.includes("--keep-store");
const store = valueArg("--store") ?? mkdtempSync(join(tmpdir(), "mah-autonomous-team-"));
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
    results.push({ id, title, status: "pass", duration_ms: Date.now() - startedAt, evidence });
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
      ].filter(Boolean).join("\n"),
    );
  }
  return result.stdout;
}

function harnessJson(commandArgs) {
  const stdout = run(harness, commandArgs, { env: { HARNESS_ROOT: store } });
  return JSON.parse(stdout);
}

function harnessText(commandArgs) {
  return run(harness, commandArgs, { env: { HARNESS_ROOT: store } });
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

function createMember(id, name, role) {
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
    ".agents/skills/generic-agent-harness/SKILL.md",
  ]);
}

function gateway() {
  return harnessJson(["agent", "gateway", "--once", "--dry-run"]);
}

function setupStandingTeam() {
  harnessText(["init"]);
  createMember("lead", "Autonomous Lead", "lead");
  createMember("observer", "System Observer", "observer");
  createMember("worker", "Implementation Worker", "worker");
  createMember("critic", "Peer Critic", "critic");
  createMember("dashboard", "Dashboard Reader", "dashboard");
  const team = harnessJson([
    "team",
    "create",
    "--id",
    "team-autonomous",
    "--name",
    "Autonomous Harness Team",
    "--description",
    "Standing team used to prove observer proposal, message collaboration, and next-round planning.",
    "--owner",
    "lead",
    "--member",
    "lead",
    "--member",
    "observer",
    "--member",
    "worker",
    "--member",
    "critic",
    "--member",
    "dashboard",
  ]);
  const goal = harnessJson([
    "goal",
    "create",
    "--id",
    "goal-autonomous-team",
    "--title",
    "Autonomous AgentTeam control loop",
    "--objective",
    "Prove a standing team can execute a goal, evaluate it, propose the next goal, and continue through messages.",
    "--owner",
    "lead",
    "--success",
    "Dashboard snapshot proves member reuse, peer messages, observer proposal, Lead decision, and follow-up task graph.",
  ]);
  const task = harnessJson([
    "task",
    "create",
    "--id",
    "task-autonomous-loop",
    "--goal",
    goal.id,
    "--title",
    "Exercise autonomous team loop",
    "--objective",
    "Drive the first goal through assignment, reports, peer critique, decision, evaluation, and next-round planning.",
    "--owner",
    "lead",
    "--reviewer",
    "critic",
    "--workspace",
    repoRoot,
    "--branch",
    "acceptance-autonomous-team",
    "--owned-path",
    "scripts/acceptance-autonomous-team.mjs",
    "--acceptance",
    "The same worker receives multiple messages and Observer proposes the next goal.",
  ]);
  const designRef = writeArtifact(
    "goal-design.md",
    [
      "# Goal Design",
      "",
      "Scenario: standing AgentTeam keeps working after one task.",
      "Flow: design -> assignment message -> member reports -> peer message -> decision -> evaluation -> next-round proposal.",
      "Team: Lead, Observer, Worker, Critic, Dashboard Reader.",
    ].join("\n"),
  );
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-autonomous-design",
    "--task",
    task.id,
    "--source-type",
    "goal_design",
    "--source-ref",
    designRef,
    "--summary",
    "GoalDesign for autonomous team control loop.",
  ]);
  return { team, goal, task };
}

function assignAndReuseWorker(taskId) {
  harnessJson(["task", "assign", "--id", taskId, "--assignee", "worker"]);
  gateway();
  harnessJson([
    "message",
    "send",
    "--id",
    "msg-worker-second-task",
    "--from",
    "lead",
    "--to",
    "worker",
    "--task",
    taskId,
    "--channel",
    "task-follow-up",
    "--kind",
    "task",
    "--content",
    "Continue the same goal and produce a second report without closing the AgentMember.",
  ]);
  gateway();
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const worker = snapshot.members.find((member) => member.id === "worker");
  const workerInbox = snapshot.messages.filter((message) => message.to_agent_id === "worker");
  const workerSessions = snapshot.provider_sessions.filter((session) => session.agent_member_id === "worker");
  assert(worker?.status === "idle", `worker should be idle after two dry-run deliveries, got ${worker?.status}`);
  assert(workerInbox.filter((message) => message.delivery_status === "delivered").length >= 2, "worker needs at least two delivered inbox messages");
  assert(workerSessions.length >= 2, "worker needs at least two provider sessions");
  assert(new Set(workerSessions.map((session) => session.provider_thread_id)).size === 1, "worker should reuse provider thread in dry-run");
  return {
    worker_status: worker.status,
    worker_inbox_count: workerInbox.length,
    worker_sessions: workerSessions.map((session) => session.id),
    provider_thread_id: worker.provider_thread_id,
  };
}

function peerMessage(taskId) {
  harnessJson([
    "message",
    "send",
    "--id",
    "msg-worker-to-critic",
    "--from",
    "worker",
    "--to",
    "critic",
    "--task",
    taskId,
    "--channel",
    "peer-question",
    "--kind",
    "message",
    "--content",
    "Please critique whether the worker reports are enough evidence for this goal.",
  ]);
  gateway();
  harnessJson([
    "message",
    "send",
    "--id",
    "msg-critic-to-worker",
    "--from",
    "critic",
    "--to",
    "worker",
    "--task",
    taskId,
    "--channel",
    "peer-answer",
    "--kind",
    "message",
    "--content",
    "Critique returned: add explicit GoalEvaluation and next-round proposal evidence.",
  ]);
  gateway();
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const question = snapshot.messages.find((message) => message.id === "msg-worker-to-critic");
  const answer = snapshot.messages.find((message) => message.id === "msg-critic-to-worker");
  assert(question?.delivery_status === "delivered", "worker -> critic peer message must be delivered");
  assert(answer?.delivery_status === "delivered", "critic -> worker peer message must be delivered");
  assert(question.from_agent_id !== "lead" && answer.from_agent_id !== "lead", "peer messages must not be Lead-mediated");
  return {
    question: { from: question.from_agent_id, to: question.to_agent_id, status: question.delivery_status },
    answer: { from: answer.from_agent_id, to: answer.to_agent_id, status: answer.delivery_status },
  };
}

function evaluateGoal(taskId, goalId) {
  const criticRef = writeArtifact(
    "critic-findings.md",
    "Critic verifies worker reuse, peer messaging, and need for next-round autonomous planning.",
  );
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-autonomous-critic",
    "--task",
    taskId,
    "--source-type",
    "critic_findings",
    "--source-ref",
    criticRef,
    "--summary",
    "Critic confirmed the autonomous loop has enough runtime evidence.",
  ]);
  harnessJson([
    "decision",
    "record",
    "--id",
    "decision-autonomous-current-goal",
    "--task",
    taskId,
    "--decision",
    "accepted by lead",
    "--rationale",
    "Worker/Critic collaboration and provider sessions prove the first loop; continue into next-round planning.",
    "--evidence",
    "evidence-autonomous-critic",
  ]);
  harnessJson(["task", "status", "--id", taskId, "--status", "done"]);
  const evaluationRef = writeArtifact(
    "goal-evaluation.md",
    [
      "# Goal Evaluation",
      "",
      "worked: standing members reused messages and returned idle.",
      "worked: peer messages were delivered without Lead mediation.",
      "next: Observer should propose a follow-up goal and task graph automatically.",
    ].join("\n"),
  );
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-autonomous-evaluation",
    "--task",
    taskId,
    "--source-type",
    "goal_evaluation",
    "--source-ref",
    evaluationRef,
    "--summary",
    "GoalEvaluation requests automatic next-round planning.",
  ]);
  const caseRef = writeArtifact(
    "goal-case.md",
    "Reusable case: transport smoke is not autonomous-team acceptance; require durable members, Observer proposal, peer message, Lead decision, and next-round task graph.",
  );
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-autonomous-goal-case",
    "--task",
    taskId,
    "--source-type",
    "goal_case",
    "--source-ref",
    caseRef,
    "--summary",
    "GoalCase for autonomous team acceptance.",
  ]);
  const status = harnessJson(["goal", "learning-status", "--id", goalId, "--strict", "--require-evaluation"]);
  assert(status.ok === true, `goal learning status should be clean: ${(status.warnings ?? []).join("; ")}`);
  return { warnings: status.warnings ?? [], goal_case_count: status.goal_cases?.length ?? 0 };
}

function planAndAcceptNextRound(taskId, goalId) {
  const planned = harnessJson([
    "autonomy",
    "plan-next",
    "--goal",
    goalId,
    "--task",
    taskId,
    "--observer",
    "observer",
    "--lead",
    "lead",
    "--proposal-summary",
    "Create the next self-evolution goal from the completed GoalEvaluation and dashboard state.",
  ]);
  gateway();
  const accepted = harnessJson([
    "autonomy",
    "decide",
    "--task",
    taskId,
    "--lead",
    "lead",
    "--proposal",
    planned.proposal.id,
    "--decision",
    "accept",
    "--rationale",
    "Accept Observer next-round proposal and create a follow-up goal/task graph for self-evolution.",
    "--evidence",
    planned.plan.id,
    "--create-goal",
    "goal-autonomous-next-round",
    "--goal-title",
    "Next autonomous self-evolution round",
    "--goal-objective",
    "Use the previous GoalEvaluation to continue improving the harness without user-provided next steps.",
    "--goal-success",
    "Follow-up task graph exists and is assigned through a message.",
    "--create-task",
    "task-autonomous-next-round",
    "--task-title",
    "Follow-up: implement next autonomous improvement",
    "--task-objective",
    "Start the next round from Observer evidence and produce a report for Lead/Critic review.",
    "--assignee",
    "worker",
    "--reviewer",
    "critic",
    "--acceptance",
    "Worker receives the next-round task through the standing message gateway.",
  ]);
  gateway();
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const projected = snapshot.autonomous_proposals.find((proposal) => proposal.id === planned.proposal.id);
  assert(projected, "dashboard must project the Observer next-round proposal");
  assert(projected.disposition === "accepted", `proposal disposition should be accepted, got ${projected.disposition}`);
  assert((projected.follow_up_task_ids ?? []).includes("task-autonomous-next-round"), "proposal must link to follow-up task");
  assert((projected.follow_up_goal_ids ?? []).includes("goal-autonomous-next-round"), "proposal must link to follow-up goal");
  assert(snapshot.goals.some((goal) => goal.id === "goal-autonomous-next-round"), "follow-up goal must exist");
  assert(snapshot.tasks.some((task) => task.id === "task-autonomous-next-round"), "follow-up task must exist");
  return {
    plan_evidence: planned.plan.id,
    proposal: planned.proposal.id,
    decision: accepted.decision.id,
    follow_up_task_ids: projected.follow_up_task_ids,
    follow_up_goal_ids: projected.follow_up_goal_ids,
  };
}

function dashboardProof() {
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const team = snapshot.teams.find((item) => item.id === "team-autonomous");
  const worker = snapshot.members.find((item) => item.id === "worker");
  const observerProposal = snapshot.autonomous_proposals.find((item) => item.disposition === "accepted");
  const peerMessages = snapshot.messages.filter((message) => ["peer-question", "peer-answer"].includes(message.channel));
  const workerSessions = snapshot.provider_sessions.filter((session) => session.agent_member_id === "worker");
  assert(team?.member_ids?.length === 5, "standing team should include five members");
  assert(worker?.status === "idle", "worker should be idle after the next-round assignment delivery");
  assert(workerSessions.length >= 3, "worker should have multiple provider sessions across current and next tasks");
  assert(peerMessages.length === 2, "dashboard snapshot should expose peer message chain");
  assert(observerProposal, "dashboard snapshot should expose accepted Observer proposal");
  return {
    team_members: team.member_ids,
    worker_status: worker.status,
    worker_sessions: workerSessions.length,
    peer_messages: peerMessages.map((message) => `${message.from_agent_id}->${message.to_agent_id}:${message.channel}`),
    autonomous_proposals: snapshot.autonomous_proposals.length,
  };
}

function finish(exitCode) {
  const report = {
    status: exitCode === 0 ? "pass" : "fail",
    store,
    results,
  };
  console.log(JSON.stringify(report, null, 2));
  if (!keepStore && exitCode === 0) {
    rmSync(store, { recursive: true, force: true });
  }
  process.exit(exitCode);
}

if (!existsSync(harness)) {
  run("cargo", ["build", "-p", "harness-cli"]);
}

let context;
stage("s0", "standing team and goal design exist", () => {
  context = setupStandingTeam();
  return { team_id: context.team.id, goal_id: context.goal.id, task_id: context.task.id };
});
stage("s1", "same worker receives multiple messages and returns idle", () => assignAndReuseWorker(context.task.id));
stage("s2", "worker and critic collaborate through peer messages", () => peerMessage(context.task.id));
stage("s3", "goal evaluation is recorded and strict learning passes", () => evaluateGoal(context.task.id, context.goal.id));
stage("s4", "Observer proposes and Lead accepts next-round goal/task graph", () => planAndAcceptNextRound(context.task.id, context.goal.id));
stage("s5", "dashboard snapshot proves autonomous-team loop", () => dashboardProof());

finish(0);
