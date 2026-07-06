import type { GoalOrchestrationRun, GoalPhase, Task, WorkflowRun, WorkflowStep } from "../types";
import type { PhaseDagLayer } from "./readModel";
import { parseTs } from "./readModel";

export interface WorkflowStepStatusCounts {
  queued: number;
  running: number;
  completed: number;
  failed: number;
  cached: number;
}

export function selectPhaseWorkflowRuns({
  goalId,
  phaseId,
  workflowRuns,
  goalOrchestrationRuns,
}: {
  goalId: string;
  phaseId: string;
  workflowRuns: WorkflowRun[];
  goalOrchestrationRuns: GoalOrchestrationRun[];
}): WorkflowRun[] {
  const linkedRunIds = new Set<string>();
  for (const run of goalOrchestrationRuns) {
    if (run.goal_id !== goalId) continue;
    for (const phaseRun of run.phase_runs ?? []) {
      if (phaseRun.phase_id === phaseId && phaseRun.workflow_run_id) {
        linkedRunIds.add(phaseRun.workflow_run_id);
      }
    }
  }
  return workflowRuns
    .filter(
      (run) =>
        (run.goal_id === goalId && run.phase_id === phaseId) || linkedRunIds.has(run.id),
    )
    .sort((a, b) => parseTs(b.created_at) - parseTs(a.created_at));
}

export function countWorkflowStepStatuses(steps: WorkflowStep[]): WorkflowStepStatusCounts {
  return steps.reduce<WorkflowStepStatusCounts>(
    (counts, step) => {
      switch (step.status) {
        case "queued":
        case "running":
        case "completed":
        case "failed":
        case "cached":
          counts[step.status] += 1;
          break;
        default:
          break;
      }
      return counts;
    },
    {
      queued: 0,
      running: 0,
      completed: 0,
      failed: 0,
      cached: 0,
    },
  );
}

export function workflowRunIsLive(run?: WorkflowRun, steps: WorkflowStep[] = []): boolean {
  return run?.status === "running" || steps.some((step) => ["queued", "running"].includes(step.status));
}

export function workflowRunProgress(steps: WorkflowStep[]): { terminalSteps: number; totalSteps: number; percent: number } {
  const counts = countWorkflowStepStatuses(steps);
  const terminalSteps = counts.completed + counts.cached + counts.failed;
  const totalSteps = steps.length;
  return {
    terminalSteps,
    totalSteps,
    percent: totalSteps ? Math.min(100, Math.round((terminalSteps / totalSteps) * 100)) : 0,
  };
}

export function workflowVerdictStep(phaseId: string | undefined, steps: WorkflowStep[]): WorkflowStep | undefined {
  if (!phaseId) return undefined;
  return steps.find((step) => step.label === `verdict-${phaseId}`);
}

export function plannedStepCount(layers: PhaseDagLayer[]): number {
  return layers.reduce(
    (sum, layer) => sum + layer.groups.reduce((inner, group) => inner + group.tasks.length, 0),
    0,
  );
}

export function workflowScriptFromRun(run?: WorkflowRun): string | undefined {
  const spec = run?.spec;
  if (!spec || typeof spec !== "object") return undefined;
  const script = (spec as { script?: unknown }).script;
  return typeof script === "string" && script.trim() ? script : undefined;
}

export function compactWorkflowScript(script: string, limit = 1800): string {
  const trimmed = script.trim();
  if (trimmed.length <= limit) return trimmed;
  return `${trimmed.slice(0, limit).trimEnd()}\n# ... truncated in preview`;
}

export function buildPhaseWorkflowPreview(phase: GoalPhase, layers: PhaseDagLayer[]): string {
  const lines: string[] = [
    `workflow("phase-${phase.id}", ${starArg(phase.intent || phase.name || phase.id)})`,
  ];
  for (const layer of layers) {
    for (const group of layer.groups) {
      if (group.parallel) {
        lines.push("parallel([");
        for (const task of group.tasks) {
          lines.push(`  ${workflowAgentLine(task, phase.id)},`);
        }
        lines.push("])");
      } else {
        for (const task of group.tasks) {
          lines.push(workflowAgentLine(task, phase.id));
        }
      }
    }
  }
  if (phase.acceptance?.trim()) {
    lines.push(
      `agent(${starArg(`Acceptance check for ${phase.id}`)}, provider="codex", label="verdict-${phase.id}", phase="${phase.id}", schema={"pass": "bool", "reason": "string"})`,
    );
    lines.push(`verdict("verdict-${phase.id}")`);
  }
  return lines.join("\n");
}

function workflowAgentLine(task: Task, phaseId: string): string {
  const args = [
    starArg(`${task.id}: ${task.title ?? task.objective ?? task.id}`),
    `provider=${starArg("codex")}`,
    `label=${starArg(task.id)}`,
    `phase=${starArg(phaseId)}`,
  ];
  if ((task.owned_paths ?? []).length > 0) {
    args.push("writable=True", `isolation=${starArg("worktree")}`);
  }
  return `agent(${args.join(", ")})`;
}

function starArg(value: string): string {
  return JSON.stringify(value);
}
