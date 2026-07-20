import type { WorkflowRun, WorkflowStep } from "../types";

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
