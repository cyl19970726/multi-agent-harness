import { Activity, Terminal, Workflow } from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { workflowRunTone } from "@/components/workbench/tones";
import { formatDuration } from "@/model/readModel";
import {
  countWorkflowStepStatuses,
  plannedStepCount,
  workflowRunIsLive,
  workflowRunProgress,
  workflowVerdictStep,
} from "@/model/workflowSelectors";
import type { PhaseDagLayer } from "@/model/readModel";
import type { WorkflowRun, WorkflowStep } from "@/types";

export function WorkflowDefinitionPreview({
  script,
  steps = [],
  stepHref,
  showSource = false,
  showPlanSummary = false,
  sourceLabel,
  heading = "Workflow plan",
  collapseExtraStepsOnMobile = false,
  className,
}: {
  script: string;
  steps?: WorkflowStep[];
  stepHref?: (step: WorkflowStep) => string | undefined;
  showSource?: boolean;
  showPlanSummary?: boolean;
  sourceLabel?: string;
  heading?: string;
  collapseExtraStepsOnMobile?: boolean;
  className?: string;
}) {
  const plan = workflowPlanFromScript(script);
  return (
    <div className={cn("min-w-0 overflow-hidden rounded-md border border-border bg-card/70", className)}>
      <div className="flex flex-wrap items-center gap-2 border-b border-border bg-muted/20 px-3 py-2">
        <span className="inline-flex items-center gap-1.5 text-[11px] font-semibold text-muted-foreground">
          <Workflow className="size-3" />
          {heading}
        </span>
        {sourceLabel && <span className="text-[10px] text-muted-foreground">{sourceLabel}</span>}
      </div>
      <div className="space-y-3 px-3 py-2.5">
        {showPlanSummary && (plan.summary || plan.successCriterion) && (
          <div className="grid gap-2 text-[12px] sm:grid-cols-2">
            {plan.summary && (
              <WorkflowPlanFact label="Objective" value={plan.summary} />
            )}
            {plan.successCriterion && (
              <WorkflowPlanFact label="Acceptance target" value={plan.successCriterion} />
            )}
          </div>
        )}
        {plan.steps.length > 0 ? (
          <div className="space-y-2">
	            <div className="grid grid-cols-3 gap-1.5 text-[12px]">
              <WorkflowPlanFact
                label="Workflow plan"
                value={`${plan.steps.length} stage${plan.steps.length === 1 ? "" : "s"}`}
              />
              <WorkflowPlanFact
                label="Owners"
                value={workflowPlanOwners(plan.steps)}
              />
              <WorkflowPlanFact
                label="Review gate"
                value={plan.steps.some((step) => step.kind === "gate") ? "included" : "not declared"}
              />
            </div>
            <div className="flex items-center justify-between gap-2 text-[10px] font-medium text-muted-foreground">
              <span className="inline-flex items-center gap-1.5 uppercase tracking-wider">
                <Activity className="size-3" />
                Run stages
              </span>
              <span>{plan.steps.length} stage{plan.steps.length === 1 ? "" : "s"}</span>
            </div>
            <div className="space-y-1.5">
              {plan.steps.slice(0, 6).map((step, index) => {
              const runtimeStep = matchRuntimeStep(step, steps, index);
              const tone: StatusTone = runtimeStep
                ? workflowStepTone(runtimeStep.status)
                : step.kind === "gate"
                  ? "decision"
                  : "idle";
              const href = runtimeStep ? stepHref?.(runtimeStep) : undefined;
              const owner = step.kind === "gate" ? "Review gate" : "Agent workflow";
              const expected = step.expectedOutput ?? defaultExpectedOutput(step.kind);
              const pass = step.passCondition ?? defaultPassCondition(step.kind);
              const status = runtimeStep ? workflowStepStatusLabel(runtimeStep.status) : "not started";
              const latestResult = runtimeStep?.output_summary ? readableWorkflowText(runtimeStep.output_summary) : undefined;
              const rowSummary = runtimeStep ? workflowStepNarrative(runtimeStep, expected, latestResult, step.kind) : `Expected evidence: ${expected}.`;
              const nextAction = runtimeStep ? workflowStepNextAction(runtimeStep.status) : "Run this stage when prerequisites are ready";
              const dependency = index === 0 ? "none" : "previous stage";
              const output = latestResult ? "recorded" : expected;
              const elapsed = runtimeStep ? formatDuration(runtimeStep.started_at, runtimeStep.ended_at) : undefined;
              return (
                <article
                  key={`${step.label ?? step.title}-${index}`}
	                  className={cn(
	                    "min-w-0 rounded-md border border-border/70 bg-background/40 px-2.5 py-2 transition-colors hover:bg-muted/20",
	                    collapseExtraStepsOnMobile && index > 0 && "max-sm:hidden",
	                    collapseExtraStepsOnMobile && "max-sm:px-2 max-sm:py-1.5",
	                  )}
	                >
	                  <div className="min-w-0">
	                    <div className="flex min-w-0 flex-wrap items-start justify-between gap-2">
	                      <span className="min-w-0">
	                        <span className="flex min-w-0 items-center gap-1.5">
	                          <StatusDot tone={tone} className="shrink-0" pulse={tone === "running"} />
                              <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[10px] font-semibold text-muted-foreground">
                                Stage {index + 1}
                              </span>
		                          <span className="min-w-0 text-[12px] font-semibold leading-snug text-foreground max-sm:line-clamp-1">
		                            {step.title}
		                          </span>
                        </span>
	                        <span className={cn(
	                          "mt-0.5 line-clamp-2 block text-[11px] leading-snug text-muted-foreground",
	                          collapseExtraStepsOnMobile && "max-sm:hidden",
	                        )}>
	                          {rowSummary}
	                        </span>
                      </span>
                      <span className="inline-flex shrink-0 items-center gap-1 rounded-md bg-muted/70 px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                        <StatusDot tone={tone} />
                        {status}
                      </span>
                    </div>
		                    <div className={cn(
		                      "mt-1.5 grid gap-1 text-[11px] text-muted-foreground sm:grid-cols-2 2xl:grid-cols-4",
		                      collapseExtraStepsOnMobile && "max-sm:hidden",
		                    )}>
		                      <WorkflowStepVisibleFact label="Runner / reviewer" value={owner} />
	                      <WorkflowStepVisibleFact label="Dependency" value={dependency} />
	                      <WorkflowStepVisibleFact label="Acceptance check" value={pass} />
	                      <WorkflowStepVisibleFact label="Evidence / output" value={output} />
	                      <WorkflowStepVisibleFact label="Next action" value={nextAction} />
	                      {elapsed && <WorkflowStepVisibleFact label="Duration" value={elapsed} />}
	                    </div>
                    <div className={cn("mt-1", collapseExtraStepsOnMobile && "max-sm:hidden")}>
                      <WorkflowStepDetails
                        owner={owner}
                        expected={expected}
                        pass={pass}
                        latestResult={latestResult}
                        actionHref={href}
                      />
                    </div>
                  </div>
                </article>
              );
            })}
            </div>
          </div>
        ) : (
          <div className="rounded-md border border-dashed border-border bg-muted/20 px-2 py-2">
            {sourceLabel && (
              <p className="mb-1 truncate font-mono text-[10px] text-foreground/75">
                {sourceLabel}
              </p>
            )}
            <p className="text-[12px] text-muted-foreground">
              Direct workflow recorded no agent leaf steps; use the run verdict and final output as the execution result.
            </p>
          </div>
        )}
        {plan.steps.length > 6 && (
          <p className="text-[10px] text-muted-foreground">
            +{plan.steps.length - 6} more steps in source
          </p>
        )}
        {collapseExtraStepsOnMobile && plan.steps.length > 1 && (
          <p className="text-[10px] text-muted-foreground sm:hidden">
            +{plan.steps.length - 1} more run stages
          </p>
        )}
        {showSource && (
          <details className="group">
            <summary className="flex cursor-pointer list-none items-center gap-1.5 text-[10px] font-medium text-muted-foreground/80 transition-colors hover:text-foreground">
              <Terminal className="size-3" />
              Source
              <span className="text-muted-foreground/70">({script.trim().split("\n").length} lines)</span>
            </summary>
            <pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-muted/30 px-3 py-2 font-mono text-[10px] leading-relaxed text-foreground/80">
              {script}
            </pre>
          </details>
        )}
      </div>
    </div>
  );
}

function WorkflowStepDetails({
  owner,
  expected,
  pass,
  latestResult,
  actionHref,
}: {
  owner: string;
  expected: string;
  pass: string;
  latestResult?: string;
  actionHref?: string;
}) {
  return (
    <details className="group">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 text-[10px] font-medium text-muted-foreground transition-colors hover:text-foreground">
        Stage details
      </summary>
      <div className="mt-1 grid gap-1 rounded-md bg-muted/20 p-2 sm:grid-cols-2">
        <WorkflowStepMiniFact label="Owner / reviewer" value={owner} />
        <WorkflowStepMiniFact label="Expected evidence" value={expected} />
        <WorkflowStepMiniFact label="Acceptance" value={pass} />
        {latestResult && <WorkflowStepMiniFact label="Latest output" value={latestResult} />}
      </div>
      {actionHref && (
        <a className="mt-1 inline-flex text-[10px] font-medium text-primary hover:underline" href={actionHref}>
          Open evidence
        </a>
      )}
    </details>
  );
}

function WorkflowStepVisibleFact({ label, value }: { label: string; value: string }) {
  return (
    <span className="min-w-0 rounded bg-muted/20 px-1.5 py-1">
      <span className="block text-[9px] font-medium uppercase tracking-wider text-muted-foreground/80">
        {label}
      </span>
      <span className="mt-0.5 block line-clamp-2 text-[11px] leading-snug text-foreground/75">
        {value}
      </span>
    </span>
  );
}

function WorkflowStepMiniFact({ label, value }: { label: string; value: string }) {
  return (
    <span className="min-w-0">
      <span className="block text-[10px] font-medium text-muted-foreground">
        {label}
      </span>
      <span className="mt-0.5 block text-[11px] leading-snug text-foreground/80">
        {value}
      </span>
    </span>
  );
}

function WorkflowPlanFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-md bg-muted/20 px-2 py-1.5">
      <div className="text-[10px] font-medium text-muted-foreground">{label}</div>
      <p className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-foreground/85">{value}</p>
    </div>
  );
}

function workflowPlanOwners(steps: ReadableWorkflowStep[]): string {
  const owners = new Set(steps.map((step) => step.kind === "gate" ? "review gate" : "agent workflow"));
  return Array.from(owners).join(", ");
}

function matchRuntimeStep(
  planStep: ReadableWorkflowStep,
  runtimeSteps: WorkflowStep[],
  index: number,
): WorkflowStep | undefined {
  if (!runtimeSteps.length) return undefined;
  const label = planStep.label?.trim();
  if (label) {
    const exact = runtimeSteps.find((step) => step.label === label);
    if (exact) return exact;
    const normalized = normalizeWorkflowLabel(label);
    const fuzzy = runtimeSteps.find((step) => normalizeWorkflowLabel(step.label) === normalized);
    if (fuzzy) return fuzzy;
  }
  return runtimeSteps[index];
}

function normalizeWorkflowLabel(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

function workflowStepStatusLabel(status: string): string {
  const value = status.toLowerCase();
  if (value === "completed" || value === "cached") return "passed";
  if (value === "failed") return "blocked";
  if (value === "running") return "running";
  if (value === "queued" || value === "planned") return "not started";
  return value || "not started";
}

function workflowStepNarrative(step: WorkflowStep, fallback: string, latestResult?: string, kind?: "agent" | "gate"): string {
  const value = step.status.toLowerCase();
  if ((value === "completed" || value === "cached") && latestResult) return workflowResultPreview(latestResult, fallback, kind);
  if (value === "completed" || value === "cached") return kind === "gate" ? "Review gate recorded." : "Evidence is ready for review.";
  if (value === "failed") return "Acceptance issue found; review evidence before continuing.";
  if (value === "running") return "Running now; latest evidence will appear here.";
  if (value === "queued" || value === "planned") return `Expected evidence: ${fallback}.`;
  return fallback;
}

function workflowStepNextAction(status: string): string {
  const value = status.toLowerCase();
  if (value === "completed" || value === "cached") return "Review evidence or advance the gate";
  if (value === "failed") return "Resolve the acceptance issue";
  if (value === "running") return "Watch live output";
  if (value === "queued" || value === "planned") return "Run when prerequisites are ready";
  return "Inspect this stage";
}

function workflowResultPreview(latestResult: string | undefined, fallback: string, kind?: "agent" | "gate"): string {
  if (!latestResult) return fallback;
  const compact = compactWorkflowText(latestResult, 180);
  const lower = compact.toLowerCase();
  if (kind === "gate") {
    if (lower.includes("ok=false") || lower.includes("not accepted") || lower.includes("failed")) {
      return "Acceptance issue recorded; inspect the failed criterion.";
    }
    return "Review decision recorded; open details for criteria and rationale.";
  }
  if (lower.includes("next_action") || lower.includes("next actions") || lower.includes("findings")) {
    return "Findings and next actions captured for review.";
  }
  return compact;
}

function workflowStepTone(status?: string): StatusTone {
  const value = status?.toLowerCase() ?? "";
  if (value === "completed" || value === "cached") return "good";
  if (value === "failed") return "bad";
  if (value === "running") return "running";
  if (value === "queued" || value === "planned") return "idle";
  return "info";
}

export function workflowStepDomId(label: string): string {
  return `workflow-step-${normalizeWorkflowLabel(label)}`;
}

function workflowRunFinalOutputSummary(run?: WorkflowRun): { hasOutput: boolean; reason?: string; verdictOk?: boolean } {
  const out = run?.final_output;
  if (!out) return { hasOutput: false, reason: run?.summary ?? undefined };
  if (typeof out === "string") {
    return { hasOutput: out.trim().length > 0, reason: readableWorkflowText(out) };
  }
  if (typeof out !== "object") {
    return { hasOutput: true, reason: String(out) };
  }
  const record = out as Record<string, unknown>;
  const verdict = record.verdict;
  const verdictRecord = verdict && typeof verdict === "object" ? verdict as Record<string, unknown> : undefined;
  const verdictOk = typeof verdictRecord?.ok === "boolean" ? verdictRecord.ok : undefined;
  const reason =
    typeof verdictRecord?.reason === "string" && verdictRecord.reason.trim()
      ? verdictRecord.reason
      : typeof record.summary === "string" && record.summary.trim()
        ? record.summary
        : run?.summary ?? undefined;
  return {
    hasOutput: Object.keys(record).length > 0,
    reason: reason ? compactWorkflowText(normalizeWorkflowUiLanguage(reason), 180) : undefined,
    verdictOk,
  };
}

export function WorkflowRunSummary({
  run,
  steps,
  attempts = 0,
  phaseId,
  plannedLayers,
  hasVerdictGate,
  onOpenRun,
  className,
}: {
  run?: WorkflowRun;
  steps: WorkflowStep[];
  attempts?: number;
  phaseId?: string;
  plannedLayers?: PhaseDagLayer[];
  hasVerdictGate?: boolean;
  onOpenRun?: (runId: string) => void;
  className?: string;
}) {
  const stepCounts = countWorkflowStepStatuses(steps);
  const rawProgress = workflowRunProgress(steps);
  const live = workflowRunIsLive(run, steps);
  const finalOutput = workflowRunFinalOutputSummary(run);
  const directWorkflowRun = Boolean(run && steps.length === 0 && (run.final_output != null || run.status === "completed"));
  const directWorkflowRejected = directWorkflowRun && finalOutput.verdictOk === false;
  const directWorkflowAccepted =
    directWorkflowRun && run?.status === "completed" && finalOutput.verdictOk !== false;
  const verdict = workflowVerdictStep(phaseId, steps);
  const currentStep = steps.find((step) => step.status === "running")
    ?? steps.find((step) => step.status === "queued")
    ?? steps.find((step) => step.status === "failed")
    ?? [...steps].reverse().find((step) => step.status);
  const failedSteps = steps.filter((step) => step.status === "failed").length + (directWorkflowRejected ? 1 : 0);
  const finishedSteps = stepCounts.completed + stepCounts.cached + (directWorkflowAccepted ? 1 : 0);
  const plannedSteps = plannedLayers ? plannedStepCount(plannedLayers) : 0;
  const totalSteps = directWorkflowRun ? 1 : steps.length || plannedSteps || finishedSteps;
  const progressPercent = directWorkflowRun
    ? failedSteps > 0 || directWorkflowAccepted
      ? 100
      : 0
    : rawProgress.percent;
  const currentTone = run
    ? run.status === "failed" || failedSteps > 0
      ? "bad"
      : live
        ? "running"
        : run.status === "completed"
          ? "good"
          : workflowRunTone(run.status)
    : "idle";
  const currentTitle = run
    ? run.status === "failed" || failedSteps > 0
      ? "needs review"
      : live
        ? "running"
        : run.status === "completed"
          ? "passed"
          : workflowRunStateLabel(run.status)
    : "not started";
  const currentDetail = run
    ? directWorkflowRun
      ? failedSteps > 0
        ? "Direct workflow finished, but its verdict did not accept the phase."
        : directWorkflowAccepted
          ? "Direct workflow verdict accepted."
          : live
            ? "Direct workflow is running."
            : "Direct workflow run recorded output; review the verdict before acceptance."
      : run.status === "failed" || failedSteps > 0
      ? failedSteps > 0
        ? `${failedSteps} run stage${failedSteps === 1 ? "" : "s"} need evidence review.`
        : `${finishedSteps}/${totalSteps || finishedSteps} run stages completed; review verdict still pending.`
      : live
        ? `${titleFromLabel(currentStep?.label ?? "run stage")} is running.`
        : `${finishedSteps}/${totalSteps || finishedSteps} run stages passed.`
    : `${totalSteps} run stage${totalSteps === 1 ? "" : "s"} not started.`;
  const latestResult = currentStep?.output_summary
    ? readableWorkflowText(currentStep.output_summary)
    : directWorkflowRun && finalOutput.reason
      ? finalOutput.reason
      : undefined;

  return (
    <div className={cn("min-w-0 space-y-3", className)}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 text-[11px] font-semibold text-muted-foreground">
          <Activity className={cn("size-3", live && "text-status-running")} />
          Live execution
        </span>
        <span className="inline-flex items-center gap-1.5 rounded-md bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
          <StatusDot tone={run ? workflowRunTone(run.status) : "idle"} pulse={live} />
          {run ? workflowRunStateLabel(run.status) : "not started"}
        </span>
      </div>
      {run ? (
        <div className="space-y-2">
          <WorkflowCurrentStateCard
            title={currentTitle}
            detail={currentDetail}
            latestResult={latestResult}
            tone={currentTone}
          />
          <div className="space-y-1">
            <div className="flex items-center justify-between text-[10px] font-medium text-muted-foreground">
              <span>Live execution progress</span>
              <span>{finishedSteps}/{totalSteps || finishedSteps}</span>
            </div>
            <div className="h-1 overflow-hidden rounded-full bg-muted">
              <div
                className={cn("h-full rounded-full", failedSteps > 0 ? "bg-status-bad/65" : "bg-status-good")}
                style={{ width: `${progressPercent}%` }}
              />
            </div>
          </div>
          {verdict && (
            <p className="line-clamp-2 text-[11px] leading-snug text-muted-foreground">
              Review outcome: <span className="font-medium text-foreground/80">{workflowStepStatusLabel(verdict.status)}</span>
              {verdict.output_summary ? ` - ${readableWorkflowText(verdict.output_summary)}` : ""}
            </p>
          )}
          <p className="text-[11px] text-muted-foreground">
            {formatDuration(run.created_at, run.ended_at) ?? "running"}{attempts > 1 ? ` · ${attempts} attempts` : ""}
          </p>
          {onOpenRun && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="h-8 w-full justify-center gap-1.5 text-[12px]"
              onClick={() => onOpenRun(run.id)}
            >
              <Workflow className="size-3.5" />
              Open live execution
            </Button>
          )}
        </div>
      ) : (
        <div className="space-y-2">
          <WorkflowCurrentStateCard
            title={currentTitle}
            detail={hasVerdictGate ? `${currentDetail} Review will be required before acceptance.` : currentDetail}
            tone="idle"
          />
        </div>
      )}
    </div>
  );
}

function WorkflowCurrentStateCard({
  title,
  detail,
  latestResult,
  tone,
}: {
  title: string;
  detail: string;
  latestResult?: string;
  tone: StatusTone;
}) {
  return (
    <div className="rounded-md border border-border/70 bg-background/50 px-2.5 py-2">
      <div className="flex items-center gap-1.5 text-[12px] font-semibold text-foreground">
        <StatusDot tone={tone} pulse={tone === "running"} />
        {title}
      </div>
      <p className="mt-1 text-[12px] leading-relaxed text-foreground/80">{detail}</p>
      {latestResult && (
        <p className="mt-1 line-clamp-2 text-[11px] leading-snug text-muted-foreground">
          Latest result: {latestResult}
        </p>
      )}
    </div>
  );
}

function workflowRunStateLabel(status: string): string {
  if (status === "failed") return "needs review";
  if (status === "completed") return "passed";
  if (status === "running") return "running";
  if (status === "queued") return "not started";
  return status || "not started";
}

type ReadableWorkflowStep = {
  title: string;
  label?: string;
  provider?: string;
  kind?: "agent" | "gate";
  writable?: boolean;
  expectedOutput?: string;
  passCondition?: string;
};

function workflowPlanFromScript(script: string): { summary?: string; successCriterion?: string; steps: ReadableWorkflowStep[] } {
  const summary = parseWorkflowSummary(script);
  const successCriterion = namedKeywordValue(script, "success_criterion");
  const steps: ReadableWorkflowStep[] = [...configObjectSteps(script)];
  const agentPattern = /agent\(([^)]*)\)/g;
  let match: RegExpExecArray | null;
  while ((match = agentPattern.exec(script)) != null) {
    const call = match[1] ?? "";
    const firstArg = firstQuotedValue(call);
    const label = namedQuotedValue(call, "label");
    if (!firstArg && !label) continue;
    if (label && steps.some((step) => step.label === label)) continue;
    const provider = namedQuotedValue(call, "provider");
    const title = cleanStepTitle(firstArg ?? label ?? "Workflow step", label);
    steps.push({
      title,
      label,
      provider,
      kind: /verdict/i.test(label ?? call) ? "gate" : "agent",
      writable: /writable\s*=\s*True|isolation\s*=/.test(call),
      expectedOutput: /verdict/i.test(label ?? call) ? "Acceptance decision with reason" : defaultExpectedOutput("agent"),
      passCondition: /verdict/i.test(label ?? call) ? "Decision is clear enough to act on" : defaultPassCondition("agent"),
    });
  }
  const looseLabelPattern = /label\s*=\s*(["'])((?:\\.|(?!\1).)*)\1/g;
  while ((match = looseLabelPattern.exec(script)) != null) {
    const label = match[2] ? unescapeQuoted(match[2]).trim() : undefined;
    if (!label || steps.some((step) => step.label === label)) continue;
    steps.push({
      title: titleFromLabel(label),
      label,
      kind: /verdict/i.test(label) ? "gate" : "agent",
      expectedOutput: /verdict/i.test(label) ? "Acceptance decision with reason" : defaultExpectedOutput("agent"),
      passCondition: /verdict/i.test(label) ? "Decision is clear enough to act on" : defaultPassCondition("agent"),
    });
  }
  return { summary, successCriterion, steps };
}

function configObjectSteps(script: string): ReadableWorkflowStep[] {
  const steps: ReadableWorkflowStep[] = [];
  const objectPattern = /^ {4}\{([\s\S]*?)^ {4}\},?/gm;
  let match: RegExpExecArray | null;
  while ((match = objectPattern.exec(script)) != null) {
    const block = match[1] ?? "";
    const label = namedObjectValue(block, "label");
    if (!label) continue;
    const provider = namedObjectValue(block, "provider");
    const prompt = tripleQuotedObjectValue(block, "prompt");
    steps.push({
      title: titleFromPrompt(prompt) ?? titleFromLabel(label),
      label,
      provider,
      kind: "agent",
      writable: block.includes('"writable"') || block.includes("'writable'") || block.includes("isolation"),
      expectedOutput: expectedOutputFromPrompt(prompt) ?? defaultExpectedOutput("agent"),
      passCondition: defaultPassCondition("agent"),
    });
  }
  return steps;
}

function parseWorkflowSummary(script: string): string | undefined {
  const match = script.match(/workflow\(\s*(["'])(?:\\.|(?!\1).)*\1\s*,\s*(["'])((?:\\.|(?!\2).)*)\2/);
  return match?.[3] ? unescapeQuoted(match[3]).trim() : undefined;
}

function firstQuotedValue(value: string): string | undefined {
  const match = value.match(/(["'])((?:\\.|(?!\1).)*)\1/);
  return match?.[2] ? unescapeQuoted(match[2]).trim() : undefined;
}

function namedQuotedValue(value: string, name: string): string | undefined {
  const pattern = new RegExp(`${name}\\s*=\\s*([\"'])((?:\\\\.|(?!\\1).)*)\\1`);
  const match = value.match(pattern);
  return match?.[2] ? unescapeQuoted(match[2]).trim() : undefined;
}

function namedKeywordValue(value: string, name: string): string | undefined {
  const pattern = new RegExp(`${name}\\s*=\\s*([\"'])((?:\\\\.|(?!\\1).)*)\\1`);
  const match = value.match(pattern);
  return match?.[2] ? unescapeQuoted(match[2]).trim() : undefined;
}

function namedObjectValue(value: string, name: string): string | undefined {
  const pattern = new RegExp(`[\"']${name}[\"']\\s*:\\s*([\"'])((?:\\\\.|(?!\\1).)*)\\1`);
  const match = value.match(pattern);
  return match?.[2] ? unescapeQuoted(match[2]).trim() : undefined;
}

function tripleQuotedObjectValue(value: string, name: string): string | undefined {
  const pattern = new RegExp(`[\"']${name}[\"']\\s*:\\s*\"\"\"([\\s\\S]*?)\"\"\"`);
  const match = value.match(pattern);
  return match?.[1]?.trim();
}

function unescapeQuoted(value: string): string {
  try {
    return JSON.parse(`"${value.replace(/"/g, '\\"')}"`) as string;
  } catch {
    return value.replace(/\\"/g, '"').replace(/\\n/g, " ");
  }
}

function cleanStepTitle(value: string, label?: string): string {
  const withoutLabel = label && value.startsWith(`${label}:`)
    ? value.slice(label.length + 1).trim()
    : value;
  return withoutLabel.replace(/\s+/g, " ").trim() || label || "Workflow step";
}

function titleFromPrompt(prompt?: string): string | undefined {
  const firstLine = prompt
    ?.split("\n")
    .map((line) => line.trim())
    .find(Boolean);
  if (!firstLine) return undefined;
  return firstLine
    .replace(/^You are\s+/i, "")
    .replace(/\.$/, "")
    .replace(/\s+/g, " ");
}

function expectedOutputFromPrompt(prompt?: string): string | undefined {
  const text = prompt?.toLowerCase() ?? "";
  if (!text) return undefined;
  if (text.includes("review")) return "Review findings and recommendation";
  if (text.includes("synthesize")) return "Decision-ready summary";
  if (text.includes("audit")) return "Findings and next actions";
  if (text.includes("implement") || text.includes("change")) return "Changed files and verification";
  return defaultExpectedOutput("agent");
}

function defaultExpectedOutput(kind?: "agent" | "gate"): string {
  return kind === "gate" ? "Review gate with reason" : "Workflow evidence";
}

function defaultPassCondition(kind?: "agent" | "gate"): string {
  return kind === "gate" ? "Decision is clear enough to act on" : "Result is specific enough for review";
}

function titleFromLabel(label: string): string {
  return label
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => (part.toLowerCase() === "ux" ? "UX" : part.charAt(0).toUpperCase() + part.slice(1)))
    .join(" ");
}

function readableWorkflowText(summary: string): string {
  const trimmed = summary.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return normalizeWorkflowUiLanguage(trimmed);
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const record = parsed as Record<string, unknown>;
      if (typeof record.findings === "string" || typeof record.next_actions === "string") {
        return "Findings and next actions captured for review.";
      }
      for (const key of ["content", "summary", "result", "final_message", "message", "findings", "next_actions"]) {
        const value = record[key];
        if (typeof value === "string" && value.trim()) return compactWorkflowText(normalizeWorkflowUiLanguage(value));
      }
    }
  } catch {
    return normalizeWorkflowUiLanguage(trimmed);
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

function compactWorkflowText(value: string, max = 220): string {
  const text = value
    .split(/\n+/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 2)
    .join(" ");
  if (text.length <= max) return text;
  return `${text.slice(0, max - 1).trimEnd()}…`;
}

function WorkflowStatusChip({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone: StatusTone;
}) {
  if (value <= 0) return null;
  return (
    <span className="inline-flex items-center gap-1 rounded-md border border-border bg-card/70 px-2 py-1 text-[10px] text-muted-foreground">
      <StatusDot tone={tone} />
      {label}
      <span className="font-mono">{value}</span>
    </span>
  );
}
