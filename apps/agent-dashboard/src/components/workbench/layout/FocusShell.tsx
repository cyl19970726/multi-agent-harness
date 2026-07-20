import type { ReactNode } from "react";
import { ChevronDown, PanelsTopLeft } from "lucide-react";

import { cn } from "@/lib/utils";

/**
 * Codex-like focus layout: a continuous main work surface, with a composed
 * context rail that supplements rather than fragments the activity stream.
 *
 * The shell has no object-specific assumptions. MemberRun, StandingAgent and
 * WorkflowRun callers supply their own header, stream, composer, and context.
 */
export function FocusShell({
  header,
  children,
  composer,
  context,
  className,
  mainClassName,
}: {
  header?: ReactNode;
  children: ReactNode;
  composer?: ReactNode;
  context?: ReactNode;
  className?: string;
  mainClassName?: string;
}) {
  return (
    <div
      className={cn(
        "grid min-h-0 flex-1 grid-cols-1 grid-rows-1 bg-background xl:grid-cols-[minmax(0,1fr)_23rem]",
        className,
      )}
    >
      <section className="flex min-h-0 min-w-0 flex-col">
        {header && <div className="border-b border-border bg-card px-4 py-3 sm:px-5">{header}</div>}
        <main className={cn("min-h-0 flex-1 overflow-y-auto", mainClassName)}>{children}</main>
        {context && (
          <details className="group shrink-0 border-t border-border bg-card xl:hidden">
            <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-2.5 text-[12px] font-semibold text-foreground marker:content-none sm:px-5">
              <PanelsTopLeft className="size-3.5 text-primary" />
              Context & controls
              <ChevronDown className="ml-auto size-3.5 text-muted-foreground transition-transform group-open:rotate-180" />
            </summary>
            <div className="max-h-[55vh] overflow-y-auto border-t border-border">{context}</div>
          </details>
        )}
        {composer && (
          <footer className="border-t border-border bg-card px-4 py-3 sm:px-5">{composer}</footer>
        )}
      </section>
      {context && (
        <div className="hidden min-h-0 xl:block">{context}</div>
      )}
    </div>
  );
}

/** Header content shared by focus pages, intentionally separate from the
 * shell so a MemberRun and StandingAgent can use different semantic context. */
export function FocusHeader({
  eyebrow,
  title,
  description,
  breadcrumb,
  meta,
  actions,
  className,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  breadcrumb?: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cn("flex min-w-0 flex-wrap items-start justify-between gap-3", className)}>
      <div className="min-w-0 flex-1 space-y-1">
        {breadcrumb && <div className="min-w-0 text-[11px] text-muted-foreground">{breadcrumb}</div>}
        {eyebrow && (
          <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {eyebrow}
          </p>
        )}
        <h1 className="min-w-0 text-lg font-semibold tracking-tight text-foreground sm:truncate">{title}</h1>
        {description && <p className="max-w-3xl text-[13px] text-muted-foreground">{description}</p>}
        {meta && <div className="flex flex-wrap items-center gap-2 pt-1">{meta}</div>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}
