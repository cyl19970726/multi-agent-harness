import type {
  GoalOrchestrationRun,
  GoalPhase,
  Task,
  WorkflowRun,
  WorkflowStep,
  WorkflowTerminalReason,
} from "../types";
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

/* ================================================================== */
/* Failure diagnosis (issue #194): terminal_reason -> human class chip, */
/* partial-output split, schema-quality flags, dry-run gloss.           */
/* ================================================================== */

/** A short human label + longer gloss for a {@link WorkflowTerminalReason}. */
export interface TerminalReasonInfo {
  reason: WorkflowTerminalReason;
  /** Short chip label, e.g. "canceled by operator". */
  label: string;
  /** One-line explanation of what this class means operationally. */
  gloss: string;
  /** Whether this class implies the run/step was actively STOPPED (not a
   * clean pass/fail outcome the workflow author reasoned about). */
  abandoned: boolean;
  tone: "bad" | "warn" | "good" | "idle";
}

const TERMINAL_REASON_INFO: Record<WorkflowTerminalReason, Omit<TerminalReasonInfo, "reason">> = {
  canceled_by_operator: {
    label: "canceled by operator",
    gloss: "An operator interrupted the driver (SIGINT/SIGTERM); active leaves were killed.",
    abandoned: true,
    tone: "warn",
  },
  driver_exited: {
    label: "driver exited — abandoned",
    gloss: "The driver process exited or crashed before the run finalized; the run was abandoned mid-flight.",
    abandoned: true,
    tone: "bad",
  },
  orphan_reaped: {
    label: "orphan reaped",
    gloss: "A worker was left running after its owning run was already gone and was reaped as an orphan.",
    abandoned: true,
    tone: "warn",
  },
  leaf_timeout: {
    label: "leaf timeout",
    gloss: "A leaf hit its per-leaf wall-clock timeout and was killed.",
    abandoned: false,
    tone: "bad",
  },
  idle_timeout: {
    label: "idle timeout",
    gloss: "A leaf produced no output for the idle window and was killed.",
    abandoned: false,
    tone: "bad",
  },
  provider_failed: {
    label: "provider failed",
    gloss: "The provider worker itself failed (nonzero exit / spawn error / crash) — a driver/provider problem, not a verdict.",
    abandoned: false,
    tone: "bad",
  },
  verdict_failed: {
    label: "verdict failed",
    gloss: "Every step ran, but the workflow's own verdict() gate returned false — a clean logical rejection, not a crash.",
    abandoned: false,
    tone: "warn",
  },
  completed: {
    label: "completed",
    gloss: "Reached its terminal state normally.",
    abandoned: false,
    tone: "good",
  },
};

/** Look up the human class chip info for a raw `terminal_reason` wire value. */
export function terminalReasonInfo(
  reason?: string | null,
): TerminalReasonInfo | undefined {
  if (!reason) return undefined;
  const known = TERMINAL_REASON_INFO[reason as WorkflowTerminalReason];
  if (!known) return undefined;
  return { reason: reason as WorkflowTerminalReason, ...known };
}

/** The run's verdict, read from `final_output.verdict` / `final_output.success_criterion`
 * (the shape `verdict(ok, reason=...)` and `success_criterion=...` journal onto
 * `final_output`, see harness-cli `run_verdict_ok`). `undefined` fields mean
 * "not recorded yet" (e.g. the run never reached its verdict call). */
export interface WorkflowRunVerdictInfo {
  ok?: boolean;
  reason?: string;
  successCriterion?: string;
}

export function workflowRunVerdictInfo(run?: WorkflowRun): WorkflowRunVerdictInfo {
  const out = run?.final_output;
  if (!out || typeof out !== "object") return {};
  const record = out as Record<string, unknown>;
  const verdict = record.verdict;
  const verdictRecord = verdict && typeof verdict === "object" ? (verdict as Record<string, unknown>) : undefined;
  return {
    ok: typeof verdictRecord?.ok === "boolean" ? verdictRecord.ok : undefined,
    reason: typeof verdictRecord?.reason === "string" ? verdictRecord.reason : undefined,
    successCriterion:
      typeof record.success_criterion === "string" ? record.success_criterion : undefined,
  };
}

/** Split a run's steps into "usable partial artifacts" (completed ok, safe to
 * read) vs the rest (failed/reaped/canceled/still-running) — issue #194's core
 * ask for `partial_output_available` runs: "separates usable partial artifacts
 * from invalid gate output". Only meaningful when the run did not complete
 * cleanly; callers should gate rendering on `run.partial_output_available`. */
export interface PartialOutputSplit {
  usable: WorkflowStep[];
  invalid: WorkflowStep[];
}

export function splitPartialOutputSteps(steps: WorkflowStep[]): PartialOutputSplit {
  const usable: WorkflowStep[] = [];
  const invalid: WorkflowStep[] = [];
  for (const step of steps) {
    if (step.status === "completed" || step.status === "cached") {
      usable.push(step);
    } else {
      invalid.push(step);
    }
  }
  return { usable, invalid };
}

/** Schema-selection quality metadata read off a step's `result` (issue #192
 * metadata, surfaced here for issue #194's "schema-quality visibility"
 * ask). `undefined` for text-mode steps (no `schema_attempt_count` recorded). */
export interface SchemaSelectionInfo {
  attemptCount?: number;
  selectedIndex?: number | null;
  candidateCount?: number;
  emptyFieldCount: number;
  strict: boolean;
  /** True when the selected candidate has ≥1 empty top-level string field —
   * the "looked valid but empty" trap the issue calls out explicitly. */
  hasEmptyFields: boolean;
}

export function schemaSelectionInfo(step: WorkflowStep): SchemaSelectionInfo | undefined {
  const result = step.result;
  if (!result || result.schema_attempt_count == null) return undefined;
  const emptyFieldCount = result.empty_field_count ?? 0;
  return {
    attemptCount: result.schema_attempt_count,
    selectedIndex: result.selected_json_index,
    candidateCount: result.schema_candidate_count,
    emptyFieldCount,
    strict: Boolean(result.schema_strict),
    hasEmptyFields: emptyFieldCount > 0,
  };
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
