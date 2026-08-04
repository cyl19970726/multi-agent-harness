import { CircleDot, CircleSlash, Clock, Eye, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import { StatusDot } from "@/components/workbench/atoms";

import type { TeamCapacityTile } from "../../../model/teamSelectors";

const TILE_ICON = {
  "active-turns": CircleDot,
  "ready-members": Users,
  "queued-works": Clock,
  "needs-review": Eye,
  blocked: CircleSlash,
} as const;

/**
 * Factual Team capacity.
 *
 * Every number is a count of durable MemberRun or Work rows. There is no
 * utilisation percentage and no invented ceiling: a tile shows a denominator
 * only when the store actually holds one, which is why `Active turns` is a
 * bare count while `Ready members` is a ratio of the durable roster.
 */
export function TeamCapacityStrip({ tiles, className }: { tiles: TeamCapacityTile[]; className?: string }) {
  return (
    <section
      aria-label="Team capacity"
      data-testid="team-capacity-strip"
      className={cn(
        // Five tiles stay on one row from the tablet breakpoint up: wrapping
        // them 3+2 spent a whole extra row of the first viewport at 900x1180.
        "flex snap-x gap-2 overflow-x-auto rounded-xl border border-border/70 bg-card/60 p-1.5 sm:grid sm:grid-cols-5 sm:overflow-visible",
        className,
      )}
    >
      {tiles.map((tile) => {
        const Icon = TILE_ICON[tile.id];
        return (
          <div
            key={tile.id}
            data-capacity-tile={tile.id}
            className="min-w-[8.5rem] shrink-0 snap-start rounded-lg px-2.5 py-1.5 sm:min-w-0"
          >
            <span className="flex min-w-0 items-center gap-1.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              <Icon className="size-3 shrink-0" />
              <span className="truncate">{tile.label}</span>
            </span>
            <span className="mt-1 flex items-baseline gap-1">
              <StatusDot tone={tile.tone} />
              <strong className="text-lg font-semibold leading-none tabular-nums text-foreground">{tile.value}</strong>
              {tile.total != null && (
                <span className="text-[10px] text-muted-foreground">of {tile.total}</span>
              )}
            </span>
            {tile.detail && <span className="mt-0.5 block truncate text-[9px] text-status-bad" title={tile.detail}>{tile.detail}</span>}
          </div>
        );
      })}
    </section>
  );
}
