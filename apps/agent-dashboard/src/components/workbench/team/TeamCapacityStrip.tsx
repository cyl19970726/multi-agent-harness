import { CircleDot, CircleSlash, Clock3, Eye, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import type { TeamPressureSummary } from "../../../model/roleViews";

const capacityFacts = (summary: TeamPressureSummary) => [
  { id: "active-turns", label: "Active turns", mobileLabel: "Turns", value: summary.active_turns, icon: CircleDot },
  { id: "ready-members", label: "Ready members", mobileLabel: "Members", value: summary.ready_members, total: summary.total_members, icon: Users },
  { id: "queued-works", label: "Ready Work", mobileLabel: "Ready", value: summary.ready_work, icon: Clock3 },
  { id: "needs-review", label: "Needs review", mobileLabel: "Review", value: summary.review_work, icon: Eye },
  { id: "blocked", label: "Blocked", mobileLabel: "Blocked", value: summary.blocked_work, icon: CircleSlash },
] as const;

/** Renders the server-built pressure projection without recalculating Team state. */
export function TeamCapacityStrip({ summary, className, compact = false }: { summary: TeamPressureSummary; className?: string; compact?: boolean }) {
  return (
    <section aria-label="Team capacity and pressure" data-testid="team-capacity-strip" className={cn("grid grid-cols-4 sm:grid-cols-5", !compact && "divide-x divide-border border-y border-border", className)}>
      {capacityFacts(summary).map((fact) => {
        const Icon = fact.icon;
        const pressured = (fact.id === "blocked" || fact.id === "needs-review") && fact.value > 0;
        return (
          <div key={fact.id} data-capacity-tile={fact.id} className={cn("min-w-0", compact ? "py-1" : "px-1.5 py-2.5 sm:px-3 sm:py-3",fact.id === "ready-members" && "hidden sm:block")}>
            <span className={cn("flex items-center gap-1 font-semibold uppercase text-muted-foreground", compact ? "justify-start text-[9px] tracking-[.08em]" : "justify-center text-[8px] tracking-[.06em] sm:justify-start sm:gap-1.5 sm:text-[10px] sm:tracking-[.11em]")}><Icon className={cn("size-3 shrink-0", !compact && "sm:size-3.5", pressured && "text-status-warn")} /><span className="sm:hidden">{fact.mobileLabel}</span><span className="hidden sm:inline">{fact.label}</span></span>
            <div className={cn("flex items-baseline gap-1", compact ? "mt-1 justify-start" : "mt-0.5 justify-center sm:mt-1 sm:justify-start")}><strong className={cn(compact ? "text-xs" : "text-base sm:text-xl", "font-semibold tabular-nums", pressured && "text-status-warn")}>{fact.value}</strong>{"total" in fact && <span className="text-[9px] text-muted-foreground">of {fact.total}</span>}</div>
          </div>
        );
      })}
    </section>
  );
}
