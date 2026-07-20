import { ChevronRight, Waves } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";
import type { Wave } from "@/types";

export function WaveMicro({ wave, className }: { wave: Wave; className?: string }) {
  const tone = waveTone(wave.status, wave.gate_status);
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground", className)}>
      <StatusDot tone={tone} pulse={tone === "running"} />
      <span className="shrink-0 font-medium">W{wave.index}</span>
      <span className="min-w-0 truncate text-foreground">{wave.title}</span>
    </span>
  );
}

export function WaveCompact({
  wave,
  onOpen,
  className,
}: {
  wave: Wave;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = waveTone(wave.status, wave.gate_status);
  const content = (
    <>
      <div className="flex min-w-0 items-start gap-2.5">
        <span className="grid size-7 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground">
          <Waves className="size-3.5" aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-1.5">
            <StatusDot tone={tone} pulse={tone === "running"} />
            <span className="shrink-0 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Wave {wave.index}</span>
            <Badge tone={tone}>{wave.status ?? "planned"}</Badge>
          </div>
          <p className="mt-1 truncate text-[13px] font-semibold text-foreground">{wave.title}</p>
          <p className="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground">{wave.objective}</p>
        </div>
        {onOpen && <ChevronRight className="mt-1 size-4 shrink-0 text-muted-foreground" />}
      </div>
      <div className="mt-2.5 flex flex-wrap items-center gap-1.5 border-t border-border/60 pt-2">
        <Badge tone="muted">{executorLabel(wave.executor_kind)}</Badge>
        <Badge tone={gateTone(wave.gate_status)}>gate {wave.gate_status ?? "pending"}</Badge>
      </div>
    </>
  );
  const baseClass = cn(
    "block w-full rounded-md border border-border bg-card px-3 py-2.5 text-left transition-colors",
    onOpen && "hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
    className,
  );
  return onOpen ? <button type="button" className={baseClass} onClick={onOpen}>{content}</button> : <div className={baseClass}>{content}</div>;
}

export function WavePanel({
  wave,
  attemptCount,
  acceptedAttemptLabel,
  onOpen,
  className,
}: {
  wave: Wave;
  attemptCount?: number;
  acceptedAttemptLabel?: string;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = waveTone(wave.status, wave.gate_status);
  return (
    <section className={cn("rounded-lg border border-border bg-card p-3.5", className)}>
      <div className="flex min-w-0 items-start gap-3">
        <span className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground"><Waves className="size-4" /></span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <StatusDot tone={tone} pulse={tone === "running"} />
            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Wave {wave.index}</span>
            <Badge tone={tone}>{wave.status ?? "planned"}</Badge>
            <Badge tone={gateTone(wave.gate_status)}>gate {wave.gate_status ?? "pending"}</Badge>
          </div>
          <h3 className="mt-1 text-[14px] font-semibold leading-snug text-foreground">{wave.title}</h3>
        </div>
      </div>
      <p className="mt-3 text-[12px] leading-relaxed text-muted-foreground">{wave.objective}</p>
      <dl className="mt-3 space-y-2 border-t border-border/60 pt-3 text-[11px]">
        <WaveFact label="Executor" value={executorLabel(wave.executor_kind)} />
        <WaveFact label="Exit" value={wave.exit_criteria ?? "Not declared"} />
        <WaveFact label="Attempts" value={attemptCount === undefined ? "—" : String(attemptCount)} />
        {acceptedAttemptLabel && <WaveFact label="Accepted" value={acceptedAttemptLabel} />}
        {wave.outcome_summary && <WaveFact label="Outcome" value={wave.outcome_summary} />}
      </dl>
      {onOpen && (
        <button type="button" onClick={onOpen} className="mt-3 inline-flex items-center gap-1 text-[11px] font-medium text-primary hover:underline">
          Open Wave <ChevronRight className="size-3.5" />
        </button>
      )}
    </section>
  );
}

function WaveFact({ label, value }: { label: string; value: string }) {
  return <div className="grid grid-cols-[4.5rem_1fr] gap-2"><dt className="text-muted-foreground">{label}</dt><dd className="min-w-0 break-words text-foreground">{value}</dd></div>;
}

function executorLabel(value?: string | null): string {
  if (value === "agent_team") return "Agent Team";
  if (value === "dynamic_workflow") return "Dynamic Workflow";
  if (value === "host") return "Host";
  return value ?? "Not selected";
}

function waveTone(status?: string | null, gate?: string | null): StatusTone {
  if (gate === "accepted") return "good";
  if (gate === "blocked" || status === "blocked" || status === "failed") return "bad";
  if (gate === "revise" || status === "waiting") return "warn";
  if (status === "running") return "running";
  if (status === "planned") return "decision";
  return "idle";
}

function gateTone(gate?: string | null): StatusTone {
  if (gate === "accepted") return "good";
  if (gate === "blocked") return "bad";
  if (gate === "revise") return "warn";
  return "idle";
}
