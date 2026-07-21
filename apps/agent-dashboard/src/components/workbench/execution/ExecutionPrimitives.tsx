import type { ReactNode } from "react";
import { CircleAlert } from "lucide-react";

import { cn } from "@/lib/utils";

export function LiveTrace({
  axis = "horizontal",
  className,
}: {
  axis?: "horizontal" | "vertical";
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "execution-live-trace",
        axis === "vertical" && "execution-live-trace--vertical",
        className,
      )}
    />
  );
}

export function ReadinessMeter({
  value,
  total,
  label = "criteria ready",
  className,
}: {
  value: number;
  total: number;
  label?: string;
  className?: string;
}) {
  const safeTotal = Math.max(1, total);
  const safeValue = Math.min(Math.max(0, value), safeTotal);
  const percent = Math.round((safeValue / safeTotal) * 100);

  return (
    <div className={cn("min-w-0", className)}>
      <div className="flex items-end justify-between gap-3">
        <strong className="text-xl font-semibold tracking-tight text-foreground">
          {safeValue} / {total}
        </strong>
        <span className="pb-0.5 text-[10px] text-muted-foreground">{label}</span>
      </div>
      <div
        role="progressbar"
        aria-label={`${safeValue} of ${total} ${label}`}
        aria-valuemin={0}
        aria-valuemax={total}
        aria-valuenow={safeValue}
        className="mt-2 h-1 overflow-hidden rounded-full bg-border/80"
      >
        <span className="block h-full rounded-full bg-status-running transition-[width] duration-500 motion-reduce:transition-none" style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

export function DecisionAnchor({
  title,
  detail,
  actionLabel,
  onAction,
  compact = false,
  className,
}: {
  title: ReactNode;
  detail?: ReactNode;
  actionLabel: string;
  onAction: () => void;
  compact?: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "decision-anchor relative flex min-w-0 items-center gap-2.5 rounded-lg border border-primary/25 bg-primary/[0.055] text-foreground shadow-[0_8px_24px_-20px_hsl(var(--primary))]",
        compact ? "px-2.5 py-2" : "px-3 py-2.5",
        className,
      )}
    >
      <span className="grid size-7 shrink-0 place-items-center rounded-full bg-primary/10 text-primary">
        <CircleAlert className="size-3.5" aria-hidden />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-[11px] font-semibold leading-snug">{title}</span>
        {detail && <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{detail}</span>}
      </span>
      <button
        type="button"
        onClick={onAction}
        className="shrink-0 rounded-md bg-primary px-2.5 py-1.5 text-[10px] font-semibold text-primary-foreground shadow-sm transition-[transform,background-color] hover:-translate-y-px hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 motion-reduce:transform-none"
      >
        {actionLabel}
      </button>
    </div>
  );
}
