import type { WorkflowStep } from "../types";

/**
 * The inferred control-flow shape of a workflow run (§4). Control flow is NOT a
 * persisted field — it is reconstructed from the steps already in hand:
 *
 *   1. Group steps by `phase`, preserving the order they appear in `steps`
 *      (which the caller has already ordered by the run's `step_ids`).
 *   2. Classify each phase: a phase with a single step, OR steps whose
 *      [started_at, ended_at] windows do NOT overlap, is SERIAL (one node). A
 *      phase whose steps have OVERLAPPING windows is a PARALLEL barrier.
 *
 * Phase grouping + step count is the primary key; window overlap is the
 * secondary signal. The same shape drives the index "Preview shape", the detail
 * Timeline, and the Definition ASCII — one function, three call sites — so the
 * structure can never disagree with itself.
 */
export type PhaseKind = "serial" | "parallel";

export interface WorkflowPhase {
  /** The `phase` marker shared by the steps in this group. */
  phase: string;
  kind: PhaseKind;
  /** The steps in this phase, in `step_ids` order. */
  steps: WorkflowStep[];
}

/** Group a run's (already step_ids-ordered) steps into classified phases. */
export function inferWorkflowShape(steps: WorkflowStep[]): WorkflowPhase[] {
  const order: string[] = [];
  const byPhase = new Map<string, WorkflowStep[]>();
  for (const step of steps) {
    const existing = byPhase.get(step.phase);
    if (existing) {
      existing.push(step);
    } else {
      order.push(step.phase);
      byPhase.set(step.phase, [step]);
    }
  }
  return order.map((phase) => {
    const phaseSteps = byPhase.get(phase) ?? [];
    return { phase, kind: classifyPhase(phaseSteps), steps: phaseSteps };
  });
}

/**
 * A phase is PARALLEL when it has more than one step AND at least two of those
 * steps' time windows overlap; otherwise SERIAL. For the canonical `investigate`
 * shape this yields scope (serial) → audit (parallel, 2).
 */
function classifyPhase(steps: WorkflowStep[]): PhaseKind {
  if (steps.length <= 1) return "serial";
  return anyWindowsOverlap(steps) ? "parallel" : "serial";
}

/** True when any two of the steps' [started_at, ended_at] windows overlap. */
function anyWindowsOverlap(steps: WorkflowStep[]): boolean {
  const windows = steps.map(stepWindow);
  for (let i = 0; i < windows.length; i += 1) {
    for (let j = i + 1; j < windows.length; j += 1) {
      const a = windows[i];
      const b = windows[j];
      if (a.start < b.end && b.start < a.end) return true;
    }
  }
  return false;
}

interface Window {
  start: number;
  end: number;
}

/** A step's time window; a still-running step is treated as open until now. */
function stepWindow(step: WorkflowStep): Window {
  const start = Date.parse(step.started_at);
  const end = step.ended_at ? Date.parse(step.ended_at) : Date.now();
  return {
    start: Number.isNaN(start) ? 0 : start,
    end: Number.isNaN(end) ? Number.MAX_SAFE_INTEGER : end,
  };
}

/** The window spanning a phase's steps (min start .. max end) for the gantt. */
export function phaseWindow(steps: WorkflowStep[]): Window {
  const windows = steps.map(stepWindow);
  return {
    start: Math.min(...windows.map((w) => w.start)),
    end: Math.max(...windows.map((w) => w.end)),
  };
}

/** A step's left%/width% within its phase window, for the inline gantt strip. */
export function stepGanttGeometry(
  step: WorkflowStep,
  window: Window,
): { left: number; width: number } {
  const span = Math.max(1, window.end - window.start);
  const w = stepWindow(step);
  const left = ((w.start - window.start) / span) * 100;
  const width = (Math.max(0, w.end - w.start) / span) * 100;
  return {
    left: Math.max(0, Math.min(100, left)),
    width: Math.max(2, Math.min(100 - left, width)),
  };
}

/** "3 · 1 serial, 2 parallel" summary for the DocProperties Steps row. */
export function describeShape(phases: WorkflowPhase[]): string {
  const total = phases.reduce((n, phase) => n + phase.steps.length, 0);
  const serial = phases
    .filter((phase) => phase.kind === "serial")
    .reduce((n, phase) => n + phase.steps.length, 0);
  const parallel = total - serial;
  const parts: string[] = [];
  if (serial) parts.push(`${serial} serial`);
  if (parallel) parts.push(`${parallel} parallel`);
  const detail = parts.length ? ` · ${parts.join(", ")}` : "";
  return `${total}${detail}`;
}
