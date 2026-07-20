import type { WorkflowRun, WorkflowStep, WorkflowTerminalReason } from "../types";

export interface WorkflowStepStatusCounts {
  queued: number;
  running: number;
  completed: number;
  failed: number;
  cached: number;
}

export function countWorkflowStepStatuses(steps: WorkflowStep[]): WorkflowStepStatusCounts {
  return steps.reduce<WorkflowStepStatusCounts>((counts, step) => {
    if (step.status in counts) counts[step.status as keyof WorkflowStepStatusCounts] += 1;
    return counts;
  }, { queued: 0, running: 0, completed: 0, failed: 0, cached: 0 });
}

export function workflowRunIsLive(run?: WorkflowRun, steps: WorkflowStep[] = []): boolean {
  return run?.status === "running" || steps.some((step) => step.status === "queued" || step.status === "running");
}

export interface TerminalReasonInfo {
  reason: WorkflowTerminalReason;
  label: string;
  gloss: string;
  abandoned: boolean;
  tone: "bad" | "warn" | "good" | "idle";
}

const TERMINAL_REASON_INFO: Record<WorkflowTerminalReason, Omit<TerminalReasonInfo, "reason">> = {
  canceled_by_operator: { label: "canceled by operator", gloss: "An operator interrupted the workflow driver.", abandoned: true, tone: "warn" },
  driver_exited: { label: "driver exited — abandoned", gloss: "The driver exited before the run finalized.", abandoned: true, tone: "bad" },
  orphan_reaped: { label: "orphan reaped", gloss: "A worker outlived its owning run and was reaped.", abandoned: true, tone: "warn" },
  leaf_timeout: { label: "leaf timeout", gloss: "A leaf exceeded its wall-clock timeout.", abandoned: false, tone: "bad" },
  idle_timeout: { label: "idle timeout", gloss: "A leaf produced no output within its idle window.", abandoned: false, tone: "bad" },
  provider_failed: { label: "provider failed", gloss: "The provider worker failed before producing an accepted result.", abandoned: false, tone: "bad" },
  verdict_failed: { label: "verdict failed", gloss: "Execution finished, but the workflow verdict rejected the result.", abandoned: false, tone: "warn" },
  completed: { label: "completed", gloss: "The workflow reached a normal terminal state.", abandoned: false, tone: "good" },
};

export function terminalReasonInfo(reason?: string | null): TerminalReasonInfo | undefined {
  if (!reason) return undefined;
  const known = TERMINAL_REASON_INFO[reason as WorkflowTerminalReason];
  return known ? { reason: reason as WorkflowTerminalReason, ...known } : undefined;
}

export interface WorkflowRunVerdictInfo {
  ok?: boolean;
  reason?: string;
  successCriterion?: string;
}

export function workflowRunVerdictInfo(run?: WorkflowRun): WorkflowRunVerdictInfo {
  if (!run?.final_output || typeof run.final_output !== "object") return {};
  const output = run.final_output as Record<string, unknown>;
  const verdict = output.verdict && typeof output.verdict === "object"
    ? output.verdict as Record<string, unknown>
    : undefined;
  return {
    ok: typeof verdict?.ok === "boolean" ? verdict.ok : undefined,
    reason: typeof verdict?.reason === "string" ? verdict.reason : undefined,
    successCriterion: typeof output.success_criterion === "string" ? output.success_criterion : undefined,
  };
}

export function splitPartialOutputSteps(steps: WorkflowStep[]): { usable: WorkflowStep[]; invalid: WorkflowStep[] } {
  const usable: WorkflowStep[] = [];
  const invalid: WorkflowStep[] = [];
  for (const step of steps) {
    (step.status === "completed" || step.status === "cached" ? usable : invalid).push(step);
  }
  return { usable, invalid };
}

export interface SchemaSelectionInfo {
  attemptCount?: number;
  selectedIndex?: number | null;
  candidateCount?: number;
  emptyFieldCount: number;
  strict: boolean;
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

export function workflowRunProgress(steps: WorkflowStep[]): { terminalSteps: number; totalSteps: number; percent: number } {
  const counts = countWorkflowStepStatuses(steps);
  const terminalSteps = counts.completed + counts.cached + counts.failed;
  return { terminalSteps, totalSteps: steps.length, percent: steps.length ? Math.round((terminalSteps / steps.length) * 100) : 0 };
}

export function isDirectWorkflowRun(run: WorkflowRun | undefined, steps: WorkflowStep[]): boolean {
  return Boolean(run && steps.length === 0 && (run.final_output != null || run.status === "completed"));
}

export function workflowScriptFromRun(run?: WorkflowRun): string | undefined {
  const spec = run?.spec;
  if (!spec || typeof spec !== "object") return undefined;
  const script = (spec as { script?: unknown }).script;
  return typeof script === "string" && script.trim() ? script : undefined;
}

export interface LabeledPlanStep { label?: string }

export function matchRuntimeSteps<T extends LabeledPlanStep>(planSteps: T[], runtimeSteps: WorkflowStep[]): (WorkflowStep | undefined)[] {
  if (!runtimeSteps.length) return planSteps.map(() => undefined);
  const hasLabels = planSteps.some((step) => Boolean(step.label?.trim()));
  const consumed = new Set<string>();
  const matched = planSteps.map((plan) => {
    const label = plan.label?.trim();
    if (!label) return undefined;
    const normalized = normalizeWorkflowLabel(label);
    const runtime = runtimeSteps.find((step) => !consumed.has(step.id) && (step.label === label || normalizeWorkflowLabel(step.label) === normalized));
    if (runtime) consumed.add(runtime.id);
    return runtime;
  });
  return hasLabels ? matched : matched.map((value, index) => value ?? runtimeSteps[index]);
}

export function normalizeWorkflowLabel(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

export function compactWorkflowScript(script: string, limit = 1800): string {
  const trimmed = script.trim();
  return trimmed.length <= limit ? trimmed : `${trimmed.slice(0, limit).trimEnd()}\n# ... truncated in preview`;
}
