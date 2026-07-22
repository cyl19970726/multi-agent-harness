import * as React from "react";
import { ChevronDown, ChevronRight, type LucideIcon } from "lucide-react";

import { cn } from "@/lib/utils";

export type StatusTone =
  | "running"
  | "good"
  | "warn"
  | "bad"
  | "info"
  | "decision"
  | "idle";

export const toneText: Record<StatusTone, string> = {
  running: "text-status-running",
  good: "text-status-good",
  warn: "text-status-warn",
  bad: "text-status-bad",
  info: "text-status-info",
  decision: "text-status-decision",
  idle: "text-status-idle",
};

export const toneBg: Record<StatusTone, string> = {
  running: "bg-status-running",
  good: "bg-status-good",
  warn: "bg-status-warn",
  bad: "bg-status-bad",
  info: "bg-status-info",
  decision: "bg-status-decision",
  idle: "bg-status-idle",
};

/** A small status dot; `pulse` adds the live ring animation for running state. */
export function StatusDot({
  tone = "idle",
  pulse = false,
  className,
}: {
  tone?: StatusTone;
  pulse?: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-block size-2 shrink-0 rounded-full",
        toneBg[tone],
        pulse && "pulse-dot",
        toneText[tone],
        className,
      )}
    />
  );
}

/** Surface-level header: kicker (uppercase), title, optional description + actions. */
export function SurfaceHeader({
  kicker,
  title,
  description,
  actions,
  className,
}: {
  kicker?: string;
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <header
      className={cn("flex flex-wrap items-start justify-between gap-x-4 gap-y-3", className)}
    >
      {/* min-w keeps the title readable; when the actions can't fit beside it the
          flex-wrap drops them to their own row instead of collapsing the title. */}
      <div className="min-w-[15rem] flex-1 space-y-1">
        {kicker && (
          <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            {kicker}
          </p>
        )}
        <h1 className="truncate text-lg font-semibold tracking-tight text-foreground">
          {title}
        </h1>
        {description && (
          <p className="max-w-2xl text-sm text-muted-foreground">{description}</p>
        )}
      </div>
      {actions && (
        <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>
      )}
    </header>
  );
}

/** A bordered content panel with a header row. */
export function Section({
  title,
  kicker,
  action,
  className,
  bodyClassName,
  children,
}: {
  title: React.ReactNode;
  kicker?: string;
  action?: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  children: React.ReactNode;
}) {
  return (
    <section
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border border-border bg-card",
        className,
      )}
    >
      <header className="flex items-center justify-between gap-2 border-b border-border px-3.5 py-2.5">
        <div className="min-w-0">
          {kicker && (
            <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {kicker}
            </p>
          )}
          <h2 className="truncate text-[13px] font-semibold text-foreground">
            {title}
          </h2>
        </div>
        {action && <div className="flex shrink-0 items-center gap-1.5">{action}</div>}
      </header>
      <div className={cn("min-h-0 flex-1", bodyClassName)}>{children}</div>
    </section>
  );
}

/**
 * Centered Notion-style document column for text-heavy detail pages. Caps line
 * length for readability and sets vertical rhythm between
 * document sections.
 */
export function DocumentSurface({
  children,
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & {
  children: React.ReactNode;
}) {
  return (
    <div className={cn("mx-auto w-full max-w-[800px] space-y-7", className)} {...props}>
      {children}
    </div>
  );
}

/**
 * Borderless document section: a small uppercase label (optional action) over
 * content, separated by whitespace rather than card borders. The Notion-style
 * counterpart to the bordered `Section` used on dashboards/boards.
 */
export function DocSection({
  label,
  action,
  className,
  children,
}: {
  label?: string;
  action?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <section className={cn("space-y-2.5", className)}>
      {(label || action) && (
        <div className="flex items-center justify-between gap-2">
          {label && (
            <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
              {label}
            </h2>
          )}
          {action && <div className="flex shrink-0 items-center gap-1.5">{action}</div>}
        </div>
      )}
      {children}
    </section>
  );
}

/**
 * Tiny inline-SVG bar sparkline for per-agent N-day activity. No chart deps;
 * empty/all-zero data renders flat baseline bars so the column never looks broken.
 */
export function AgentSparkline({
  data,
  className,
  width = 64,
  height = 18,
}: {
  data: number[];
  className?: string;
  width?: number;
  height?: number;
}) {
  const max = Math.max(1, ...data);
  const n = Math.max(1, data.length);
  const gap = 1.5;
  const barW = (width - gap * (n - 1)) / n;
  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      className={cn("text-status-info/70", className)}
      aria-hidden
    >
      {data.map((value, index) => {
        const h = Math.max(1, Math.round((value / max) * (height - 1)));
        const x = index * (barW + gap);
        return (
          <rect
            key={index}
            x={x}
            y={height - h}
            width={barW}
            height={h}
            rx={0.75}
            className={value > 0 ? "fill-current" : "fill-muted-foreground/25"}
          />
        );
      })}
    </svg>
  );
}

/**
 * A single-label block whose body collapses behind a chevron header. Used in
 * the agent Config rail/tab so each named concept (Skills / Runtime / env /
 * params / MCP) is one scannable row that opens on demand. `defaultOpen`
 * controls the initial state; `action` rides in the header.
 */
export function CollapsibleBlock({
  label,
  defaultOpen = false,
  action,
  className,
  children,
}: {
  label: string;
  defaultOpen?: boolean;
  action?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}) {
  const [open, setOpen] = React.useState(defaultOpen);
  return (
    <section className={cn("rounded-lg border border-border bg-card", className)}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-2.5 text-left transition-colors hover:bg-accent/40"
      >
        {open ? (
          <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        <span className="flex-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          {label}
        </span>
        {action && <span className="shrink-0">{action}</span>}
      </button>
      {open && <div className="border-t border-border/60 px-3 py-3">{children}</div>}
    </section>
  );
}

/** Notion-style page properties: borderless label/value rows at the top of a doc. */
export function DocProperties({
  items,
  className,
}: {
  items: { label: string; value: React.ReactNode }[];
  className?: string;
}) {
  return (
    <dl className={cn("space-y-2", className)}>
      {items.map((item) => (
        <div key={item.label} className="flex items-start gap-3 text-[13px]">
          <dt className="w-32 shrink-0 text-muted-foreground">{item.label}</dt>
          <dd className="min-w-0 flex-1 break-words text-foreground">{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Definition-list style metadata. */
export function MetaList({
  items,
  className,
}: {
  items: { label: string; value: React.ReactNode }[];
  className?: string;
}) {
  return (
    <dl className={cn("grid gap-2.5", className)}>
      {items.map((item) => (
        <div key={item.label} className="grid grid-cols-[7rem_1fr] gap-3">
          <dt className="text-[11px] uppercase tracking-wide text-muted-foreground">
            {item.label}
          </dt>
          <dd className="min-w-0 break-words text-[13px] text-foreground">
            {item.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-muted px-1.5 font-mono text-[10px] text-muted-foreground">
      {children}
    </kbd>
  );
}

export function MonoId({ children }: { children: React.ReactNode }) {
  return (
    <span className="font-mono text-[11px] text-muted-foreground">{children}</span>
  );
}

export function EmptyState({
  icon: Icon,
  title,
  description,
}: {
  icon?: LucideIcon;
  title: string;
  description?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-1.5 px-6 py-10 text-center">
      {Icon && <Icon className="size-5 text-muted-foreground" aria-hidden />}
      <p className="text-sm font-medium text-foreground">{title}</p>
      {description && (
        <p className="max-w-xs text-xs text-muted-foreground">{description}</p>
      )}
    </div>
  );
}

/** A canonical-activity / timeline row that links back to a harness object. */
export function TimelineRow({
  kind,
  title,
  meta,
  body,
  tone = "idle",
  onClick,
}: {
  kind: string;
  title: React.ReactNode;
  meta?: React.ReactNode;
  body?: React.ReactNode;
  tone?: StatusTone;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "group flex w-full items-start gap-3 border-b border-border/60 px-3.5 py-2.5 text-left transition-colors last:border-b-0 hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
      )}
    >
      <span className="mt-1 flex shrink-0 items-center gap-2">
        <StatusDot tone={tone} pulse={tone === "running"} />
      </span>
      <span className="min-w-0 flex-1 space-y-0.5">
        <span className="flex items-center gap-2">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {kind}
          </span>
          {meta && (
            <span className="truncate text-[11px] text-muted-foreground">{meta}</span>
          )}
        </span>
        <span className="block truncate text-[13px] font-medium text-foreground">
          {title}
        </span>
        {body && (
          <span className="block truncate text-xs text-muted-foreground">{body}</span>
        )}
      </span>
    </button>
  );
}
