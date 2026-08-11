import { CircleDot, CircleSlash, Clock3, Eye, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import type { TeamPressureSummary } from "../../../model/roleViews";

const capacityFacts = (summary: TeamPressureSummary) => [
  { id: "active-turns", label: "Active turns", value: summary.active_turns, icon: CircleDot },
  { id: "ready-members", label: "Ready members", value: summary.ready_members, total: summary.total_members, icon: Users },
  { id: "queued-works", label: "Ready Work", value: summary.ready_work, icon: Clock3 },
  { id: "needs-review", label: "Needs review", value: summary.review_work, icon: Eye },
  { id: "blocked", label: "Blocked", value: summary.blocked_work, icon: CircleSlash },
] as const;

/** Renders the server-built pressure projection without recalculating Team state. */
export function TeamCapacityStrip({ summary, className }: { summary: TeamPressureSummary; className?: string }) {
  return (
    <section aria-label="Team capacity and pressure" data-testid="team-capacity-strip" className={cn("grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-border bg-border sm:grid-cols-5", className)}>
      {capacityFacts(summary).map((fact) => {
        const Icon = fact.icon;
        const pressured = (fact.id === "blocked" || fact.id === "needs-review") && fact.value > 0;
        return (
          <div key={fact.id} data-capacity-tile={fact.id} className="min-w-0 bg-card px-2.5 py-2.5 last:col-span-2 sm:px-3 sm:last:col-span-1">
            <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[.11em] text-muted-foreground"><Icon className={cn("size-3.5 shrink-0", pressured && "text-status-warn")} /><span className="truncate">{fact.label}</span></span>
            <div className="mt-1 flex items-baseline gap-1"><strong className={cn("text-xl font-semibold tabular-nums", pressured && "text-status-warn")}>{fact.value}</strong>{"total" in fact && <span className="text-[10px] text-muted-foreground">of {fact.total}</span>}</div>
          </div>
        );
      })}
    </section>
  );
}
