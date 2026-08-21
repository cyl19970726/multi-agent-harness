import { useEffect, useState, type ReactNode } from "react";
import {
  Activity,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Code,
  Terminal,
  Workflow,
  X,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import {
  DocProperties,
  DocSection,
  DocumentSurface,
  EmptyState,
  MonoId,
  StatusDot,
  type StatusTone,
} from "@/components/workbench/atoms";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import {
  WorkflowDefinitionPreview,
  workflowStepDomId,
} from "@/components/workbench/WorkflowPanels";
import { workflowRunTone, workflowStepTone } from "@/components/workbench/tones";

import { formatDuration, parseTs, type WorkbenchModel } from "../../model/readModel";
import {
  compactWorkflowScript,
  splitPartialOutputSteps,
  terminalReasonInfo,
  workflowScriptFromRun,
} from "../../model/workflowSelectors";
import {
  describeShape,
  inferWorkflowShape,
  phaseWindow,
  stepGanttGeometry,
  type WorkflowPhase,
} from "../../model/workflowShape";
import { fetchNativeWorkflowStepActivity, normalizeBaseUrl } from "../../api";
import type {
  NativeActivityProjection,
  NativeSessionRef,
  WorkflowRun,
  WorkflowStep,
} from "../../types";
import type { SelectionState } from "../../app/selection";

export function Definition({
  phases,
  workflowName,
  apiUrl,
}: {
  phases: WorkflowPhase[];
  workflowName: string;
  apiUrl?: string;
}) {
  return (
    <div className="space-y-3">
      {phases.length ? (
        <AsciiGraph phases={phases} />
      ) : (
        <p className="text-[11px] text-muted-foreground">
          The structural graph is derived from the run's steps.
        </p>
      )}
      <RustSource workflowName={workflowName} apiUrl={apiUrl} />
    </div>
  );
}

/**
 * The shared one-line ASCII structural restatement. Each node carries its
 * step's tone via a leading StatusDot so the graph and timeline agree. Cheap —
 * computed from the steps already in hand, no fetch.
 */
export function AsciiGraph({ phases }: { phases: WorkflowPhase[] }) {
  return (
    <div className="overflow-x-auto rounded-md border border-border bg-muted/30 p-2 font-mono text-[11px]">
      <div className="flex flex-wrap items-center gap-1.5 whitespace-nowrap">
        {phases.map((phase, phaseIndex) => (
          <span key={phase.phase} className="flex items-center gap-1.5">
            {phaseIndex > 0 && <span className="text-muted-foreground">──▶</span>}
            {phase.kind === "parallel" ? (
              <span className="flex items-center gap-1">
                <span className="text-muted-foreground">⟨</span>
                {phase.steps.map((step, i) => (
                  <span key={step.id} className="flex items-center gap-1">
                    {i > 0 && <span className="text-muted-foreground">∥</span>}
                    <NodeLabel step={step} />
                  </span>
                ))}
                <span className="text-muted-foreground">⟩</span>
              </span>
            ) : (
              phase.steps.map((step) => <NodeLabel key={step.id} step={step} />)
            )}
          </span>
        ))}
        {phases.some((p) => p.kind === "parallel") && (
          <span className="flex items-center gap-1.5">
            <span className="text-muted-foreground">──▶</span>
            <span className="text-muted-foreground">⟂ join</span>
          </span>
        )}
      </div>
    </div>
  );
}

function NodeLabel({ step }: { step: WorkflowStep }) {
  return (
    <span className="inline-flex items-center gap-1">
      <StatusDot tone={workflowStepTone(step.status)} className="size-1.5" />
      <span className="text-foreground">{step.label}</span>
    </span>
  );
}

/** Lazy Rust source, following the TurnDrillIn lazy-fetch contract exactly. */
function RustSource({ workflowName, apiUrl }: { workflowName: string; apiUrl?: string }) {
  const [open, setOpen] = useState(false);
  const [source, setSource] = useState<{ path: string; source: string } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function toggle() {
    const next = !open;
    setOpen(next);
    if (!next || source !== null || !apiUrl) return;
    setLoading(true);
    setError(null);
    try {
      const base = normalizeBaseUrl(apiUrl);
      const res = await fetch(`${base}/v1/workflows/${encodeURIComponent(workflowName)}/source`);
      if (res.status === 404) {
        setError("source unavailable (endpoint not present in this build)");
        return;
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setSource((await res.json()) as { path: string; source: string });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div>
      <button
        type="button"
        onClick={toggle}
        className="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
      >
        {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
        <Code className="size-3" />
        View Rust source · workflow.rs
      </button>
      {open && (
        <div className="mt-1.5 space-y-1">
          {loading && <span className="text-[11px] text-muted-foreground">loading…</span>}
          {error && <span className="text-[11px] text-status-bad">{error}</span>}
          {source && (
            <>
              <MonoId>{source.path}</MonoId>
              <pre className="max-h-96 overflow-auto whitespace-pre rounded-md border border-border bg-muted/30 p-2 font-mono text-[11px] text-foreground">
                {source.source}
              </pre>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/* ================================================================== */
/* Small shared bits                                                   */
/* ================================================================== */

/** A flip-chevron collapsible header row (the CollapsibleBlock idiom). */
export function CollapsibleRow({
  open,
  onToggle,
  label,
}: {
  open: boolean;
  onToggle: () => void;
  label: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
    >
      {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
      {label}
    </button>
  );
}

/**
 * Compact launch time for the runs list: clock + short date on one line, a
 * relative "Nm ago" under it, full datetime on hover — so "which runs are
 * current" reads at a glance without opening the detail. parseTs handles the
 * "unix-ms:<n>" format (raw Date.parse returns NaN on the prefix).
 */
export function CompactTimestamp({ value }: { value: string }) {
  const ms = parseTs(value);
  if (Number.isNaN(ms))
    return <span className="text-[12px] text-muted-foreground">—</span>;
  const d = new Date(ms);
  return (
    <span className="block min-w-0" title={d.toLocaleString()}>
      <span className="block truncate text-[12px] tabular-nums text-foreground/80">
        {d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        <span className="text-muted-foreground">
          {" "}
          {d.toLocaleDateString([], { month: "short", day: "numeric" })}
        </span>
      </span>
      <span className="block truncate text-[10px] text-muted-foreground">
        {relativeTime(ms)}
      </span>
    </span>
  );
}

export function Timestamp({ value }: { value: string }) {
  // created_at / ended_at are "unix-ms:<n>"; Date.parse can't read that prefix
  // (→ NaN → the raw "unix-ms:…" string leaked into the UI). parseTs handles it.
  const ms = parseTs(value);
  if (Number.isNaN(ms)) return <>{value}</>;
  return (
    <span className="tabular-nums">
      {new Date(ms).toLocaleString()}
      <span className="text-muted-foreground"> · {relativeTime(ms)}</span>
    </span>
  );
}

export function isTerminal(status: string): boolean {
  const s = status.toLowerCase();
  return s === "completed" || s === "failed" || s === "cached";
}

export function statusGloss(status: string): string {
  const s = status.toLowerCase();
  if (s === "completed" || s === "cached") return "ok";
  if (s === "failed") return "failed";
  return s;
}

/** A plain-English gloss of the gate logic for the verdict card (§3). */
export function verdictGloss(run: WorkflowRun, steps: WorkflowStep[]): string {
  const status = (run.status ?? "").toLowerCase();
  const total = steps.length;
  const failed = steps.filter((s) => (s.status ?? "").toLowerCase() === "failed").length;
  if (status === "failed") {
    return "Review gate needs evidence or rationale before this run can be accepted.";
  }
  if (status === "completed" && failed > 0) {
    return `Completed with concerns: ${failed} of ${total} steps failed and should be reviewed.`;
  }
  if (status === "completed") {
    return `Review passed: all ${total} run stages finished.`;
  }
  return run.summary ?? "";
}

export function stepTiming(step: WorkflowStep): string {
  const start = fmtClock(step.started_at);
  if (!step.ended_at) {
    return `· started ${start} · running…`;
  }
  const end = fmtClock(step.ended_at);
  const dur = formatDuration(step.started_at, step.ended_at);
  return `· ${start} → ${end}${dur ? ` · ${dur}` : ""}`;
}

function fmtClock(value?: string | null): string {
  if (!value) return "—";
  const ms = parseTs(value);
  if (Number.isNaN(ms)) return value;
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function relativeTime(ms: number): string {
  const deltaS = Math.round((Date.now() - ms) / 1000);
  if (deltaS < 60) return "just now";
  const m = Math.floor(deltaS / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

/** Parse a "codex"/"claude"/"kimi" role hint from a step label like "audit-codex". */
export function roleHintFromLabel(label: string): string | undefined {
  const lower = label.toLowerCase();
  if (lower.includes("codex")) return "codex";
  if (lower.includes("claude")) return "claude";
  if (lower.includes("kimi")) return "kimi";
  return undefined;
}

/** The schematic (declared) shape for a registered def, when known. */
export function schematicPhasesFor(name: string): WorkflowPhase[] | undefined {
  if (name === "investigate") {
    return [
      { phase: "scope", kind: "serial", steps: [schematicStep("scope", "scope-question")] },
      {
        phase: "audit",
        kind: "parallel",
        steps: [schematicStep("audit", "audit-codex"), schematicStep("audit", "audit-claude")],
      },
    ];
  }
  return undefined;
}

/** A status-less placeholder step for schematic (declared, no-run) rendering. */
function schematicStep(phase: string, label: string): WorkflowStep {
  return {
    id: `schematic-${phase}-${label}`,
    run_id: "schematic",
    phase,
    label,
    status: "queued",
    started_at: "",
  };
}

/** Order steps for the index glyph by phase appearance (no run object here). */
export function orderForGlyph(steps: WorkflowStep[]): WorkflowStep[] {
  return steps;
}
