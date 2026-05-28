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
      "Flow: design -> assignment message -> member reports -> peer message -> decision -> GoalEvaluation/final acceptance -> GoalClose -> vision comparison -> next goal/task graph.",
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
  const visionRef = writeArtifact(
    "vision.md",
    [
      "# Vision",
      "",
      "A standing AgentTeam should evaluate completed task graphs, close accepted goals, compare the result with the long-term product vision, propose the next goal, and create the next task graph without waiting for the user to name every next step.",
    ].join("\n"),
  );
  return { team, goal, task, visionRef };
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

function runSchedulerForNextRound(goalId, visionRef) {
  const result = harnessJson([
    "autonomy",
    "loop",
    "--iterations",
    "1",
    "--goal",
    goalId,
    "--observer",
    "observer",
    "--lead",
    "lead",
    "--auto-accept",
    "--assignee",
    "worker",
    "--reviewer",
    "critic",
    "--dry-run",
    "--vision-ref",
    visionRef,
    "--vision-summary",
    "Autonomous teams should move from completed/evaluated goals to next goal proposals and task graphs.",
    "--goal-success",
    "Generated task graph is assigned and visible in Dashboard state.",
    "--acceptance",
    "Worker receives the generated task through the standing message gateway.",
  ]);
  const scheduled = result.results?.[0]?.tick?.scheduled?.[0];
  assert(scheduled, "runner should schedule one next goal");
  const createdGoal = scheduled.decision?.created_goal;
  const createdTask = scheduled.decision?.created_task;
  const proposal = scheduled.proposal;
  assert(scheduled.goal_close?.goal?.status === "complete", "runner should mark source goal complete");
  assert(createdGoal?.id, "runner should create next goal when auto-accept is enabled");
  assert(createdTask?.id, "runner should create next task graph root when auto-accept is enabled");
  const snapshot = harnessJson(["dashboard", "snapshot"]);
  const sourceGoal = snapshot.goals.find((goal) => goal.id === goalId);
  assert(sourceGoal?.status === "complete", "source goal should be complete in dashboard snapshot");
  const projected = snapshot.autonomous_proposals.find((item) => item.id === proposal.id);
  assert(projected, "dashboard must project the runner-created Observer next-goal proposal");
  assert(projected.disposition === "accepted", `proposal disposition should be accepted, got ${projected.disposition}`);
  assert((projected.follow_up_task_ids ?? []).includes(createdTask.id), "proposal must link to generated task graph");
  assert((projected.follow_up_goal_ids ?? []).includes(createdGoal.id), "proposal must link to generated goal");
  return {
    closed_goal_id: goalId,
    plan_evidence: scheduled.plan.id,
    proposal: proposal.id,
    decision: scheduled.decision.decision.id,
    next_goal_id: createdGoal.id,
    next_task_id: createdTask.id,
    follow_up_task_ids: projected.follow_up_task_ids,
    follow_up_goal_ids: projected.follow_up_goal_ids,
  };
}

function executeAcceptedNextRound(taskId, goalId, visionRef) {
  const criticRef = writeArtifact(
    "next-round-critic-findings.md",
    "Critic verifies the accepted next-round task actually executed and can produce another next-round proposal.",
  );
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-next-round-critic",
    "--task",
    taskId,
    "--source-type",
    "critic_findings",
    "--source-ref",
    criticRef,
    "--summary",
    "Critic confirmed the accepted next-round task executed through the standing team.",
  ]);
  harnessJson([
    "decision",
    "record",
    "--id",
    "decision-next-round-current-goal",
    "--task",
    taskId,
    "--decision",
    "accepted by lead",
    "--rationale",
    "The accepted next-round task was delivered to the standing worker and can now produce another follow-up proposal.",
    "--evidence",
    "evidence-next-round-critic",
  ]);
  harnessJson(["task", "status", "--id", taskId, "--status", "done"]);
  const evaluationRef = writeArtifact(
    "next-round-goal-evaluation.md",
    [
      "# Next Round Goal Evaluation",
      "",
      "worked: the follow-up goal created by Observer/Lead was executed by the same standing team.",
      "next: create another autonomous proposal to prove the loop can continue beyond one generated round.",
    ].join("\n"),
  );
  harnessJson([
    "evidence",
    "add",
    "--id",
    "evidence-next-round-evaluation",
    "--task",
    taskId,
    "--source-type",
    "goal_evaluation",
    "--source-ref",
    evaluationRef,
    "--summary",
    "Next-round GoalEvaluation requests another automatic carry-forward proposal.",
  ]);
  const status = harnessJson(["goal", "learning-status", "--id", goalId, "--strict", "--require-evaluation"]);
  assert(status.ok === true, `next-round goal learning status should be clean: ${(status.warnings ?? []).join("; ")}`);
  const scheduled = runSchedulerForNextRound(goalId, visionRef);
  return {
    next_round_status_warnings: status.warnings ?? [],
    proposal: scheduled.proposal,
    decision: scheduled.decision,
    follow_up_task_ids: scheduled.follow_up_task_ids,
    follow_up_goal_ids: scheduled.follow_up_goal_ids,
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
  assert(workerSessions.length >= 5, "worker should have multiple provider sessions across generated rounds");
  assert(peerMessages.length === 2, "dashboard snapshot should expose peer message chain");
  assert(observerProposal, "dashboard snapshot should expose accepted Observer proposal");
  assert(snapshot.autonomous_proposals.filter((proposal) => proposal.disposition === "accepted").length >= 2, "dashboard should expose multiple accepted autonomous proposals");
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
let nextRound;
stage("s0", "standing team and goal design exist", () => {
  context = setupStandingTeam();
  return { team_id: context.team.id, goal_id: context.goal.id, task_id: context.task.id };
});
stage("s1", "same worker receives multiple messages and returns idle", () => assignAndReuseWorker(context.task.id));
stage("s2", "worker and critic collaborate through peer messages", () => peerMessage(context.task.id));
stage("s3", "goal evaluation is recorded and strict learning passes", () => evaluateGoal(context.task.id, context.goal.id));
stage("s4", "runner closes goal and creates next goal/task graph from vision", () => {
  nextRound = runSchedulerForNextRound(context.goal.id, context.visionRef);
  return nextRound;
});
stage("s5", "accepted next round executes and runner creates another goal/task graph", () =>
  executeAcceptedNextRound(nextRound.next_task_id, nextRound.next_goal_id, context.visionRef),
);
stage("s6", "dashboard snapshot proves autonomous-team loop", () => dashboardProof());

finish(0);
