import type { SelectionState } from "../app/selection";
import type {
  AgentMember,
  DashboardSnapshot,
  Evidence,
  WorkflowDef,
  WorkflowRun,
  WorkflowStep,
} from "../types";

/**
 * Dashboard read model after the superseded coordination-stack retirement.
 * Native Mission/Wave and Agent Team selectors read directly from `snapshot`;
 * this projection only keeps shared lookup state needed by execution surfaces.
 */
export interface WorkbenchModel {
  snapshot: DashboardSnapshot;
  generatedAt?: string;
  selectedMember?: AgentMember;
  evidence: Evidence[];
  workflowDefs: WorkflowDef[];
  workflowRuns: WorkflowRun[];
  workflowStepsByRun: Map<string, WorkflowStep[]>;
  selectedWorkflowRun?: WorkflowRun;
  selectedWorkflowSteps: WorkflowStep[];
}

export function buildWorkbenchModel(
  snapshot: DashboardSnapshot,
  selection: SelectionState,
  workflowDefs: WorkflowDef[] = [],
): WorkbenchModel {
  const members = snapshot.members ?? [];
  const selectedMember = selection.memberId
    ? members.find((member) => member.id === selection.memberId)
    : undefined;
  const workflowRuns = [...(snapshot.workflow_runs ?? [])].sort(compareWorkflowRuns);
  const workflowStepsByRun = groupBy(snapshot.workflow_steps ?? [], (step) => step.run_id);
  const selectedWorkflowRun = selection.workflowRunId
    ? workflowRuns.find((run) => run.id === selection.workflowRunId)
    : undefined;

  return {
    snapshot,
    generatedAt: snapshot.generated_at,
    selectedMember,
    evidence: snapshot.evidence ?? [],
    workflowDefs,
    workflowRuns,
    workflowStepsByRun,
    selectedWorkflowRun,
    selectedWorkflowSteps: selectedWorkflowRun
      ? orderStepsByRun(selectedWorkflowRun, workflowStepsByRun.get(selectedWorkflowRun.id) ?? [])
      : [],
  };
}

function compareWorkflowRuns(a: WorkflowRun, b: WorkflowRun): number {
  const aLive = a.status === "running" ? 1 : 0;
  const bLive = b.status === "running" ? 1 : 0;
  if (aLive !== bLive) return bLive - aLive;
  return parseTs(b.created_at) - parseTs(a.created_at);
}

function groupBy<T>(values: T[], key: (value: T) => string): Map<string, T[]> {
  const grouped = new Map<string, T[]>();
  for (const value of values) {
    const id = key(value);
    const rows = grouped.get(id) ?? [];
    rows.push(value);
    grouped.set(id, rows);
  }
  return grouped;
}

/** Preserve authoritative `step_ids` order; append newly streamed rows. */
export function orderStepsByRun(run: WorkflowRun, steps: WorkflowStep[]): WorkflowStep[] {
  const remaining = new Map(steps.map((step) => [step.id, step]));
  const ordered: WorkflowStep[] = [];
  for (const id of run.step_ids ?? []) {
    const step = remaining.get(id);
    if (!step) continue;
    ordered.push(step);
    remaining.delete(id);
  }
  for (const step of steps) {
    if (remaining.delete(step.id)) ordered.push(step);
  }
  return ordered;
}

export function parseTs(value?: string | null): number {
  if (!value) return 0;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function formatDuration(start?: string | null, end?: string | null): string | undefined {
  const startMs = parseTs(start);
  if (!startMs) return undefined;
  const endMs = end ? parseTs(end) : Date.now();
  const seconds = Math.max(0, Math.round((endMs - startMs) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (minutes < 60) return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const minuteRemainder = minutes % 60;
  return minuteRemainder ? `${hours}h ${minuteRemainder}m` : `${hours}h`;
}
