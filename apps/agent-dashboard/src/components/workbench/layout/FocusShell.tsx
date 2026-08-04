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
  headerClassName,
  composerClassName,
  responsiveContextVariant = "inline",
  splitMobileToolbar = false,
  mainLabel,
}: {
  header?: ReactNode;
  children: ReactNode;
  composer?: ReactNode;
  context?: ReactNode;
  className?: string;
  mainClassName?: string;
  headerClassName?: string;
  composerClassName?: string;
  responsiveContextVariant?: "inline" | "sheet";
  /**
   * Opt-in: below `sm`, put the composer and the context disclosure side by
   * side in one toolbar row instead of stacking two full-width bands. Off by
   * default so surfaces that did not ask for it keep their exact layout.
   */
  splitMobileToolbar?: boolean;
  mainLabel?: string;
}) {
  return (
    <div
      className={cn(
        "grid min-h-0 flex-1 grid-cols-1 grid-rows-1 bg-background xl:grid-cols-[minmax(0,1fr)_23.625rem]",
        className,
      )}
    >
      <section className="flex min-h-0 min-w-0 flex-col">
        {header && <div className={cn("border-b border-border bg-card px-4 py-3 sm:px-5", headerClassName)}>{header}</div>}
        <main
          className={cn("min-h-0 flex-1 overflow-y-auto", mainClassName)}
          data-member-history-scroll-owner={mainLabel ? "true" : undefined}
          aria-label={mainLabel}
          tabIndex={mainLabel ? 0 : undefined}
        >{children}</main>
        {/* `display: contents` by default, so this wrapper never becomes a flex
            item of the shell column and every surface that did not opt in keeps
            its exact previous layout. It only materialises as a real flex row
            below `sm`, and only for callers that asked for the split toolbar. */}
        <div className={cn("contents", splitMobileToolbar && "max-sm:flex max-sm:items-stretch max-sm:border-t max-sm:border-border")}>
          {context && (
            <details
              data-shell-context-disclosure="true"
              className={cn(
                "group shrink-0 border-t border-border bg-card xl:hidden",
                responsiveContextVariant === "sheet" && "responsive-context-sheet",
                // Visual order is composer-then-context; DOM order stays
                // context-then-composer so no other surface's reading order moves.
                splitMobileToolbar && "max-sm:order-2 max-sm:min-w-0 max-sm:flex-1 max-sm:border-l max-sm:border-t-0",
              )}
            >
              <summary className={cn(
                "flex cursor-pointer list-none items-center gap-2 px-4 py-2.5 text-[12px] font-semibold text-foreground marker:content-none sm:px-5",
                splitMobileToolbar && "max-sm:h-11 max-sm:justify-center max-sm:gap-1.5 max-sm:px-2 max-sm:py-0",
              )}>
                <PanelsTopLeft className="size-3.5 shrink-0 text-primary" />
                <span className="truncate">Context &amp; controls</span>
                <ChevronDown className={cn(
                  "ml-auto size-3.5 shrink-0 text-muted-foreground transition-transform group-open:rotate-180",
                  splitMobileToolbar && "max-sm:ml-0",
                )} />
              </summary>
              <div className="max-h-[55vh] overflow-y-auto border-t border-border">{context}</div>
            </details>
          )}
          {composer && (
            <footer className={cn(
              "border-t border-border bg-card px-4 py-3 sm:px-5",
              splitMobileToolbar && "max-sm:order-1 max-sm:min-w-0 max-sm:flex-1 max-sm:border-t-0",
              composerClassName,
            )}>{composer}</footer>
          )}
        </div>
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
        {description && <p className="line-clamp-2 max-w-3xl text-[13px] leading-relaxed text-muted-foreground">{description}</p>}
        {meta && <div className="flex flex-wrap items-center gap-2 pt-1">{meta}</div>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}
