import { Activity, Terminal, Workflow } from "lucide-react";

import { StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { cn } from "@/lib/utils";
import { matchRuntimeSteps, normalizeWorkflowLabel } from "@/model/workflowSelectors";
import type { WorkflowStep } from "@/types";

interface PlanStep { title: string; label?: string; gate: boolean }

export function WorkflowDefinitionPreview({
  script,
  steps = [],
  stepHref,
  showSource = false,
  sourceLabel,
  heading = "Workflow plan",
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
  const plan = parsePlan(script);
  const runtime = matchRuntimeSteps(plan, steps);
  return (
    <div className={cn("min-w-0 overflow-hidden rounded-md border border-border bg-card/70", className)}>
      <div className="flex items-center gap-2 border-b border-border bg-muted/20 px-3 py-2 text-[11px] font-semibold text-muted-foreground">
        <Workflow className="size-3" /> {heading}
        {sourceLabel && <span className="ml-auto truncate font-normal">{sourceLabel}</span>}
      </div>
      <div className="space-y-2 p-3">
        {plan.length ? plan.slice(0, 8).map((item, index) => {
          const actual = runtime[index];
          const tone = actual ? statusTone(actual.status) : item.gate ? "decision" : "idle";
          const href = actual ? stepHref?.(actual) : undefined;
          const content = (
            <>
              <StatusDot tone={tone} pulse={tone === "running"} />
              <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">Stage {index + 1}</span>
              <span className="min-w-0 flex-1 truncate text-xs font-medium">{item.title}</span>
              <span className="text-[10px] text-muted-foreground">{actual?.status ?? "not started"}</span>
            </>
          );
          return href ? <a key={`${item.label}-${index}`} href={href} className="flex items-center gap-2 rounded-md border border-border/70 bg-background/40 px-2.5 py-2 hover:bg-muted/20">{content}</a>
            : <div key={`${item.label}-${index}`} className="flex items-center gap-2 rounded-md border border-border/70 bg-background/40 px-2.5 py-2">{content}</div>;
        }) : (
          <p className="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">This workflow records its result directly and has no declared agent stages.</p>
        )}
        {showSource && <details><summary className="flex cursor-pointer items-center gap-1.5 text-[10px] text-muted-foreground"><Terminal className="size-3" /> Source</summary><pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap rounded-md bg-muted/30 p-3 font-mono text-[10px]">{script}</pre></details>}
        <p className="flex items-center gap-1.5 text-[10px] text-muted-foreground"><Activity className="size-3" /> Runtime stages match declared labels before position.</p>
      </div>
    </div>
  );
}

export function workflowStepDomId(label: string): string {
  return `workflow-step-${normalizeWorkflowLabel(label)}`;
}

function parsePlan(script: string): PlanStep[] {
  const rows: PlanStep[] = [];
  const pattern = /agent\(([\s\S]*?)\)/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(script)) != null) {
    const call = match[1] ?? "";
    const label = namedValue(call, "label");
    const title = firstValue(call) ?? label ?? "Workflow stage";
    rows.push({ title: title.replace(/\s+/g, " ").trim(), label, gate: /verdict|review|gate/i.test(label ?? call) });
  }
  return rows;
}

function firstValue(value: string): string | undefined {
  return value.match(/(["'])((?:\\.|(?!\1).)*)\1/)?.[2];
}

function namedValue(value: string, name: string): string | undefined {
  return value.match(new RegExp(`${name}\\s*=\\s*(["'])((?:\\\\.|(?!\\1).)*)\\1`))?.[2];
}

function statusTone(status: string): StatusTone {
  if (status === "running") return "running";
  if (status === "completed" || status === "cached") return "good";
  if (status === "failed") return "bad";
  return "idle";
}
