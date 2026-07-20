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
import { normalizeBaseUrl } from "../api";
import type {
  HarnessTurnEvent,
  ProviderSession,
  WorkflowRun,
  WorkflowStep,
} from "../types";
import type { SelectionState } from "../app/selection";
import { TurnDrillIn } from "./Surfaces";

interface WorkflowSurfaceProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
  apiUrl?: string;
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

export function WorkflowRunDetail({ model, onSelectionChange, apiUrl }: WorkflowSurfaceProps) {
  const run = model.selectedWorkflowRun;
  const back = () => onSelectionChange({ surface: "workflows", workflowRunId: undefined });

  if (!run) {
    return (
      <DocumentSurface>
        <BackRow onBack={back} />
        <EmptyState
          icon={Workflow}
          title="Workflow run not found"
          description="It may not have streamed yet, or the source is offline."
        />
      </DocumentSurface>
    );
  }

  const steps = model.selectedWorkflowSteps;
  const sessions = model.snapshot.provider_sessions ?? [];
  const tone = workflowRunTone(run.status);
  const headerTone = run.status === "failed" ? "decision" : tone;
  const headerStatus = run.status === "failed" ? "needs review" : run.status === "completed" ? "passed" : run.status;
  const running = tone === "running";
  const phases = inferWorkflowShape(steps);
  const duration = formatDuration(run.created_at, run.ended_at);
  const specScript = workflowScriptFromRun(run);
  const parsedVerdict = parseVerdictSummary(readableWorkflowOutput(run.summary) ?? run.summary ?? "");
  const terminal = terminalReasonInfo(run.terminal_reason);
  const partial = splitPartialOutputSteps(steps);

  // Prev/next stepper over the (ordered) runs list, so cross-run scanning
  // survives without a standing rail.
  const runs = model.workflowRuns;
  const index = runs.findIndex((r) => r.id === run.id);
  const goto = (i: number) => {
    const target = runs[i];
    if (target) onSelectionChange({ surface: "workflows", workflowRunId: target.id });
  };

  return (
    <DocumentSurface className="max-w-[1120px]">
      <header className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <BackRow onBack={back} />
          {runs.length > 1 && (
            <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
              <button
                type="button"
                disabled={index <= 0}
                onClick={() => goto(index - 1)}
                className="rounded p-0.5 transition-colors hover:text-foreground disabled:opacity-40"
                aria-label="Previous run"
              >
                <ChevronUp className="size-3.5" />
              </button>
              <button
                type="button"
                disabled={index < 0 || index >= runs.length - 1}
                onClick={() => goto(index + 1)}
                className="rounded p-0.5 transition-colors hover:text-foreground disabled:opacity-40"
                aria-label="Next run"
              >
                <ChevronDown className="size-3.5" />
              </button>
            </div>
          )}
        </div>

        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <Avatar name={run.workflow_name} tone={headerTone} size="lg" />
            <div className="min-w-0">
              <h1 className="truncate text-2xl font-semibold tracking-tight text-foreground">
                {run.workflow_name}
              </h1>
              <div className="mt-1 flex flex-wrap items-center gap-1.5">
                <Badge tone={headerTone}>{headerStatus}</Badge>
                {run.dry_run && <Badge tone="warn">dry-run</Badge>}
                {terminal && terminal.reason !== "completed" && (
                  <Badge tone={terminal.tone} title={terminal.gloss}>{terminal.label}</Badge>
                )}
              </div>
            </div>
          </div>
        </div>

      </header>

      {run.partial_output_available && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2 text-xs text-muted-foreground">
          This run ended before full acceptance. {partial.usable.length} completed or cached step{partial.usable.length === 1 ? "" : "s"} remain usable; {partial.invalid.length} step{partial.invalid.length === 1 ? "" : "s"} require review.
        </div>
      )}

      <div className="grid gap-3 xl:grid-cols-[minmax(0,1.35fr)_minmax(18rem,0.65fr)]">
        {run.spec != null && (
          specScript ? (
            <WorkflowDefinitionPreview
              script={compactWorkflowScript(specScript, 4000)}
              steps={steps}
              stepHref={(step) => `#${workflowStepDomId(step.label)}`}
              heading="Workflow spec"
              showPlanSummary
              collapseExtraStepsOnMobile
            />
          ) : (
            <SpecDisclosure spec={run.spec} />
          )
        )}

        <div className="min-w-0 space-y-3">
          <div className="rounded-md border border-border bg-card/70 px-3 py-2.5">
            <WorkflowExecutionSnapshot run={run} steps={steps} />
          </div>
          <WorkflowRunVerdictBanner run={run} steps={steps} parsed={parsedVerdict} tone={headerTone} />
        </div>
      </div>

      <div className="grid gap-3 lg:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
        <WorkflowRunContextStrip
          run={run}
          steps={steps}
          parsed={parsedVerdict}
        />
        <section className="min-w-0 rounded-lg border border-border bg-card/70 p-3">
          <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Activity className="size-3" />
            Execution summary
          </div>
          <div className="mt-2">
            <TimelinePreview steps={steps} />
          </div>
        </section>
      </div>

      {!running && (
        <details className="group rounded-lg border border-border/70 bg-card/50">
          <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground">
            <ChevronRight className="size-3.5 transition-transform group-open:rotate-90" />
            Review gate details
          </summary>
          <div className="border-t border-border/70 p-3">
            <VerdictCard run={run} steps={steps} tone={tone} />
          </div>
        </details>
      )}

      <DocSection label="Detailed workflow timeline">
        {phases.length ? (
          <Timeline phases={phases} sessions={sessions} model={model} apiUrl={apiUrl} run={run} onSelectionChange={onSelectionChange} />
        ) : (
          <EmptyState
            icon={Workflow}
            title="No steps yet"
            description="Steps animate in here as the run progresses."
          />
        )}
      </DocSection>

      <DocSection label="Runtime metrics">
        <RunSummary run={run} steps={steps} />
      </DocSection>

      {run.design_intent && (
        <DocSection label="Design intent">
          <div className="rounded-md border border-primary/25 bg-primary/5 p-3 text-[13px] leading-relaxed text-foreground">
            {run.design_intent}
          </div>
        </DocSection>
      )}

      <DocSection label="Run metadata">
        <DocProperties
          items={[
            { label: "Run id", value: <MonoId>{run.id}</MonoId> },
            {
              label: "Initiated by",
              value: run.initiated_by ? (
                <span className="inline-flex items-center gap-1.5">
                  <Avatar name={run.initiated_by} tone="idle" />
                  {run.initiated_by}
                </span>
              ) : (
                "—"
              ),
            },
            { label: "Trace", value: <TraceIndicator retention={run.trace_retention} /> },
            { label: "Started", value: <Timestamp value={run.created_at} /> },
            {
              label: "Ended",
              value: run.ended_at ? <Timestamp value={run.ended_at} /> : "running…",
            },
            { label: "Duration", value: running ? "running" : (duration ?? "—") },
            {
              label: "Shape",
              value: phases.length ? describeShape(phases) : "—",
            },
          ]}
        />
      </DocSection>

      <DocSection label="Definition">
        <Definition phases={phases} workflowName={run.workflow_name} apiUrl={apiUrl} />
      </DocSection>
    </DocumentSurface>
  );
}

function WorkflowRunContextStrip({
  run,
  steps,
  parsed,
}: {
  run: WorkflowRun;
  steps: WorkflowStep[];
  parsed: { result: string; criterion?: string; detail?: string };
}) {
  const finished = steps.filter((step) => isTerminal(step.status)).length;
  const evidenceOutputs = steps.filter((step) => step.output_summary?.trim()).length;
  const failed = steps.filter((step) => step.status === "failed").length;
  const context = "standalone workflow";
  return (
    <section className="grid gap-2 rounded-lg border border-border bg-card/70 p-3 text-[12px] sm:grid-cols-2 xl:grid-cols-[0.8fr_0.7fr_0.7fr_minmax(0,1.8fr)]">
      <WorkflowContextItem label="Execution context" value={context} />
      <WorkflowContextItem
        label="Run stages"
        value={steps.length ? `${finished}/${steps.length} passed` : "not started"}
        detail={failed > 0 ? `${failed} failed` : formatDuration(run.created_at, run.ended_at) ?? "running"}
      />
      <WorkflowContextItem
        label="Evidence"
        value={evidenceOutputs > 0 ? `${evidenceOutputs} check artifact${evidenceOutputs === 1 ? "" : "s"}` : "none yet"}
        detail={evidenceOutputs > 0 ? "review evidence recorded" : "waiting for run evidence"}
      />
      <WorkflowContextItem
        label="Review criterion"
        value={parsed.criterion ?? "not recorded"}
        detail={run.status === "failed" ? "review outcome below" : plainVerdictResult(parsed.result)}
      />
    </section>
  );
}

function WorkflowPlanOverview({
  run,
  steps,
  parsed,
  phases,
}: {
  run: WorkflowRun;
  steps: WorkflowStep[];
  parsed: { result: string; criterion?: string; detail?: string };
  phases: WorkflowPhase[];
}) {
  const context = "standalone workflow";
  const purpose = run.design_intent ?? parsed.criterion ?? "Workflow run";
  const shape = phases.length ? readableWorkflowShape(phases, steps.length) : `${steps.length} stage${steps.length === 1 ? "" : "s"}`;
  const evidenceOutputs = steps.filter((step) => step.output_summary?.trim()).length;

  return (
    <section className="rounded-lg border border-border bg-card/70 px-3 py-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          <Workflow className="size-3" />
          Workflow spec
        </span>
        <Badge tone="idle">{context}</Badge>
      </div>
      <p className="mt-2 text-[13px] leading-relaxed text-foreground/85">
        {purpose}
      </p>
      <div className="mt-2 grid gap-2 sm:grid-cols-3">
        <WorkflowOverviewFact label="Execution plan" value={shape} />
        <WorkflowOverviewFact
          label="Evidence"
          value={evidenceOutputs > 0 ? `${evidenceOutputs} check artifact${evidenceOutputs === 1 ? "" : "s"}` : "none yet"}
        />
        <WorkflowOverviewFact
          label="Acceptance"
          value={parsed.criterion ?? "not recorded"}
        />
      </div>
    </section>
  );
}

function readableWorkflowShape(phases: WorkflowPhase[], totalSteps: number): string {
  const parallelGroups = phases.filter((phase) => phase.kind === "parallel").length;
  const serialSteps = phases
    .filter((phase) => phase.kind === "serial")
    .reduce((sum, phase) => sum + phase.steps.length, 0);
  if (parallelGroups > 0) {
    return `${totalSteps} stages: ${serialSteps} serial, ${parallelGroups} parallel group${parallelGroups === 1 ? "" : "s"}`;
  }
  return `${totalSteps} serial stage${totalSteps === 1 ? "" : "s"}`;
}

function WorkflowExecutionSnapshot({ run, steps }: { run: WorkflowRun; steps: WorkflowStep[] }) {
  const failed = steps.filter((step) => step.status === "failed").length;
  const running = steps.filter((step) => step.status === "running").length;
  const finished = steps.filter((step) => isTerminal(step.status)).length;
  const currentStep = steps.find((step) => step.status === "running")
    ?? steps.find((step) => step.status === "failed")
    ?? steps.find((step) => step.status === "queued" || step.status === "planned")
    ?? [...steps].reverse().find((step) => step.status);
  const total = steps.length || finished;
  const tone: StatusTone = failed > 0 || run.status === "failed"
    ? "bad"
    : running > 0
      ? "running"
      : run.status === "completed"
        ? "good"
        : "idle";
  const title = failed > 0 || run.status === "failed"
    ? "needs review"
    : running > 0
      ? "running"
      : finished > 0
        ? "passed"
        : "not started";
  const detail = failed > 0 || run.status === "failed"
    ? "Live execution finished; the review verdict explains which acceptance criterion needs work."
    : running > 0
      ? `${running} run stage${running === 1 ? "" : "s"} running now.`
      : total > 0
        ? `${finished}/${total} run stages passed.`
        : "Run stages are not started.";
  const currentLabel = currentStep ? workflowTitleFromLabel(currentStep.label) : undefined;
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-1.5 text-[11px] font-semibold text-muted-foreground">
        <Activity className="size-3" />
        Live execution
      </div>
      <div className="rounded-md border border-border/70 bg-background/50 px-2.5 py-2">
        <div className="flex items-center gap-1.5 text-[12px] font-semibold text-foreground">
          <StatusDot tone={tone} pulse={tone === "running"} />
          {title}
        </div>
        <p className="mt-1 text-[12px] leading-relaxed text-foreground/80 max-sm:hidden">{detail}</p>
        {currentLabel && (
          <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
            Current stage: <span className="font-medium text-foreground/80">{currentLabel}</span>
          </p>
        )}
      </div>
      <div className="space-y-1">
        <div className="flex items-center justify-between text-[10px] font-medium text-muted-foreground">
          <span>Live execution progress</span>
          <span>{finished}/{total}</span>
        </div>
        <div className="h-1 overflow-hidden rounded-full bg-muted">
          <div
            className={cn("h-full rounded-full", failed > 0 ? "bg-status-bad/65" : "bg-status-good")}
            style={{ width: `${total ? Math.min(100, Math.round((finished / total) * 100)) : 0}%` }}
          />
        </div>
      </div>
    </div>
  );
}

function workflowTitleFromLabel(label: string): string {
  return label
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => (part.toLowerCase() === "ux" ? "UX" : part.charAt(0).toUpperCase() + part.slice(1)))
    .join(" ");
}

function WorkflowOverviewFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-md bg-muted/20 px-2 py-1.5">
      <div className="text-[10px] font-medium text-muted-foreground">{label}</div>
      <p className="mt-0.5 text-[12px] leading-snug text-foreground/85">{value}</p>
    </div>
  );
}

function WorkflowContextItem({
  label,
  value,
  detail,
}: {
  label: string;
  value: ReactNode;
  detail?: string;
}) {
  return (
    <div className="min-w-0">
      <div className="text-[10px] font-medium text-muted-foreground">{label}</div>
      <div className="mt-1 line-clamp-2 text-[13px] font-medium leading-snug text-foreground">{value}</div>
      {detail && <div className="mt-0.5 text-[11px] leading-snug text-muted-foreground">{detail}</div>}
    </div>
  );
}

function WorkflowRunVerdictBanner({
  run,
  steps,
  parsed,
  tone,
}: {
  run: WorkflowRun;
  steps: WorkflowStep[];
  parsed: { result: string; criterion?: string; detail?: string };
  tone: StatusTone;
}) {
  const finished = steps.filter((step) => isTerminal(step.status)).length;
  const failed = steps.filter((step) => step.status === "failed").length;
  const running = steps.filter((step) => step.status === "running").length;
  const evidenceOutputs = steps.filter((step) => step.output_summary?.trim()).length;
  const statusLabel = run.status === "failed"
      ? "needs review"
    : run.status === "completed"
      ? "passed"
    : run.status === "running"
        ? "running"
        : run.status || "not started";
  const detail = run.status === "failed"
    ? failed > 0
      ? `${failed} run stage${failed === 1 ? "" : "s"} needs reviewer attention before acceptance.`
      : `Waiting for reviewer approval: ${parsed.criterion ?? "review gate requested changes"}.`
    : run.status === "completed"
      ? `${finished}/${steps.length || finished} run stages passed.`
    : running > 0
      ? `${running} run stage${running === 1 ? "" : "s"} running now.`
      : "The current run is waiting for execution data.";
  const issue = parsed.detail ?? parsed.result;
  const tint =
    tone === "bad" || run.status === "failed"
	      ? "border-status-bad/25 bg-status-bad/6"
      : tone === "good"
        ? "border-status-good/25 bg-status-good/8"
        : "border-border bg-card/70";

  return (
    <section className={cn("rounded-md border px-3 py-2", tint)}>
      <div className="grid gap-2">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone={tone}>{statusLabel}</Badge>
            {run.status === "failed" && issue && <Badge tone="warn">acceptance issue</Badge>}
            {evidenceOutputs > 0 && (
              <Badge tone="good">{evidenceOutputs} check artifact{evidenceOutputs === 1 ? "" : "s"}</Badge>
            )}
            <span className="min-w-0 text-[13px] font-semibold leading-snug text-foreground">{detail}</span>
          </div>
          {run.status === "failed" && issue && (
            <details className="group mt-2 rounded-md border border-status-bad/20 bg-background/45 px-2 py-1.5">
              <summary className="flex cursor-pointer list-none flex-wrap items-center gap-1.5 text-[12px] leading-snug text-foreground/85 transition-colors hover:text-foreground">
                <span className="shrink-0 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                  Acceptance issue
                </span>
                <span className="min-w-0 flex-1">{compactReviewIssueSummary(issue)}</span>
                <span className="shrink-0 rounded-md border border-border/70 bg-background px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground group-open:hidden">
                  Open evidence
                </span>
              </summary>
              <div className="mt-2 border-t border-border/60 pt-2">
                <VerdictIssueRows detail={issue} />
              </div>
            </details>
          )}
        </div>
        <div className="grid gap-1">
          <VerdictBannerFact
            label="Workflow plan"
            value={`${finished}/${steps.length || finished}`}
            detail="run stages passed"
            tone={failed > 0 ? "warn" : "good"}
          />
	          <VerdictBannerFact
	            label="Checks"
	            value={evidenceOutputs > 0 ? `${evidenceOutputs} check artifacts` : "not started"}
	            detail={evidenceOutputs > 0 ? "review evidence recorded" : "waiting for stage output"}
	            tone={evidenceOutputs > 0 ? "good" : "idle"}
	          />
          <VerdictBannerFact
            label="Review gate"
            value={run.status === "failed" ? "needs review" : parsed.result ? plainVerdictResult(parsed.result) : "pending"}
            detail={parsed.criterion ? "acceptance criterion set" : "waiting for review"}
            tone={run.status === "failed" ? "bad" : tone}
          />
        </div>
      </div>
    </section>
  );
}

function VerdictBannerFact({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string;
  detail: string;
  tone: StatusTone;
}) {
  return (
    <div className="min-w-0 rounded-md bg-background/50 px-2 py-1">
      <div className="flex items-center gap-1.5 text-[10px] font-medium text-muted-foreground">
        <StatusDot tone={tone} pulse={tone === "running"} />
        {label}
      </div>
      <p className="mt-0.5 truncate text-[12px] font-medium text-foreground">{value}</p>
      <p className="truncate text-[10px] text-muted-foreground">{detail}</p>
    </div>
  );
}

function WorkflowFailureSummary({
  run,
  steps,
  parsed,
}: {
  run: WorkflowRun;
  steps: WorkflowStep[];
  parsed: { result: string; criterion?: string; detail?: string };
}) {
  const finished = steps.filter((step) => isTerminal(step.status)).length;
  const failed = steps.filter((step) => step.status === "failed").length;
  const detail = compactVerdictDetail(parsed.detail ?? parsed.result);
  return (
    <section className="rounded-lg border border-status-bad/25 bg-card px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone="bad">needs review</Badge>
        <span className="text-[13px] font-medium text-foreground">
          {failed > 0
            ? `${failed} action${failed === 1 ? "" : "s"} failed.`
            : `${finished}/${steps.length || finished} actions finished; review gate requested changes.`}
        </span>
      </div>
      <p className="mt-1 line-clamp-2 text-[12px] leading-relaxed text-foreground/80">
        {detail}
      </p>
    </section>
  );
}

/**
 * The "trace: durable|live|expired" indicator for a run. "durable" keeps the
 * heavy per-node turn-event trace so a completed run can be drilled into; "live"
 * streams it over SSE during execution but retains nothing afterwards;
 * "expired" was durable but its trace was later swept by the retention-window
 * GC (`harness workflow gc-trace`) — the audit record stays, the heavy trace is
 * gone.
 */
function TraceIndicator({ retention }: { retention?: string }) {
  const value = retention ?? "durable";
  const durable = value === "durable";
  const caption =
    value === "durable"
      ? "per-node trace retained"
      : value === "expired"
        ? "trace swept by retention GC"
        : "streamed live, not retained";
  return (
    <span className="inline-flex items-center gap-1.5">
      <Badge tone={durable ? "info" : "idle"}>trace: {value}</Badge>
      <span className="text-[11px] text-muted-foreground">{caption}</span>
    </span>
  );
}

/** USD per 1M tokens [input, output] — rough public list prices; ESTIMATE only. */
const TOKEN_RATES: { match: RegExp; in: number; out: number }[] = [
  { match: /claude|sonnet|opus|haiku/i, in: 3, out: 15 },
  { match: /gpt-5|codex|o[0-9]/i, in: 1.25, out: 10 },
];
function rateFor(model?: string | null): { in: number; out: number } {
  const hit = model ? TOKEN_RATES.find((r) => r.match.test(model)) : undefined;
  return hit ?? { in: 2, out: 10 };
}

/** Parse a `unix-ms:<n>` (or ISO) timestamp to epoch ms; NaN if unparseable. */
function parseMs(ts?: string | null): number {
  if (!ts) return NaN;
  const m = ts.match(/^unix-ms:(\d+)$/);
  return m ? Number(m[1]) : Date.parse(ts);
}

/**
 * Max number of step windows overlapping at once — the OBSERVED parallelism.
 * Prefers the worker's real `duration_ms` (captured at completion) for the end
 * bound: journaled `ended_at` is stamped at run-finalize time for every step, so
 * a serial step looks like it ran until the run ended and would falsely overlap.
 */
function maxOverlap(steps: WorkflowStep[]): number {
  const edges: [number, number][] = [];
  for (const step of steps) {
    const start = parseMs(step.started_at);
    if (Number.isNaN(start)) continue;
    const dur = step.result?.duration_ms;
    const endRaw =
      dur != null ? start + dur : step.ended_at ? parseMs(step.ended_at) : Date.now();
    const end = Number.isNaN(endRaw) ? start : Math.max(endRaw, start);
    edges.push([start, 1], [end, -1]);
  }
  // Closes (-1) before opens (+1) at equal timestamps so touching windows do
  // not count as overlapping.
  edges.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  let current = 0;
  let max = 0;
  for (const [, delta] of edges) {
    current += delta;
    if (current > max) max = current;
  }
  return max;
}

/**
 * Run-level rollup from the per-step observability fields: workers, observed
 * parallelism, wall-clock, total tokens, a rough cost estimate, and the failed
 * count. Token/cost stats appear once durable workers report usage.
 */
function RunSummary({ run, steps }: { run: WorkflowRun; steps: WorkflowStep[] }) {
  let tokIn = 0;
  let tokOut = 0;
  let tokTotal = 0;
  let cost = 0;
  let costExact = false; // true once any step contributed a provider-reported cost
  let failed = 0;
  for (const step of steps) {
    const result = step.result;
    if (result?.tokens) {
      tokIn += result.tokens.input;
      tokOut += result.tokens.output;
      tokTotal += result.tokens.total;
    }
    // Prefer the provider's EXACT billed cost (claude `total_cost_usd`, captured
    // onto the step); fall back to a token-rate ESTIMATE only when absent (codex
    // reports no dollar figure). Mixing is fine — the label reflects whether any
    // exact figure was used.
    if (typeof result?.cost_usd === "number") {
      cost += result.cost_usd;
      costExact = true;
    } else if (result?.tokens) {
      const rate = rateFor(result.model);
      cost += (result.tokens.input / 1e6) * rate.in + (result.tokens.output / 1e6) * rate.out;
    }
    if (result?.failure?.failed) failed += 1;
  }
  const parallelism = maxOverlap(steps);
  const wall = run.ended_at ? parseMs(run.ended_at) - parseMs(run.created_at) : NaN;

  const stats: { label: string; value: ReactNode; bad?: boolean }[] = [
    { label: "Workers", value: formatCount(steps.length) },
    { label: "Parallelism", value: `${parallelism}×` },
  ];
  if (!Number.isNaN(wall) && wall >= 0) stats.push({ label: "Wall-clock", value: formatMillis(wall) });
  if (tokTotal > 0) {
    stats.push({
      label: "Tokens",
      value: `${formatCount(tokTotal)} (${formatCount(tokIn)} in · ${formatCount(tokOut)} out)`,
    });
  }
  if (cost > 0) {
    // "Cost" once any provider-reported figure is in the total; "Est. cost" (≈)
    // when it is purely token-rate estimated (e.g. a codex-only run).
    stats.push({
      label: costExact ? "Cost" : "Est. cost",
      value: `${costExact ? "" : "≈ "}$${cost < 0.01 ? cost.toFixed(4) : cost.toFixed(2)}`,
    });
  }
  if (failed > 0) stats.push({ label: "Failed", value: formatCount(failed), bad: true });

  return (
    <div className="flex flex-wrap gap-x-6 gap-y-2">
      {stats.map((stat) => (
        <div key={stat.label} className="flex flex-col">
          <span className="text-[10px] uppercase tracking-wider text-muted-foreground">{stat.label}</span>
          <span
            className={cn(
              "text-[13px] tabular-nums",
              stat.bad ? "text-status-bad" : "text-foreground",
            )}
          >
            {stat.value}
          </span>
        </div>
      ))}
      {tokTotal === 0 && (
        <span className="self-center text-[12px] text-muted-foreground">
          Token usage appears once durable workers report it.
        </span>
      )}
    </div>
  );
}

/**
 * Collapsible pretty-printed view of the run's authored source — the Starlark
 * program snapshotted as `{ lang: "starlark", script }`. Reuses the same
 * fenced-code styling as the Rust source / Markdown code blocks so the dynamic
 * spec reads as the run's durable audit record.
 */
function SpecDisclosure({ spec }: { spec: unknown }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  // `spec` is journaled as `{ lang: "starlark", script }`; show the ACTUAL
  // source, not the escaped JSON wrapper. Fall back to a string / pretty JSON.
  const source = (() => {
    if (
      spec &&
      typeof spec === "object" &&
      typeof (spec as { script?: unknown }).script === "string"
    ) {
      return (spec as { script: string }).script;
    }
    if (typeof spec === "string") return spec;
    try {
      return JSON.stringify(spec, null, 2);
    } catch {
      return String(spec);
    }
  })();
  const lineCount = source.split("\n").length;
  const copy = () => {
    void navigator.clipboard?.writeText(source).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };
  return (
    <div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          className="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
        >
          {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
          <Code className="size-3" />
          View spec · Starlark source
          <span className="text-muted-foreground/70">· {lineCount} lines</span>
        </button>
        {open && (
          <button
            type="button"
            onClick={copy}
            className="text-[10px] text-muted-foreground transition-colors hover:text-foreground"
          >
            {copied ? "copied ✓" : "copy"}
          </button>
        )}
      </div>
      {open && (
        <pre className="mt-1.5 max-h-96 overflow-auto whitespace-pre rounded-md border border-border bg-muted/30 p-2 font-mono text-[11px] leading-relaxed text-foreground">
          {source}
        </pre>
      )}
    </div>
  );
}

function BackRow({ onBack }: { onBack: () => void }) {
  return (
    <button
      type="button"
      onClick={onBack}
      className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground transition-colors hover:text-foreground"
    >
      <ChevronLeft className="size-3.5" /> Workflows
    </button>
  );
}

function TimelinePreview({ steps }: { steps: WorkflowStep[] }) {
  if (steps.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-border bg-muted/20 px-3 py-2 text-[12px] text-muted-foreground">
        Runtime steps will appear here when the workflow starts.
      </div>
    );
  }
  const preview = steps.slice(0, 4);
  return (
    <div className="grid gap-2">
        {preview.map((step) => {
          const tone = workflowStepTone(step.status);
          const output = readableWorkflowOutput(step.output_summary);
          const role = roleHintFromLabel(step.label);
          return (
            <a
              key={step.id}
              href={`#${workflowStepDomId(step.label)}`}
              className="grid min-w-0 gap-2 rounded-lg border border-border bg-card/60 px-3 py-2 text-left transition-colors hover:border-input hover:bg-muted/20 sm:grid-cols-[auto_minmax(0,1fr)_auto]"
            >
              <span className="mt-0.5 flex items-center gap-1.5">
                <StatusDot tone={tone} pulse={tone === "running"} />
                <Badge tone={tone}>{shortStepStatusLabel(step.status)}</Badge>
              </span>
              <span className="min-w-0">
                <span className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                  <Timestamp value={step.started_at} />
                  {role && <span>{role}</span>}
                </span>
                <span className="mt-0.5 block truncate text-[12px] font-semibold text-foreground">
                  {workflowStepEventTitle(step)}
                </span>
                {output && (
                  <span className="mt-0.5 block line-clamp-2 text-[11px] leading-snug text-muted-foreground">
                    {workflowStepEventDetail(output)}
                  </span>
                )}
              </span>
              <span className="self-center text-[11px] text-muted-foreground">{stepTiming(step)}</span>
            </a>
          );
        })}
      {steps.length > preview.length && (
        <a
          href={`#${workflowStepDomId(steps[preview.length].label)}`}
          className="block rounded-md border border-border bg-muted/20 px-3 py-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground"
        >
          +{steps.length - preview.length} more runtime event{steps.length - preview.length === 1 ? "" : "s"}
        </a>
      )}
    </div>
  );
}

function workflowStepEventTitle(step: WorkflowStep): string {
  const label = step.label.replace(/[-_]+/g, " ").trim();
  if (step.status === "running") return `${titleCase(label)} is running`;
  if (step.status === "failed") return `${titleCase(label)} needs review`;
  if (step.status === "queued") return `${titleCase(label)} is queued`;
  if (step.status === "completed" || step.status === "cached") return `${titleCase(label)} finished`;
  return titleCase(label);
}

function workflowStepEventDetail(output: string): string {
  const lower = output.toLowerCase();
  if (
    lower.includes("next actions")
    || lower.includes("next_action")
    || lower.includes("findings")
    || lower.includes("run plan")
    || lower.includes("debug language")
  ) {
    return "Findings and next actions captured for review.";
  }
  return output
    .split(/\n+/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 2)
    .join(" ");
}

function shortStepStatusLabel(status: string): string {
  const value = status.toLowerCase();
  if (value === "completed" || value === "cached") return "passed";
  if (value === "failed") return "blocked";
  if (value === "queued" || value === "planned") return "not started";
  return status;
}

/** Terminal-run verdict card, tinted by run tone, with a plain-English gloss. */
function VerdictCard({
  run,
  steps,
  tone,
}: {
  run: WorkflowRun;
  steps: WorkflowStep[];
  tone: StatusTone;
}) {
  const parsed = parseVerdictSummary(readableWorkflowOutput(run.summary) ?? run.summary ?? "—");
  const tint =
    tone === "bad"
      ? "border-status-bad/30 bg-status-bad/12"
      : tone === "good"
        ? "border-status-good/30 bg-status-good/12"
        : "border-border bg-muted/30";
  return (
    <div className={cn("space-y-1.5 rounded-lg border p-3", tint)}>
      <div className="grid gap-2 sm:grid-cols-[minmax(0,0.75fr)_minmax(0,1.25fr)]">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Result
          </p>
          <p className="mt-0.5 text-[13px] font-medium text-foreground">{plainVerdictResult(parsed.result)}</p>
        </div>
        {parsed.criterion && (
          <div>
            <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Criterion
            </p>
            <p className="mt-0.5 text-[12px] leading-snug text-foreground/85">{parsed.criterion}</p>
          </div>
        )}
      </div>
      {parsed.detail && <VerdictIssueRows detail={parsed.detail} />}
      <p className="text-xs text-muted-foreground">{verdictGloss(run, steps)}</p>
    </div>
  );
}

function VerdictIssueRows({ detail }: { detail: string }) {
  const normalizedDetail = normalizeWorkflowUiLanguage(detail);
  const issues = splitVerdictIssues(normalizedDetail);
  if (issues.length <= 1) {
    const summary = compactVerdictDetail(normalizedDetail);
    if (summary === normalizedDetail) {
      return <p className="text-[12px] leading-relaxed text-foreground/85">{normalizedDetail}</p>;
    }
    return (
      <details className="group rounded-md border border-border/70 bg-background/55 px-2 py-1.5">
        <summary className="cursor-pointer list-none text-[12px] leading-snug text-foreground/85 transition-colors hover:text-foreground">
          {summary}
          <span className="ml-1 text-[10px] font-medium text-muted-foreground group-open:hidden">
            more
          </span>
        </summary>
        <p className="mt-1.5 border-t border-border/60 pt-1.5 text-[12px] leading-relaxed text-muted-foreground">
          {normalizedDetail}
        </p>
      </details>
    );
  }
  return (
    <div className="space-y-1.5">
      {issues.slice(0, 5).map((issue, index) => (
        <div key={index} className="flex gap-2 rounded-md border border-border/70 bg-background/55 px-2 py-1.5">
          <Badge tone={issue.severity === "P0" ? "bad" : issue.severity === "P1" ? "warn" : "idle"}>
            {issue.severity}
          </Badge>
          <p className="min-w-0 text-[12px] leading-snug text-foreground/85">{issue.text}</p>
        </div>
      ))}
      {issues.length > 5 && (
        <p className="text-[10px] text-muted-foreground">+{issues.length - 5} more findings in step output</p>
      )}
    </div>
  );
}

function compactVerdictDetail(detail: string): string {
  if (detail.length <= 220) return detail;
  const sentence = detail.match(/^(.{80,220}?[.!?])\s/)?.[1]?.trim();
  return sentence ?? `${detail.slice(0, 210).trim()}...`;
}

function compactReviewIssueSummary(detail: string): string {
  const issue = splitVerdictIssues(normalizeWorkflowUiLanguage(detail))[0];
  const severity = issue?.severity?.match(/^P[0-3]$/) ? `${issue.severity} acceptance issue` : "acceptance issue";
  return `Review found a ${severity}; open evidence and rationale.`;
}

function splitVerdictIssues(detail: string): { severity: string; text: string }[] {
  const matches = Array.from(detail.matchAll(/(P[0-3]):\s*([\s\S]*?)(?=\s+P[0-3]:|$)/g));
  if (!matches.length) return [{ severity: "note", text: detail }];
  return matches.map((match) => ({
    severity: match[1] ?? "note",
    text: (match[2] ?? "").trim(),
  })).filter((issue) => issue.text.length > 0);
}

function parseVerdictSummary(summary: string): { result: string; criterion?: string; detail?: string } {
  const cleaned = summary.replace(/\s+/g, " ").trim();
  const match = cleaned.match(/^(.*?)\s+\[criterion:\s*(.*?)\]\s+[—-]\s+(.*)$/);
  if (match) {
    return {
      result: match[1]?.trim() || "Verdict recorded",
      criterion: match[2]?.trim(),
      detail: match[3]?.trim(),
    };
  }
  const [head, ...rest] = cleaned.split(/\s+[—-]\s+/);
  return {
    result: head || "Verdict recorded",
    detail: rest.join(" - ") || undefined,
  };
}

function plainVerdictResult(result: string): string {
  const cleaned = result.replace(/^.*verdict:\s*/i, "").trim();
  if (/intent\s+NOT\s+met/i.test(cleaned)) return "acceptance failed";
  if (/intent\s+met/i.test(cleaned)) return "acceptance passed";
  return cleaned || "verdict recorded";
}

/* ================================================================== */
/* Timeline (the dominant block)                                       */
/* ================================================================== */

function Timeline({
  phases,
  sessions,
  model,
  apiUrl,
  run,
  onSelectionChange,
}: {
  phases: WorkflowPhase[];
  sessions: ProviderSession[];
  model: WorkbenchModel;
  apiUrl?: string;
  run: WorkflowRun;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  return (
    <div className="space-y-3">
      {phases.map((phase) => (
        <div key={phase.phase} className="space-y-2.5 rounded-lg border border-border bg-card/45 p-3">
          <div className="flex items-center justify-between gap-2">
            <span className="inline-flex min-w-0 items-center gap-2">
              <StatusDot
                tone={phase.steps.every((step) => isTerminal(step.status)) ? "good" : "running"}
                pulse={phase.steps.some((step) => step.status === "running")}
              />
              <span className="truncate text-[12px] font-semibold text-foreground">
                {titleCase(phase.phase)} phase
              </span>
            </span>
            <Badge tone="idle">
              {phase.kind === "serial"
                ? `${phase.steps.length} serial step${phase.steps.length === 1 ? "" : "s"}`
                : `${phase.steps.length} parallel steps`}
            </Badge>
          </div>

          {phase.kind === "parallel" && <GanttStrip steps={phase.steps} />}

          <div className="space-y-2.5">
            {phase.steps.map((step) => (
              <StepCard
                key={step.id}
                step={step}
                phase={phase}
                sessions={sessions}
                model={model}
                apiUrl={apiUrl}
                run={run}
                onSelectionChange={onSelectionChange}
              />
            ))}
          </div>

          {phase.kind === "parallel" && <JoinBar steps={phase.steps} />}
        </div>
      ))}
    </div>
  );
}

/** Inline gantt: one thin bar per step, positioned within the phase window. */
function GanttStrip({ steps }: { steps: WorkflowStep[] }) {
  const window = phaseWindow(steps);
  return (
    <div className="space-y-1">
      <div className="hidden space-y-1 sm:block">
        {steps.map((step) => {
          const geo = stepGanttGeometry(step, window);
          const tone = workflowStepTone(step.status);
          return (
            <div key={step.id} className="flex items-center gap-2">
              <span className="w-20 shrink-0 truncate text-[10px] text-muted-foreground">
                {step.label}
              </span>
              <div className="relative h-1 flex-1 rounded-full bg-muted">
                <div
                  className={cn("absolute h-1 rounded-full", toneBarClass(tone))}
                  style={{ left: `${geo.left}%`, width: `${geo.width}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
      <p className="text-[10px] text-muted-foreground sm:hidden">ran concurrently</p>
    </div>
  );
}

function toneBarClass(tone: StatusTone): string {
  switch (tone) {
    case "running":
      return "bg-status-running/60";
    case "good":
      return "bg-status-good/60";
    case "bad":
      return "bg-status-bad/60";
    case "info":
      return "bg-status-info/60";
    default:
      return "bg-status-idle/60";
  }
}

function titleCase(value: string): string {
  return value
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

/** The barrier join bar; settles when every step in the phase is terminal. */
function JoinBar({ steps }: { steps: WorkflowStep[] }) {
  const allTerminal = steps.every((step) => isTerminal(step.status));
  const tone: StatusTone = allTerminal ? "good" : "running";
  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-md border px-2 py-1 text-[10px] uppercase tracking-wider",
        allTerminal
          ? "border-status-good/30 bg-status-good/8 text-status-good"
          : "border-border bg-muted/30 text-muted-foreground",
      )}
    >
      <StatusDot tone={tone} pulse={!allTerminal} />
      {allTerminal
        ? `parallel group complete — all ${steps.length} steps resolved`
        : `parallel group waiting — ${steps.filter((s) => !isTerminal(s.status)).length} of ${steps.length} unresolved`}
    </div>
  );
}

function readableWorkflowOutput(summary?: string | null): string | undefined {
  const trimmed = summary?.trim();
  if (!trimmed) return undefined;
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return normalizeWorkflowUiLanguage(trimmed);
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const record = parsed as Record<string, unknown>;
      for (const key of ["content", "summary", "result", "final_message", "message", "findings", "next_actions"]) {
        const value = record[key];
        if (typeof value === "string" && value.trim()) {
          return normalizeWorkflowUiLanguage(value)
            .split(/\n+/)
            .map((line) => line.trim())
            .filter(Boolean)
            .slice(0, 4)
            .join("\n");
        }
      }
    }
  } catch {
    // Not JSON after all; keep the provider text.
  }
  return normalizeWorkflowUiLanguage(trimmed);
}

function normalizeWorkflowUiLanguage(value: string): string {
  return value
    .replace(/\bcompiled workflow runner\b/gi, "workflow runner")
    .replace(/\bcompiled workflow\b/gi, "workflow plan")
    .replace(/\breadable workflow steps\b/gi, "readable run stages")
    .replace(/\bworkflow steps\b/gi, "run stages")
    .replace(/\bFirst workflow step\b/gi, "First run stage");
}

/** One step card: status + "ran by" (via session) + output + turn drill-in. */
function StepCard({
  step,
  phase,
  sessions,
  model,
  apiUrl,
  run,
  onSelectionChange,
}: {
  step: WorkflowStep;
  phase: WorkflowPhase;
  sessions: ProviderSession[];
  model: WorkbenchModel;
  apiUrl?: string;
  run: WorkflowRun;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}) {
  const [drawerOpen, setDrawerOpen] = useState(false);
  const tone = workflowStepTone(step.status);
  const running = tone === "running";
  const session = step.provider_session_id
    ? sessions.find((s) => s.id === step.provider_session_id)
    : undefined;
  // Once the run is terminal, drill-ins backfill from the durable per-session
  // NDJSON (GET /v1/sessions/{id}/events). A `--trace live` run reports
  // retained:false there, so TurnDrillIn renders "trace not retained" instead of
  // an endless "loading…". In-flight runs keep the live tee + SSE path.
  const historical = isTerminal(run.status);
  // The step actor is a PROVIDER that ran in a one-shot ephemeral worker
  // (codex/claude), carried on the structured result — not a pre-existing
  // member. `isolation` is set when the node opted into a throwaway worktree.
  const provider = step.result?.provider ?? undefined;
  const isolation = step.result?.isolation ?? undefined;
  const roleHint = roleHintFromLabel(step.label);
  const isRequired = phase.kind === "serial" && phase.steps[0]?.id === step.id;
  const isToleratedFail = phase.kind === "parallel" && tone === "bad";
  // The SSE-pushed NORMALIZED live buffer for this node's session, keyed by
  // session id — threaded into TurnDrillIn so the node detail streams sub-second.
  const liveNormalizedEvents = session
    ? model.snapshot.live_normalized_events?.[session.id]
    : undefined;
  const readableOutput = readableWorkflowOutput(step.output_summary);

  return (
    <>
      <div
        id={workflowStepDomId(step.label)}
        className="scroll-mt-20 rounded-lg border border-border bg-card transition-colors hover:border-input"
      >
        {/* The whole card body (lines 1–3) is the click target that opens the
            node drill-in drawer; line 4 keeps the inline TurnDrillIn so the
            timeline still streams in place. */}
        <button
          type="button"
          onClick={() => session && setDrawerOpen(true)}
          disabled={!session}
          className={cn(
            "block w-full p-3 text-left",
            session ? "cursor-pointer" : "cursor-default",
          )}
          aria-label={session ? `Open drill-in for ${step.label}` : undefined}
        >
          {/* Line 1 — workflow action, role hint, status */}
          <div className="flex items-start justify-between gap-2">
            <span className="flex min-w-0 items-start gap-2">
              <StatusDot tone={tone} pulse={running} />
              <span className="min-w-0">
                <span className="block text-[13px] font-medium leading-snug text-foreground">
                  {workflowStepEventTitle(step)}
                </span>
                {roleHint && (
                  <span className="mt-0.5 block text-[11px] text-muted-foreground">{roleHint}</span>
                )}
              </span>
            </span>
            <span className="flex shrink-0 items-center gap-1.5">
              <Badge tone={tone}>{shortStepStatusLabel(step.status)}</Badge>
              {isRequired && <Badge tone="info">required</Badge>}
              {isToleratedFail && <Badge tone="warn">tolerated</Badge>}
            </span>
          </div>

          {/* Line 2 — owner/runtime + timing */}
          <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
            <span>Owner</span>
            {provider ? (
              <span className="inline-flex items-center gap-1 text-foreground">
                <Avatar name={provider} tone="idle" />
                {provider}
              </span>
            ) : (
              <span>—</span>
            )}
            <span className="tabular-nums">{stepTiming(step)}</span>
          </div>

          {/* Line 3 — latest readable result */}
          <div className="mt-2 rounded-md bg-muted/20 px-2 py-1.5 text-[12px] text-foreground">
            <div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Latest result
            </div>
            {readableOutput ? (
              <div className="leading-relaxed">
                <Markdown source={workflowStepEventDetail(readableOutput)} />
              </div>
            ) : running ? (
              <span className="text-muted-foreground">Running…</span>
            ) : tone === "bad" ? (
              <span className="text-muted-foreground">No output (step failed before delivery)</span>
            ) : (
              <span className="text-muted-foreground">No output</span>
            )}
          </div>
        </button>

        {/* Line 4 — drill-in (verbatim TurnDrillIn) or disabled stub. Live events
            threaded so it streams sub-second; a "drill in" affordance opens the
            full drawer. */}
        <div className="flex items-center justify-between gap-2 px-3 pb-3">
          {session ? (
            <>
              <TurnDrillIn session={session} apiUrl={apiUrl} liveNormalizedEvents={liveNormalizedEvents} historical={historical} />
              <button
                type="button"
                onClick={() => setDrawerOpen(true)}
                className="inline-flex shrink-0 items-center gap-1 text-[10px] text-muted-foreground transition-colors hover:text-foreground"
              >
                drill in
                <ChevronRight className="size-3" />
              </button>
            </>
          ) : (
            <span className="inline-flex cursor-not-allowed items-center gap-1 text-[10px] text-muted-foreground">
              <ChevronRight className="size-3 opacity-40" />
              no turn yet
            </span>
          )}
        </div>
      </div>

      {drawerOpen && session && (
        <StepDrawer
          step={step}
          session={session}
          tone={tone}
          provider={provider}
          isolation={isolation}
          liveNormalizedEvents={liveNormalizedEvents}
          apiUrl={apiUrl}
          historical={historical}
          onClose={() => setDrawerOpen(false)}
        />
      )}
    </>
  );
}

/**
 * Per-node drill-in drawer: a right-side slide-over (mirrors the TaskSheet
 * idiom) that wraps the verbatim `TurnDrillIn` for this step's
 * `provider_session_id`, opened auto-expanded and fed the SSE live buffer so the
 * node's streamed tool_use/tool_result render sub-second. Esc and backdrop
 * close it.
 */
function StepDrawer({
  step,
  session,
  tone,
  provider,
  isolation,
  liveNormalizedEvents,
  apiUrl,
  historical,
  onClose,
}: {
  step: WorkflowStep;
  session: ProviderSession;
  tone: StatusTone;
  provider?: string;
  isolation?: string | null;
  liveNormalizedEvents?: HarnessTurnEvent[];
  apiUrl?: string;
  historical?: boolean;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const running = tone === "running";
  const readableOutput = readableWorkflowOutput(step.output_summary);

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      <button
        type="button"
        aria-label="Close node detail"
        className="absolute inset-0 bg-foreground/20 backdrop-blur-[1px]"
        onClick={onClose}
      />
      <aside
        role="dialog"
        aria-label="Workflow node detail"
        className="relative flex h-full w-full max-w-[660px] flex-col border-l border-border bg-background shadow-xl"
      >
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
          <Terminal className="size-3.5 text-muted-foreground" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            Node
          </span>
          <span className="min-w-0 truncate text-[13px] font-medium text-foreground">
            {step.label}
          </span>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="ml-auto grid size-8 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4">
          <div className="space-y-3">
            <div className="flex flex-wrap items-center gap-1.5">
              <StatusDot tone={tone} pulse={running} />
              <Badge tone={tone}>{step.status}</Badge>
              {isolation === "worktree" && <Badge tone="info">worktree</Badge>}
              <MonoId>{session.id}</MonoId>
            </div>
            <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
              <span>ran by</span>
              {provider ? (
                <span className="inline-flex items-center gap-1 text-foreground">
                  <Avatar name={provider} tone="idle" />
                  {provider} (ephemeral)
                </span>
              ) : (
                <span>—</span>
              )}
              <span className="tabular-nums">{stepTiming(step)}</span>
            </div>
            {readableOutput && (
              <div className="rounded-md border border-border bg-muted/30 p-2 text-[12px] text-foreground">
                <Markdown source={readableOutput} />
              </div>
            )}
            <StepObservability step={step} />
            <DocSection label="Turn">
              <TurnDrillIn
                session={session}
                apiUrl={apiUrl}
                defaultOpen
                liveNormalizedEvents={liveNormalizedEvents}
                historical={historical}
              />
            </DocSection>
          </div>
        </div>
      </aside>
    </div>
  );
}

/**
 * Per-step observability panel: the model/exit/duration/token metadata the
 * runtime captures onto `step.result` (see `build_step_details` in harness-cli),
 * plus a structured failure callout and a collapsible worktree diff for
 * isolated steps. Renders nothing when no observability fields are present (e.g.
 * a still-queued step or an older run with a bare result).
 */
function StepObservability({ step }: { step: WorkflowStep }) {
  const result = step.result;
  if (!result) return null;

  const { model, exit_code, duration_ms, tokens, cost_usd, failure } = result;
  const meta: { label: string; value: ReactNode }[] = [];
  if (model) meta.push({ label: "Model", value: <MonoId>{model}</MonoId> });
  if (duration_ms != null) {
    meta.push({ label: "Duration", value: formatMillis(duration_ms) });
  }
  if (typeof cost_usd === "number") {
    meta.push({
      label: "Cost",
      value: (
        <span className="tabular-nums">
          ${cost_usd < 0.01 ? cost_usd.toFixed(4) : cost_usd.toFixed(2)}
        </span>
      ),
    });
  }
  if (exit_code != null) {
    meta.push({
      label: "Exit code",
      value: (
        <Badge tone={exit_code === 0 ? "good" : "bad"}>
          <span className="tabular-nums">{exit_code}</span>
        </Badge>
      ),
    });
  }
  if (tokens) {
    meta.push({
      label: "Tokens",
      value: (
        <span className="tabular-nums">
          {formatCount(tokens.total)} total
          <span className="text-muted-foreground">
            {" "}
            · {formatCount(tokens.input)} in · {formatCount(tokens.output)} out
          </span>
        </span>
      ),
    });
  }

  const hasDiff = Boolean(result.worktree_diff);
  if (meta.length === 0 && !failure && !hasDiff) return null;

  return (
    <DocSection label="Observability">
      {meta.length > 0 && <DocProperties items={meta} />}
      {failure?.failed && (
        <div className="space-y-1.5 rounded-md border border-status-bad/30 bg-status-bad/10 p-2.5">
          <div className="flex items-center gap-1.5">
            <Badge tone="bad">{failure.reason}</Badge>
            <span className="text-[11px] font-medium uppercase tracking-wide text-status-bad">
              failed
            </span>
          </div>
          {failure.detail && (
            <pre className="overflow-x-auto whitespace-pre-wrap break-words font-mono text-[11px] text-foreground">
              {failure.detail}
            </pre>
          )}
        </div>
      )}
      {hasDiff && (
        <WorktreeDiff
          diff={result.worktree_diff ?? ""}
          truncated={Boolean(result.worktree_diff_truncated)}
        />
      )}
    </DocSection>
  );
}

/** Collapsible monospace worktree diff for an `isolation: "worktree"` step. */
function WorktreeDiff({ diff, truncated }: { diff: string; truncated: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="rounded-md border border-border">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-1.5 px-2.5 py-2 text-left text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground"
      >
        {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        worktree diff
        {truncated && <Badge tone="warn">truncated</Badge>}
      </button>
      {open && (
        <pre className="max-h-96 overflow-auto border-t border-border bg-muted/30 p-2.5 font-mono text-[11px] leading-relaxed text-foreground">
          {diff}
        </pre>
      )}
    </div>
  );
}

/** "1.2s" / "850ms" / "2m 05s" from a raw millisecond count. */
function formatMillis(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const totalSeconds = ms / 1000;
  if (totalSeconds < 60) return `${totalSeconds.toFixed(1)}s`;
  const mins = Math.floor(totalSeconds / 60);
  const secs = Math.round(totalSeconds % 60);
  return `${mins}m ${String(secs).padStart(2, "0")}s`;
}

/** Compact token count: "1,234" up to 9999, then "12.3k". */
function formatCount(n: number): string {
  if (n < 10000) return n.toLocaleString();
  return `${(n / 1000).toFixed(1)}k`;
}

/* ================================================================== */
/* Definition (ASCII graph + lazy Rust source)                         */
/* ================================================================== */

function Definition({
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
function AsciiGraph({ phases }: { phases: WorkflowPhase[] }) {
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
function CollapsibleRow({
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
function CompactTimestamp({ value }: { value: string }) {
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

function Timestamp({ value }: { value: string }) {
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

function isTerminal(status: string): boolean {
  const s = status.toLowerCase();
  return s === "completed" || s === "failed" || s === "cached";
}

function statusGloss(status: string): string {
  const s = status.toLowerCase();
  if (s === "completed" || s === "cached") return "ok";
  if (s === "failed") return "failed";
  return s;
}

/** A plain-English gloss of the gate logic for the verdict card (§3). */
function verdictGloss(run: WorkflowRun, steps: WorkflowStep[]): string {
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

function stepTiming(step: WorkflowStep): string {
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
function roleHintFromLabel(label: string): string | undefined {
  const lower = label.toLowerCase();
  if (lower.includes("codex")) return "codex";
  if (lower.includes("claude")) return "claude";
  if (lower.includes("kimi")) return "kimi";
  return undefined;
}

/** The schematic (declared) shape for a registered def, when known. */
function schematicPhasesFor(name: string): WorkflowPhase[] | undefined {
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
function orderForGlyph(steps: WorkflowStep[]): WorkflowStep[] {
  return steps;
}
