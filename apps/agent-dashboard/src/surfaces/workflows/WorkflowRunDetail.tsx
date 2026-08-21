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


import type { WorkflowSurfaceProps } from "../Workflows";
import {
  Timeline,
  formatCount,
  formatMillis,
  normalizeWorkflowUiLanguage,
  readableWorkflowOutput,
  shortStepStatusLabel,
  titleCase,
  workflowStepEventDetail,
  workflowStepEventTitle,
} from "./WorkflowExecutionDetails";
import {
  CompactTimestamp,
  Definition,
  Timestamp,
  isTerminal,
  roleHintFromLabel,
  statusGloss,
  stepTiming,
  verdictGloss,
} from "./WorkflowDefinition";

export function WorkflowRunDetail({
  model,
  onSelectionChange,
  apiUrl,
  projectBindingId,
  executionSpaceId,
}: WorkflowSurfaceProps) {
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
          <Timeline
            phases={phases}
            model={model}
            apiUrl={apiUrl}
            projectBindingId={projectBindingId}
            executionSpaceId={executionSpaceId}
            run={run}
            onSelectionChange={onSelectionChange}
          />
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
