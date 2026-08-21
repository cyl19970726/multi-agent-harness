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

import { formatDuration, parseTs, type WorkbenchModel } from "../model/readModel";
import {
  compactWorkflowScript,
  splitPartialOutputSteps,
  terminalReasonInfo,
  workflowScriptFromRun,
} from "../model/workflowSelectors";
import {
  describeShape,
  inferWorkflowShape,
  phaseWindow,
  stepGanttGeometry,
  type WorkflowPhase,
} from "../model/workflowShape";
import { fetchNativeWorkflowStepActivity, normalizeBaseUrl } from "../api";
import type {
  NativeActivityProjection,
  NativeSessionRef,
  WorkflowRun,
  WorkflowStep,
} from "../types";
import type { SelectionState } from "../app/selection";


import {
  AsciiGraph,
  CollapsibleRow,
  CompactTimestamp,
  orderForGlyph,
  schematicPhasesFor,
} from "./workflows/WorkflowDefinition";
export { WorkflowRunDetail } from "./workflows/WorkflowRunDetail";

export interface WorkflowSurfaceProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  apiUrl?: string;
  projectBindingId?: string;
  executionSpaceId?: string;
}

/* ================================================================== */
/* INDEX — registered catalog + every run                              */
/* ================================================================== */

export function WorkflowsList({ model, onSelectionChange }: WorkflowSurfaceProps) {
  const defs = model.workflowDefs;
  const runs = model.workflowRuns;
  return (
    <DocumentSurface className="max-w-[940px]">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Workflow className="size-3.5" /> Workflows
          </div>
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">
            Workflows
          </h1>
          <p className="text-sm text-muted-foreground">
            Registered pipelines and every run. Open a run to see its timeline.
          </p>
        </div>
      </header>

      <DocSection label="Registered">
        {defs.length ? (
          <div className="space-y-2.5">
            {defs.map((def) => (
              <RegisteredCard key={def.name} name={def.name} summary={def.summary} />
            ))}
          </div>
        ) : (
          <EmptyState
            icon={Workflow}
            title="Workflow catalog unavailable"
            description="Connect a running harness with Load live to see the registered pipelines."
          />
        )}
      </DocSection>

      <DocSection label={`${runs.length} ${runs.length === 1 ? "run" : "runs"}`}>
        {runs.length ? (
          <RunsTable
            runs={runs}
            stepsByRun={model.workflowStepsByRun}
            onOpen={(id) => onSelectionChange({ surface: "workflows", workflowRunId: id })}
          />
        ) : (
          <EmptyState
            icon={Workflow}
            title="No runs yet"
            description="Run a registered workflow from the harness to see its serial→parallel timeline here."
          />
        )}
      </DocSection>
    </DocumentSurface>
  );
}

/** One registered-def card with a collapsible schematic shape preview. */
function RegisteredCard({ name, summary }: { name: string; summary: string }) {
  return (
    <div className="rounded-lg border border-border bg-card p-3">
      <div className="flex min-w-0 items-start gap-2.5">
        <Workflow className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1 space-y-1">
          <p className="truncate text-[13px] font-medium text-foreground">{name}</p>
          <p className="line-clamp-1 text-xs text-muted-foreground">{summary}</p>
          <MonoId>{name}</MonoId>
          <SchematicPreview name={name} />
        </div>
      </div>
    </div>
  );
}

/**
 * "Preview shape" collapsible. With no run there are no steps to infer from, so
 * this renders the canonical declared shape of the built-in `investigate`
 * workflow as the schematic restatement (the same ASCII renderer, schematic
 * mode). Other defs fall back to a generic note until a run exists.
 */
function SchematicPreview({ name }: { name: string }) {
  const [open, setOpen] = useState(false);
  const schematic = schematicPhasesFor(name);
  return (
    <div className="pt-0.5">
      <CollapsibleRow
        open={open}
        onToggle={() => setOpen((v) => !v)}
        label="Preview shape"
      />
      {open && (
        <div className="mt-1.5">
          {schematic ? (
            <AsciiGraph phases={schematic} />
          ) : (
            <p className="text-[11px] text-muted-foreground">
              Shape is derived from a run; open a run to see its timeline.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** The grid-of-buttons runs list (mirrors the AgentsList idiom). */
function RunsTable({
  runs,
  stepsByRun,
  onOpen,
}: {
  runs: WorkflowRun[];
  stepsByRun: Map<string, WorkflowStep[]>;
  onOpen: (id: string) => void;
}) {
  const cols =
    "grid-cols-[minmax(0,1.9fr)_minmax(0,1.1fr)_minmax(0,1fr)_minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1.5fr)]";
  return (
    <div className="overflow-hidden">
      <div
        className={cn(
          "grid gap-3 border-b border-border px-2 pb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground",
          cols,
        )}
      >
        <span>Run</span>
        <span>Started</span>
        <span>Status</span>
        <span>Steps</span>
        <span className="hidden lg:block">Duration</span>
        <span className="hidden lg:block">Summary</span>
      </div>
      <div>
        {runs.map((run) => {
          const tone = workflowRunTone(run.status);
          const running = tone === "running";
          const steps = stepsByRun.get(run.id) ?? [];
          const duration = formatDuration(run.created_at, run.ended_at);
          const terminal = terminalReasonInfo(run.terminal_reason);
          return (
            <button
              key={run.id}
              type="button"
              onClick={() => onOpen(run.id)}
              className={cn(
                "grid w-full items-center gap-3 border-b border-border/60 px-2 py-2.5 text-left transition-colors last:border-b-0 hover:bg-accent/40",
                cols,
              )}
            >
              <span className="flex min-w-0 items-center gap-2.5">
                <StatusDot tone={tone} pulse={running} />
                <span className="min-w-0">
                  <span className="block truncate text-[13px] font-medium text-foreground">
                    {run.workflow_name}
                  </span>
                  <span className="block truncate">
                    <MonoId>{run.id}</MonoId>
                  </span>
                </span>
              </span>
              <span className="min-w-0">
                <CompactTimestamp value={run.created_at} />
              </span>
              <span className="min-w-0">
                <Badge tone={tone}>{run.status}</Badge>
                {run.dry_run && <Badge tone="warn">dry-run</Badge>}
                {terminal && terminal.reason !== "completed" && <Badge tone={terminal.tone}>{terminal.label}</Badge>}
              </span>
              <span className="min-w-0">
                <ShapeGlyph steps={steps} />
              </span>
              <span className="hidden min-w-0 truncate text-[12px] tabular-nums text-muted-foreground lg:block">
                {running ? "· running" : (duration ?? "—")}
              </span>
              <span className="hidden min-w-0 truncate text-[12px] text-muted-foreground lg:block">
                {running ? "—" : (run.summary ?? "—")}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/**
 * The grafted shape glyph: one small toned pill per step, laid out in phase
 * order, so the serial→parallel shape + per-step health read at a glance.
 */
function ShapeGlyph({ steps }: { steps: WorkflowStep[] }) {
  if (!steps.length) return <span className="text-[11px] text-muted-foreground">—</span>;
  const phases = inferWorkflowShape(orderForGlyph(steps));
  return (
    <span className="flex flex-wrap items-center gap-1.5">
      {phases.map((phase) => (
        <span key={phase.phase} className="flex items-center gap-0.5">
          {phase.steps.map((step) => (
            <StatusDot key={step.id} tone={workflowStepTone(step.status)} className="size-1.5" />
          ))}
          <span className="text-[10px] text-muted-foreground">{phase.phase}</span>
        </span>
      ))}
    </span>
  );
}

/* ================================================================== */
/* DETAIL — one run, top-to-bottom report                              */
/* ================================================================== */
