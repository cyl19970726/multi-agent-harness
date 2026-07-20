import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, Pin } from "lucide-react";

import { cn } from "@/lib/utils";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";

/**
 * The right-side context area shared by Focus, Team, and Wave pages.
 *
 * It deliberately accepts composition through children rather than a fixed
 * data model: a MemberRun needs Wave + Team + runtime context, while a
 * StandingAgent needs availability + capabilities. Callers decide that
 * composition without teaching this primitive either object's semantics.
 */
export function ContextRail({
  children,
  label = "Context",
  className,
  contentClassName,
}: {
  children: ReactNode;
  label?: string;
  className?: string;
  contentClassName?: string;
}) {
  return (
    <aside
      aria-label={label}
      className={cn(
        "min-h-0 bg-sidebar xl:border-l xl:border-border",
        className,
      )}
    >
      <div className="flex h-full min-h-0 flex-col">
        <div className="border-b border-border px-4 py-3 xl:px-4">
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {label}
          </p>
        </div>
        <div className={cn("min-h-0 space-y-2.5 overflow-y-auto p-3", contentClassName)}>{children}</div>
      </div>
    </aside>
  );
}

/** A rail module with optional disclosure. Modules are intentionally small,
 * so a page can compose Wave / Team / Member / Runtime context in priority
 * order instead of forcing another tab bar. */
export function ContextModule({
  title,
  kicker,
  icon,
  tone,
  live = false,
  action,
  defaultOpen = true,
  collapsible = false,
  pinned = false,
  className,
  children,
}: {
  title: ReactNode;
  kicker?: string;
  icon?: ReactNode;
  tone?: StatusTone;
  /** Use only for an actual changing runtime value, not decorative motion. */
  live?: boolean;
  action?: ReactNode;
  defaultOpen?: boolean;
  collapsible?: boolean;
  pinned?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const showBody = !collapsible || open;

  return (
    <section
      className={cn(
        "overflow-hidden rounded-lg border border-border bg-card",
        className,
      )}
    >
      <div className="flex min-w-0 items-center gap-2 px-3 py-2.5">
        {collapsible ? (
          <button
            type="button"
            onClick={() => setOpen((value) => !value)}
            aria-expanded={open}
            className="flex min-w-0 flex-1 items-center gap-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {open ? (
              <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
            ) : (
              <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
            )}
            <ContextModuleTitle
              title={title}
              kicker={kicker}
              icon={icon}
              tone={tone}
              live={live}
              pinned={pinned}
            />
          </button>
        ) : (
          <ContextModuleTitle
            title={title}
            kicker={kicker}
            icon={icon}
            tone={tone}
            live={live}
            pinned={pinned}
          />
        )}
        {action && <div className="ml-auto shrink-0">{action}</div>}
      </div>
      {showBody && <div className="border-t border-border/70 px-3 py-3">{children}</div>}
    </section>
  );
}

function ContextModuleTitle({
  title,
  kicker,
  icon,
  tone,
  live,
  pinned,
}: {
  title: ReactNode;
  kicker?: string;
  icon?: ReactNode;
  tone?: StatusTone;
  live: boolean;
  pinned: boolean;
}) {
  return (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      {icon && <span className="shrink-0 text-muted-foreground">{icon}</span>}
      {tone && <StatusDot tone={tone} pulse={live && tone === "running"} />}
      <span className="min-w-0 flex-1">
        {kicker && (
          <span className="block truncate text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">
            {kicker}
          </span>
        )}
        <span className="block truncate text-[12px] font-semibold leading-snug text-foreground">
          {title}
        </span>
      </span>
      {pinned && <Pin className="size-3 shrink-0 text-muted-foreground" aria-label="Pinned" />}
    </span>
  );
}
