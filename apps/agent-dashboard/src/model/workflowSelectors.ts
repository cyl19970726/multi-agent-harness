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

/**
 * A run counts as "direct workflow" (no recorded agent leaf steps, judged
 * only by its verdict/final output) exactly when the phase/run is actually
 * workflow-mode or scripted AND it has zero recorded steps. A completed run
 * with zero steps that ISN'T workflow-mode is not a direct workflow (it's
 * just an empty/degenerate run); a workflow-mode phase with no run yet is
 * not a "direct workflow run" either (there is nothing to narrate as direct
 * yet — the caller should render its own "not started" copy).
 */
export function isDirectWorkflowRun(
  run: WorkflowRun | undefined,
  steps: WorkflowStep[],
  isWorkflowModePhase: boolean,
): boolean {
  return Boolean(
    isWorkflowModePhase && run && steps.length === 0 && (run.final_output != null || run.status === "completed"),
  );
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

/** Minimal shape a plan-step needs for runtime-step matching (see `matchRuntimeSteps`). */
export interface LabeledPlanStep {
  label?: string;
}

/**
 * Match each plan row to at most one runtime step, in row order.
 *
 * Label matching (exact, then normalized-fuzzy) is tried first for every row
 * that has a parseable label; a runtime step consumed by an earlier row can
 * never be borrowed again by a later row. The positional fallback
 * (`runtimeSteps[index]`) only fires when the WHOLE plan has no parseable
 * labels at all — i.e. position is the only signal available. If even one
 * plan row has a label, unmatched rows render as "not started" instead of
 * guessing from position, because dynamic runs create step rows only as
 * agents start and a parallel group can start out of textual order.
 */
export function matchRuntimeSteps<T extends LabeledPlanStep>(
  planSteps: T[],
  runtimeSteps: WorkflowStep[],
): (WorkflowStep | undefined)[] {
  if (!runtimeSteps.length) return planSteps.map(() => undefined);
  const anyLabelParseable = planSteps.some((step) => Boolean(step.label?.trim()));
  const consumedIds = new Set<string>();
  const matches: (WorkflowStep | undefined)[] = planSteps.map((planStep) => {
    const label = planStep.label?.trim();
    if (label) {
      const exact = runtimeSteps.find((step) => step.label === label && !consumedIds.has(step.id));
      if (exact) {
        consumedIds.add(exact.id);
        return exact;
      }
      const normalized = normalizeWorkflowLabel(label);
      const fuzzy = runtimeSteps.find(
        (step) => normalizeWorkflowLabel(step.label) === normalized && !consumedIds.has(step.id),
      );
      if (fuzzy) {
        consumedIds.add(fuzzy.id);
        return fuzzy;
      }
    }
    return undefined;
  });

  if (anyLabelParseable) return matches;

  // Fully label-less plan: position is the only signal, so fall back to it
  // for every row (still skipping steps already consumed above, though none
  // would be at this point since no labels matched).
  return matches.map((match, index) => match ?? runtimeSteps[index]);
}

export function normalizeWorkflowLabel(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
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
