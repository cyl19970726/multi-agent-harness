import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

/**
 * Operator forms: the dialogs an operator uses to drive the team with ZERO CLI
 * (WP-iii). Each dialog collects the fields the matching WP-ii create route
 * requires, then hands a typed payload to its `onSubmit` so the surface can
 * dispatch the right action descriptor. The dialog owns nothing about the
 * harness wire shape — it only gathers input and reports validity.
 *
 * Built on plain React (no new modal dependency) so it stays inside the
 * existing dark operator-console design system + primitives.
 */

const ACTIONS_DISABLED_HINT = "Connect a live source to enable actions";

/** A lightweight modal: fixed overlay + centered panel, Escape-to-close, focus on open. */
export function Dialog({
  open,
  title,
  description,
  onClose,
  children,
}: {
  open: boolean;
  title: string;
  description?: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    // Focus the first focusable control so the form is keyboard-usable at once.
    const first = panelRef.current?.querySelector<HTMLElement>(
      "input, textarea, select, button",
    );
    first?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-background/70 p-4 backdrop-blur-sm sm:items-center"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="rise w-full max-w-md rounded-xl border border-border bg-popover shadow-2xl"
      >
        <div className="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold tracking-tight text-foreground">{title}</h2>
            {description && (
              <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>
            )}
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
        <div className="p-4">{children}</div>
      </div>
    </div>
  );
}

/** Labelled form field wrapper. */
export function Field({
  label,
  hint,
  required,
  children,
}: {
  label: string;
  hint?: string;
  required?: boolean;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="space-y-1">
      <label htmlFor={id} className="block text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
        {label}
        {required && <span className="ml-1 text-status-bad">*</span>}
      </label>
      {children(id)}
      {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  );
}

const inputClass =
  "h-9 w-full rounded-md border border-border bg-background px-2.5 text-[13px] text-foreground outline-none transition-colors focus:border-ring placeholder:text-muted-foreground/70";

export function TextInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cn(inputClass, props.className)} />;
}

export function TextArea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      rows={3}
      {...props}
      className={cn(
        "min-h-16 w-full resize-y rounded-md border border-border bg-background px-2.5 py-2 text-[13px] text-foreground outline-none transition-colors focus:border-ring placeholder:text-muted-foreground/70",
        props.className,
      )}
    />
  );
}

export function Select(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select {...props} className={cn(inputClass, "appearance-none", props.className)}>
      {props.children}
    </select>
  );
}

/** Footer row: cancel + a submit button that is honest about read-only mode. */
export function DialogFooter({
  onCancel,
  onSubmit,
  submitLabel,
  actionsEnabled,
  canSubmit,
}: {
  onCancel: () => void;
  onSubmit: () => void;
  submitLabel: string;
  actionsEnabled: boolean;
  canSubmit: boolean;
}) {
  return (
    <div className="mt-4 flex items-center justify-end gap-2">
      <Button variant="secondary" size="sm" type="button" onClick={onCancel}>
        Cancel
      </Button>
      {actionsEnabled ? (
        <Button size="sm" type="submit" onClick={onSubmit} disabled={!canSubmit}>
          {submitLabel}
        </Button>
      ) : (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="inline-flex">
              <Button size="sm" type="button" disabled title={ACTIONS_DISABLED_HINT}>
                {submitLabel}
              </Button>
            </span>
          </TooltipTrigger>
          <TooltipContent side="top">{ACTIONS_DISABLED_HINT}</TooltipContent>
        </Tooltip>
      )}
    </div>
  );
}

/** Parse a comma/newline-separated list into trimmed, non-empty entries. */
export function parseList(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}
