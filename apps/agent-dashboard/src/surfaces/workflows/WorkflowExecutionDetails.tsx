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


import { CollapsibleRow, Timestamp, isTerminal, roleHintFromLabel, stepTiming } from "./WorkflowDefinition";

export function Timeline({
  phases,
  model,
  apiUrl,
  projectBindingId,
  executionSpaceId,
  run,
  onSelectionChange,
}: {
  phases: WorkflowPhase[];
  model: WorkbenchModel;
  apiUrl?: string;
  projectBindingId?: string;
  executionSpaceId?: string;
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
                model={model}
                apiUrl={apiUrl}
                projectBindingId={projectBindingId}
                executionSpaceId={executionSpaceId}
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

export function titleCase(value: string): string {
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

export function readableWorkflowOutput(summary?: string | null): string | undefined {
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

export function normalizeWorkflowUiLanguage(value: string): string {
  return value
    .replace(/\bcompiled workflow runner\b/gi, "workflow runner")
    .replace(/\bcompiled workflow\b/gi, "workflow plan")
    .replace(/\breadable workflow steps\b/gi, "readable run stages")
    .replace(/\bworkflow steps\b/gi, "run stages")
    .replace(/\bFirst workflow step\b/gi, "First run stage");
}

/** One step card: status + provider-native binding + explicit Workflow outcome. */
function StepCard({
  step,
  phase,
  model,
  apiUrl,
  run,
  onSelectionChange,
  projectBindingId,
  executionSpaceId,
}: {
  step: WorkflowStep;
  phase: WorkflowPhase;
  model: WorkbenchModel;
  apiUrl?: string;
  run: WorkflowRun;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  projectBindingId?: string;
  executionSpaceId?: string;
}) {
  const [drawerOpen, setDrawerOpen] = useState(false);
  const tone = workflowStepTone(step.status);
  const running = tone === "running";
  const session = step.native_session ?? step.result?.native_session ?? undefined;
  const [nativeActivity, setNativeActivity] = useState<NativeActivityProjection>();
  useEffect(() => {
    setNativeActivity(undefined);
    if (!apiUrl || !session) return;
    let cancelled = false;
    fetchNativeWorkflowStepActivity(
      apiUrl,
      step.id,
      projectBindingId,
      executionSpaceId,
    )
      .then((projection) => { if (!cancelled) setNativeActivity(projection); })
      .catch(() => { /* missing provider history remains an honest empty state */ });
    return () => { cancelled = true; };
  }, [
    apiUrl,
    step.id,
    projectBindingId,
    executionSpaceId,
    session?.native_session_id,
  ]);
  // The step actor is a PROVIDER that ran in a one-shot ephemeral worker
  // (codex/claude), carried on the structured result — not a pre-existing
  // member. `isolation` is set when the node opted into a throwaway worktree.
  const provider = step.result?.provider ?? undefined;
  const isolation = step.result?.isolation ?? undefined;
  const roleHint = roleHintFromLabel(step.label);
  const isRequired = phase.kind === "serial" && phase.steps[0]?.id === step.id;
  const isToleratedFail = phase.kind === "parallel" && tone === "bad";
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

        {/* Line 4 — provider-native activity; never a Harness transcript copy. */}
        <div className="flex items-center justify-between gap-2 px-3 pb-3">
          {session ? (
            <>
              <span className="inline-flex items-center gap-1 text-[10px] text-muted-foreground">
                <Activity className="size-3" />
                {nativeActivity
                  ? `${nativeActivity.items.length} native activit${nativeActivity.items.length === 1 ? "y" : "ies"}`
                  : "provider-native session"}
              </span>
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
          nativeActivity={nativeActivity}
          apiUrl={apiUrl}
          onClose={() => setDrawerOpen(false)}
        />
      )}
    </>
  );
}

/**
 * Per-node drill-in drawer: a right-side slide-over (mirrors the TaskSheet
 * idiom) that joins the durable Workflow outcome with a read-only projection
 * from the provider-owned native session. Esc and backdrop close it.
 */
function StepDrawer({
  step,
  session,
  tone,
  provider,
  isolation,
  nativeActivity,
  apiUrl,
  onClose,
}: {
  step: WorkflowStep;
  session: NativeSessionRef;
  tone: StatusTone;
  provider?: string;
  isolation?: string | null;
  nativeActivity?: NativeActivityProjection;
  apiUrl?: string;
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
              <MonoId>{session.native_session_id}</MonoId>
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
            <DocSection label="Provider-native activity">
              <div className="space-y-2">
                {(nativeActivity?.items ?? []).map((item, index) => (
                  <div key={`${item.occurred_at ?? "native"}-${index}`} className="rounded-md border border-border bg-muted/20 p-2">
                    <div className="flex items-center gap-2 text-[11px]">
                      <Badge tone={item.status === "failed" ? "bad" : item.status === "started" ? "running" : "good"}>{item.kind}</Badge>
                      <span className="font-medium text-foreground">{item.title}</span>
                      {item.occurred_at && <span className="ml-auto tabular-nums text-muted-foreground">{item.occurred_at}</span>}
                    </div>
                    {item.summary && <div className="mt-1 text-[11px] text-muted-foreground">{item.summary}</div>}
                  </div>
                ))}
                {!nativeActivity?.items.length && (
                  <div className="rounded-md border border-dashed border-border p-3 text-[11px] text-muted-foreground">
                    Native activity is unavailable or the provider has not written readable events yet.
                  </div>
                )}
              </div>
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
export function formatMillis(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const totalSeconds = ms / 1000;
  if (totalSeconds < 60) return `${totalSeconds.toFixed(1)}s`;
  const mins = Math.floor(totalSeconds / 60);
  const secs = Math.round(totalSeconds % 60);
  return `${mins}m ${String(secs).padStart(2, "0")}s`;
}

/** Compact token count: "1,234" up to 9999, then "12.3k". */
export function formatCount(n: number): string {
  if (n < 10000) return n.toLocaleString();
  return `${(n / 1000).toFixed(1)}k`;
}

/* ================================================================== */
/* Definition (ASCII graph + lazy Rust source)                         */
/* ================================================================== */

export function workflowStepEventTitle(step: WorkflowStep): string {
  const label = step.label.replace(/[-_]+/g, " ").trim();
  if (step.status === "running") return `${titleCase(label)} is running`;
  if (step.status === "failed") return `${titleCase(label)} needs review`;
  if (step.status === "queued") return `${titleCase(label)} is queued`;
  if (step.status === "completed" || step.status === "cached") return `${titleCase(label)} finished`;
  return titleCase(label);
}


export function workflowStepEventDetail(output: string): string {
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


export function shortStepStatusLabel(status: string): string {
  const value = status.toLowerCase();
  if (value === "completed" || value === "cached") return "passed";
  if (value === "failed") return "blocked";
  if (value === "queued" || value === "planned") return "not started";
  return status;
}
